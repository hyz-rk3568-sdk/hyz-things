use crate::{
    application::ports::{
        PlatformError, SubscriptionProviderState, SubscriptionResolverPort, SubscriptionSourcePort,
        SubscriptionSourceState, SubscriptionStorePort, SubscriptionTransportPort,
    },
    domain::subscription::{
        compose_managed_mihomo_source, validate_dns_results, GenerationId, SubscriptionStatus,
        SubscriptionUrl, ValidatedSubscription, MAX_SUBSCRIPTION_BYTES, MAX_SUBSCRIPTION_URL_BYTES,
    },
};
use serde_yaml::Value;
use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use ureq::{
    config::Config,
    http::Uri,
    tls::{TlsConfig, TlsProvider},
    unversioned::{
        resolver::{ResolvedSocketAddrs, Resolver},
        transport::NextTimeout,
    },
    Agent, Proxy, ProxyProtocol,
};
use zeroize::Zeroizing;

const O_NOFOLLOW: i32 = 0o400000;
const SUBSCRIPTION_USER_AGENT: &str = "clash.meta";
const SUBSCRIPTION_ACCEPT: &str = "application/yaml, text/yaml, text/plain";
const SUBSCRIPTION_ACCEPT_ENCODING: &str = "identity";
const SUBSCRIPTION_CONNECTION: &str = "close";
const SUBSCRIPTION_PROXY_HOST: &str = "127.0.0.1";
const SUBSCRIPTION_PROXY_PORT: u16 = 7_890;
#[cfg(test)]
const SUBSCRIPTION_PROXY_URL: &str = "http://127.0.0.1:7890/";
const URL_FILE: &str = "subscription.url";
const STATUS_FILE: &str = "status";
const CURRENT_FILE: &str = "current";
const GENERATIONS_DIR: &str = "generations";
const MAX_STATUS_BYTES: usize = 1_100;
const SUBSCRIPTION_PROVIDER_PATH: &str = "/userdata/hyz-router/mihomo/providers/subscription.yaml";
const SUBSCRIPTION_PROVIDER_REFERENCE: &str = "./providers/subscription.yaml";
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub const DEFAULT_SUBSCRIPTION_ROOT: &str = "/userdata/hyz-router/mihomo/subscription";

#[derive(Debug, Default)]
pub struct SystemSubscriptionResolver;

impl SubscriptionResolverPort for SystemSubscriptionResolver {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, PlatformError> {
        let addresses = (host, port)
            .to_socket_addrs()
            .map_err(|_| {
                PlatformError::ProbeFailed("subscription DNS resolution failed".to_owned())
            })?
            .collect::<Vec<_>>();
        let ips = addresses.iter().map(SocketAddr::ip).collect::<Vec<_>>();
        validate_dns_results(&ips).map_err(|_| {
            PlatformError::UnsafeToCutOver(
                "subscription DNS returned an empty or non-public address set".to_owned(),
            )
        })?;
        Ok(addresses)
    }
}

#[derive(Clone)]
struct PinnedResolver {
    resolver: Arc<dyn SubscriptionResolverPort>,
}

impl fmt::Debug for PinnedResolver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PinnedResolver")
    }
}

impl Resolver for PinnedResolver {
    fn resolve(
        &self,
        uri: &Uri,
        _config: &Config,
        _timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let host = uri.host().ok_or(ureq::Error::HostNotFound)?;
        let port = uri.port_u16().unwrap_or(443);
        let addresses = self
            .resolver
            .resolve(host, port)
            .map_err(|_| ureq::Error::HostNotFound)?;
        let ips = addresses.iter().map(SocketAddr::ip).collect::<Vec<_>>();
        if addresses.iter().any(|address| address.port() != port)
            || validate_dns_results(&ips).is_err()
        {
            return Err(ureq::Error::HostNotFound);
        }
        let mut resolved = self.empty();
        for address in addresses.into_iter().take(16) {
            resolved.push(address);
        }
        if resolved.is_empty() {
            Err(ureq::Error::HostNotFound)
        } else {
            Ok(resolved)
        }
    }
}

#[derive(Clone, Debug)]
struct SubscriptionProxyResolver {
    target: PinnedResolver,
}

impl Resolver for SubscriptionProxyResolver {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        // Only the fixed local Mihomo listener bypasses public-address validation.
        if uri.host() == Some(SUBSCRIPTION_PROXY_HOST)
            && uri.port_u16() == Some(SUBSCRIPTION_PROXY_PORT)
        {
            let mut resolved = self.empty();
            resolved.push(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                SUBSCRIPTION_PROXY_PORT,
            ));
            return Ok(resolved);
        }
        self.target.resolve(uri, config, timeout)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubscriptionRoute {
    Direct,
    MihomoProxy,
}

fn fetch_subscription_with_fallback<F>(url: &str, mut fetch: F) -> Result<Vec<u8>, PlatformError>
where
    F: FnMut(SubscriptionRoute, &str) -> Result<Vec<u8>, PlatformError>,
{
    match fetch(SubscriptionRoute::Direct, url) {
        Ok(bytes) => Ok(bytes),
        Err(direct_error) => fetch(SubscriptionRoute::MihomoProxy, url).or(Err(direct_error)),
    }
}

fn subscription_agent_config(proxy: Option<Proxy>) -> Config {
    Agent::config_builder()
        .https_only(true)
        .proxy(proxy)
        .max_redirects(0)
        .user_agent(SUBSCRIPTION_USER_AGENT)
        .accept(SUBSCRIPTION_ACCEPT)
        .accept_encoding(SUBSCRIPTION_ACCEPT_ENCODING)
        .timeout_global(Some(Duration::from_secs(30)))
        .tls_config(TlsConfig::builder().provider(TlsProvider::Rustls).build())
        .build()
}

#[derive(Clone)]
pub struct UreqSubscriptionTransport {
    direct_agent: Agent,
    proxy_agent: Agent,
}

impl UreqSubscriptionTransport {
    pub fn new(resolver: Arc<dyn SubscriptionResolverPort>) -> Self {
        let direct_agent = Agent::with_parts(
            subscription_agent_config(None),
            ureq::unversioned::transport::DefaultConnector::default(),
            PinnedResolver {
                resolver: resolver.clone(),
            },
        );
        let proxy = Proxy::builder(ProxyProtocol::Http)
            .host(SUBSCRIPTION_PROXY_HOST)
            .port(SUBSCRIPTION_PROXY_PORT)
            .resolve_target(false)
            .build()
            .expect("fixed Mihomo subscription proxy must be valid");
        let proxy_agent = Agent::with_parts(
            subscription_agent_config(Some(proxy)),
            ureq::unversioned::transport::DefaultConnector::default(),
            SubscriptionProxyResolver {
                target: PinnedResolver { resolver },
            },
        );
        Self {
            direct_agent,
            proxy_agent,
        }
    }

    fn fetch_with_agent(agent: &Agent, url: &str) -> Result<Vec<u8>, PlatformError> {
        let mut response = agent
            .get(url)
            .header("Connection", SUBSCRIPTION_CONNECTION)
            .call()
            .map_err(|_| PlatformError::Io("subscription HTTPS request failed".to_owned()))?;
        if response.status().is_redirection() {
            return Err(PlatformError::UnsafeToCutOver(
                "subscription redirects are forbidden".to_owned(),
            ));
        }
        if response.headers().contains_key("content-encoding") {
            return Err(PlatformError::InvalidState(
                "encoded subscription responses are forbidden".to_owned(),
            ));
        }
        if response
            .body()
            .content_length()
            .is_some_and(|length| length > MAX_SUBSCRIPTION_BYTES as u64)
        {
            return Err(PlatformError::InvalidState(
                "subscription response exceeds size limit".to_owned(),
            ));
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit(MAX_SUBSCRIPTION_BYTES as u64 + 1)
            .read_to_vec()
            .map_err(|_| PlatformError::Io("could not read subscription response".to_owned()))?;
        if bytes.is_empty() || bytes.len() > MAX_SUBSCRIPTION_BYTES {
            return Err(PlatformError::InvalidState(
                "subscription response is empty or exceeds size limit".to_owned(),
            ));
        }
        Ok(bytes)
    }
}

impl SubscriptionTransportPort for UreqSubscriptionTransport {
    fn fetch(&self, url: &SubscriptionUrl) -> Result<Vec<u8>, PlatformError> {
        let mut secret = Zeroizing::new(Vec::with_capacity(MAX_SUBSCRIPTION_URL_BYTES));
        url.write_secret(&mut *secret)
            .map_err(|_| PlatformError::Io("could not encode subscription URL".to_owned()))?;
        let secret = Zeroizing::new(String::from_utf8(std::mem::take(&mut *secret)).map_err(
            |_| PlatformError::InvalidState("subscription URL encoding is invalid".to_owned()),
        )?);
        fetch_subscription_with_fallback(secret.as_str(), |route, request_url| match route {
            SubscriptionRoute::Direct => Self::fetch_with_agent(&self.direct_agent, request_url),
            SubscriptionRoute::MihomoProxy => {
                Self::fetch_with_agent(&self.proxy_agent, request_url)
            }
        })
    }
}

#[derive(Clone, Debug)]
pub struct SubscriptionStore {
    root: PathBuf,
}

impl Default for SubscriptionStore {
    fn default() -> Self {
        Self::new(DEFAULT_SUBSCRIPTION_ROOT)
    }
}

impl SubscriptionStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn initialize(&self) -> Result<(), SubscriptionStorageError> {
        validate_absolute_path(&self.root)?;
        create_secure_directory_chain(&self.root)?;
        fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))
            .map_err(storage_io("chmod subscription root"))?;

        let generations = self.generations_path();
        fs::create_dir(&generations)
            .or_else(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .map_err(storage_io("create generations directory"))?;
        require_root_directory(&generations)?;
        fs::set_permissions(&generations, fs::Permissions::from_mode(0o700))
            .map_err(storage_io("chmod generations directory"))?;
        Ok(())
    }

    pub fn store_url(&self, url: &SubscriptionUrl) -> Result<(), SubscriptionStorageError> {
        self.initialize()?;
        atomic_write(&self.root, URL_FILE, |file| {
            url.write_secret(file)
                .map_err(storage_io("write subscription URL"))
        })
    }

    pub fn clear_url(&self) -> Result<(), SubscriptionStorageError> {
        self.initialize()?;
        let path = self.root.join(URL_FILE);
        let _ = read_optional_private(&path, MAX_SUBSCRIPTION_URL_BYTES)?;
        match fs::remove_file(&path) {
            Ok(()) => sync_directory(&self.root),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(storage_io("remove subscription URL")(error)),
        }
    }

    pub fn load_url(&self) -> Result<Option<SubscriptionUrl>, SubscriptionStorageError> {
        self.initialize()?;
        let Some(bytes) =
            read_optional_private(&self.root.join(URL_FILE), MAX_SUBSCRIPTION_URL_BYTES)?
        else {
            return Ok(None);
        };
        let value = String::from_utf8(bytes)
            .map_err(|_| SubscriptionStorageError::InvalidState("stored URL is not UTF-8"))?;
        SubscriptionUrl::parse(value)
            .map(Some)
            .map_err(|_| SubscriptionStorageError::InvalidState("stored URL is invalid"))
    }

    pub fn store_status(
        &self,
        status: &SubscriptionStatus,
    ) -> Result<(), SubscriptionStorageError> {
        if let SubscriptionStatus::Failed(message) = status {
            SubscriptionStatus::failed(message.as_str())
                .map_err(|_| SubscriptionStorageError::InvalidState("failure status is invalid"))?;
        }
        self.initialize()?;
        atomic_write(&self.root, STATUS_FILE, |file| {
            match status {
                SubscriptionStatus::Idle => file.write_all(b"idle\n"),
                SubscriptionStatus::Fetching => file.write_all(b"fetching\n"),
                SubscriptionStatus::Active(generation) => {
                    writeln!(file, "active:{}", generation.as_str())
                }
                SubscriptionStatus::Failed(message) => writeln!(file, "failed:{message}"),
            }
            .map_err(storage_io("write subscription status"))
        })
    }

    pub fn load_status(&self) -> Result<Option<SubscriptionStatus>, SubscriptionStorageError> {
        self.initialize()?;
        let Some(bytes) = read_optional_private(&self.root.join(STATUS_FILE), MAX_STATUS_BYTES)?
        else {
            return Ok(None);
        };
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| SubscriptionStorageError::InvalidState("stored status is not UTF-8"))?;
        let text = text.strip_suffix('\n').unwrap_or(text);
        let status = match text {
            "idle" => SubscriptionStatus::Idle,
            "fetching" => SubscriptionStatus::Fetching,
            _ if text.starts_with("active:") => {
                SubscriptionStatus::Active(GenerationId::parse(&text[7..]).map_err(|_| {
                    SubscriptionStorageError::InvalidState("stored generation is invalid")
                })?)
            }
            _ if text.starts_with("failed:") => {
                SubscriptionStatus::failed(&text[7..]).map_err(|_| {
                    SubscriptionStorageError::InvalidState("stored failure status is invalid")
                })?
            }
            _ => {
                return Err(SubscriptionStorageError::InvalidState(
                    "stored status is invalid",
                ))
            }
        };
        Ok(Some(status))
    }

    /// Creates an immutable generation without changing the active pointer.
    pub fn stage_generation(
        &self,
        generation: &GenerationId,
        subscription: &ValidatedSubscription,
    ) -> Result<(), SubscriptionStorageError> {
        self.initialize()?;
        let generations = self.generations_path();
        let destination = generations.join(format!("{}.yaml", generation.as_str()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW)
            .open(&destination)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    SubscriptionStorageError::GenerationExists
                } else {
                    SubscriptionStorageError::Io(format!("create generation: {error}"))
                }
            })?;
        if let Err(error) = file
            .write_all(subscription.as_bytes())
            .and_then(|_| file.sync_all())
        {
            let _ = fs::remove_file(&destination);
            return Err(SubscriptionStorageError::Io(format!(
                "write generation: {error}"
            )));
        }
        require_root_private_file(&destination)?;
        sync_directory(&generations)
    }

    /// Atomically changes the active pointer after all candidate validation and cutover succeeds.
    pub fn activate_generation(
        &self,
        generation: &GenerationId,
    ) -> Result<(), SubscriptionStorageError> {
        self.initialize()?;
        let path = self
            .generations_path()
            .join(format!("{}.yaml", generation.as_str()));
        require_root_private_file(&path)?;
        atomic_write(&self.root, CURRENT_FILE, |file| {
            writeln!(file, "{}", generation.as_str()).map_err(storage_io("write current pointer"))
        })
    }

    pub fn current_generation(&self) -> Result<Option<GenerationId>, SubscriptionStorageError> {
        self.initialize()?;
        let Some(bytes) = read_optional_private(&self.root.join(CURRENT_FILE), 66)? else {
            return Ok(None);
        };
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| SubscriptionStorageError::InvalidState("current pointer is not UTF-8"))?;
        GenerationId::parse(text.strip_suffix('\n').unwrap_or(text))
            .map(Some)
            .map_err(|_| SubscriptionStorageError::InvalidState("current pointer is invalid"))
    }

    pub fn read_generation(
        &self,
        generation: &GenerationId,
    ) -> Result<Vec<u8>, SubscriptionStorageError> {
        self.initialize()?;
        let path = self
            .generations_path()
            .join(format!("{}.yaml", generation.as_str()));
        read_optional_private(&path, MAX_SUBSCRIPTION_BYTES)?
            .ok_or(SubscriptionStorageError::GenerationNotFound)
    }

    fn generations_path(&self) -> PathBuf {
        self.root.join(GENERATIONS_DIR)
    }
}

impl SubscriptionStorePort for SubscriptionStore {
    fn store_url(&self, url: &SubscriptionUrl) -> Result<(), PlatformError> {
        SubscriptionStore::store_url(self, url).map_err(subscription_storage_error)
    }

    fn clear_url(&self) -> Result<(), PlatformError> {
        SubscriptionStore::clear_url(self).map_err(subscription_storage_error)
    }

    fn load_url(&self) -> Result<Option<SubscriptionUrl>, PlatformError> {
        SubscriptionStore::load_url(self).map_err(subscription_storage_error)
    }

    fn store_subscription_status(&self, status: &SubscriptionStatus) -> Result<(), PlatformError> {
        self.store_status(status)
            .map_err(subscription_storage_error)
    }

    fn load_subscription_status(&self) -> Result<Option<SubscriptionStatus>, PlatformError> {
        self.load_status().map_err(subscription_storage_error)
    }

    fn stage_generation(
        &self,
        generation: &GenerationId,
        subscription: &ValidatedSubscription,
    ) -> Result<(), PlatformError> {
        SubscriptionStore::stage_generation(self, generation, subscription)
            .map_err(subscription_storage_error)
    }

    fn activate_generation(&self, generation: &GenerationId) -> Result<(), PlatformError> {
        SubscriptionStore::activate_generation(self, generation).map_err(subscription_storage_error)
    }
}

impl SubscriptionSourcePort for super::process::LinuxRouterPlatform {
    fn load_source(&self) -> Result<SubscriptionSourceState, PlatformError> {
        let source = super::storage::read_private_small_optional(
            super::paths::MIHOMO_SOURCE_CONFIG,
            MAX_SUBSCRIPTION_BYTES,
        )?
        .map(String::into_bytes)
        .ok_or_else(|| PlatformError::InvalidState("Mihomo source config is absent".to_owned()))?;
        let provider = if source_uses_managed_subscription_provider(&source)? {
            let contents = super::storage::read_private_small_optional(
                SUBSCRIPTION_PROVIDER_PATH,
                MAX_SUBSCRIPTION_BYTES,
            )?
            .map(String::into_bytes);
            Some(SubscriptionProviderState { contents })
        } else {
            None
        };
        Ok(SubscriptionSourceState { source, provider })
    }

    fn prepare_candidate(
        &self,
        current_source: &[u8],
        subscription: &ValidatedSubscription,
        lan_tun_enabled: bool,
    ) -> Result<Vec<u8>, PlatformError> {
        let current_source = super::proxy::migrate_legacy_persisted_source_bytes(current_source)?;
        let candidate =
            compose_managed_mihomo_source(&current_source, subscription).map_err(|error| {
                PlatformError::InvalidState(format!(
                    "managed Mihomo subscription candidate was rejected: {error}"
                ))
            })?;
        if candidate.is_empty() || candidate.len() > MAX_SUBSCRIPTION_BYTES {
            return Err(PlatformError::InvalidState(
                "candidate config is empty or exceeds size limit".to_owned(),
            ));
        }
        self.validate_subscription_candidate(&candidate, lan_tun_enabled)?;
        Ok(candidate)
    }

    fn store_source(&self, state: &SubscriptionSourceState) -> Result<(), PlatformError> {
        std::str::from_utf8(&state.source)
            .map_err(|_| PlatformError::InvalidState("candidate source is not UTF-8".to_owned()))?;
        if let Some(provider) = &state.provider {
            match &provider.contents {
                Some(contents) => {
                    super::storage::atomic_write_private(SUBSCRIPTION_PROVIDER_PATH, contents)?;
                }
                None => remove_private_file_if_present(SUBSCRIPTION_PROVIDER_PATH)?,
            }
        }
        super::storage::atomic_write_private(super::paths::MIHOMO_SOURCE_CONFIG, &state.source)
    }
}

fn source_uses_managed_subscription_provider(source: &[u8]) -> Result<bool, PlatformError> {
    let document: Value = serde_yaml::from_slice(source).map_err(|_| {
        PlatformError::InvalidState("Mihomo source config is not valid YAML".to_owned())
    })?;
    let provider = document
        .get("proxy-providers")
        .and_then(Value::as_mapping)
        .and_then(|providers| providers.get(Value::String("subscription".to_owned())))
        .and_then(Value::as_mapping);
    Ok(provider
        .and_then(|provider| provider.get(Value::String("type".to_owned())))
        .and_then(Value::as_str)
        == Some("file")
        && provider
            .and_then(|provider| provider.get(Value::String("path".to_owned())))
            .and_then(Value::as_str)
            == Some(SUBSCRIPTION_PROVIDER_REFERENCE))
}

fn remove_private_file_if_present(path: &str) -> Result<(), PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            super::storage::require_private_root_file(path)?;
            super::storage::remove_file_durable(path)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PlatformError::Io(format!("inspect {path}: {error}"))),
    }
}

fn subscription_storage_error(error: SubscriptionStorageError) -> PlatformError {
    match error {
        SubscriptionStorageError::Io(reason) => PlatformError::Io(reason),
        other => PlatformError::InvalidState(other.to_string()),
    }
}

fn validate_absolute_path(path: &Path) -> Result<(), SubscriptionStorageError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(SubscriptionStorageError::UnsafePath);
    }
    Ok(())
}

fn create_secure_directory_chain(path: &Path) -> Result<(), SubscriptionStorageError> {
    let mut current = PathBuf::from("/");
    for component in path.components() {
        if let Component::Normal(component) = component {
            current.push(component);
            match fs::symlink_metadata(&current) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let mut builder = fs::DirBuilder::new();
                    builder.mode(0o700);
                    match builder.create(&current) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(error) => {
                            return Err(SubscriptionStorageError::Io(format!(
                                "create secure directory: {error}"
                            )))
                        }
                    }
                }
                Err(error) => {
                    return Err(SubscriptionStorageError::Io(format!(
                        "inspect directory chain: {error}"
                    )))
                }
            }
            require_root_directory(&current)?;
        }
    }
    Ok(())
}

fn require_root_directory(path: &Path) -> Result<(), SubscriptionStorageError> {
    let metadata = fs::symlink_metadata(path).map_err(storage_io("inspect directory"))?;
    if !metadata.file_type().is_dir() || metadata.uid() != 0 {
        return Err(SubscriptionStorageError::InsecureOwnership);
    }
    Ok(())
}

fn require_root_private_file(path: &Path) -> Result<(), SubscriptionStorageError> {
    let metadata = fs::symlink_metadata(path).map_err(storage_io("inspect private file"))?;
    if !metadata.file_type().is_file() || metadata.uid() != 0 || metadata.mode() & 0o077 != 0 {
        return Err(SubscriptionStorageError::InsecureOwnership);
    }
    Ok(())
}

fn read_optional_private(
    path: &Path,
    maximum: usize,
) -> Result<Option<Vec<u8>>, SubscriptionStorageError> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(SubscriptionStorageError::Io(format!("open state: {error}"))),
    };
    let metadata = file.metadata().map_err(storage_io("inspect open state"))?;
    if !metadata.file_type().is_file() || metadata.uid() != 0 || metadata.mode() & 0o077 != 0 {
        return Err(SubscriptionStorageError::InsecureOwnership);
    }
    if metadata.len() > maximum as u64 {
        return Err(SubscriptionStorageError::InvalidState(
            "stored value exceeds size limit",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(storage_io("read state"))?;
    if bytes.len() > maximum {
        return Err(SubscriptionStorageError::InvalidState(
            "stored value exceeds size limit",
        ));
    }
    Ok(Some(bytes))
}

fn atomic_write(
    directory: &Path,
    name: &str,
    write: impl FnOnce(&mut File) -> Result<(), SubscriptionStorageError>,
) -> Result<(), SubscriptionStorageError> {
    require_root_directory(directory)?;
    let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(".{name}.{}.{}.tmp", std::process::id(), sequence));
    let destination = directory.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW)
        .open(&temporary)
        .map_err(storage_io("create temporary state"))?;
    let result = write(&mut file)
        .and_then(|()| file.sync_all().map_err(storage_io("sync temporary state")))
        .and_then(|()| fs::rename(&temporary, &destination).map_err(storage_io("commit state")))
        .and_then(|()| require_root_private_file(&destination))
        .and_then(|()| sync_directory(directory));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn sync_directory(path: &Path) -> Result<(), SubscriptionStorageError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(storage_io("sync state directory"))
}

fn storage_io(context: &'static str) -> impl FnOnce(std::io::Error) -> SubscriptionStorageError {
    move |error| SubscriptionStorageError::Io(format!("{context}: {error}"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubscriptionStorageError {
    Io(String),
    UnsafePath,
    InsecureOwnership,
    InvalidState(&'static str),
    GenerationExists,
    GenerationNotFound,
    TransportUnavailable,
}

impl fmt::Display for SubscriptionStorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(reason) => write!(formatter, "subscription storage I/O error: {reason}"),
            Self::UnsafePath => formatter.write_str("subscription storage path is unsafe"),
            Self::InsecureOwnership => formatter
                .write_str("subscription state must be root-owned and must not be a symlink"),
            Self::InvalidState(reason) => write!(formatter, "invalid subscription state: {reason}"),
            Self::GenerationExists => formatter.write_str("subscription generation already exists"),
            Self::GenerationNotFound => {
                formatter.write_str("subscription generation does not exist")
            }
            Self::TransportUnavailable => {
                formatter.write_str("subscription transport is not configured")
            }
        }
    }
}

impl Error for SubscriptionStorageError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::subscription::parse_mihomo_subscription;
    use std::os::unix::fs::symlink;

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "hyz-router-subscription-{label}-{}-{}",
            std::process::id(),
            TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn transport_requests_mihomo_yaml_without_enabling_redirects_or_compression() {
        let transport = UreqSubscriptionTransport::new(Arc::new(SystemSubscriptionResolver));
        let config = transport.direct_agent.config();

        match config.user_agent() {
            ureq::config::AutoHeaderValue::Provided(value) => {
                assert_eq!(value.as_str(), SUBSCRIPTION_USER_AGENT);
            }
            other => panic!("unexpected subscription User-Agent configuration: {other:?}"),
        }
        match config.accept() {
            ureq::config::AutoHeaderValue::Provided(value) => {
                assert_eq!(value.as_str(), SUBSCRIPTION_ACCEPT);
            }
            other => panic!("unexpected subscription Accept configuration: {other:?}"),
        }
        assert_eq!(config.max_redirects(), 0);
        match config.accept_encoding() {
            ureq::config::AutoHeaderValue::Provided(value) => {
                assert_eq!(value.as_str(), SUBSCRIPTION_ACCEPT_ENCODING);
            }
            other => panic!("unexpected subscription Accept-Encoding configuration: {other:?}"),
        }
        let request = transport
            .direct_agent
            .get("https://example.invalid/")
            .header("Connection", SUBSCRIPTION_CONNECTION);
        assert_eq!(
            request
                .headers_ref()
                .and_then(|headers| headers.get("Connection"))
                .and_then(|value| value.to_str().ok()),
            Some(SUBSCRIPTION_CONNECTION)
        );
    }

    #[test]
    fn transport_has_fixed_mihomo_proxy_fallback() {
        let transport = UreqSubscriptionTransport::new(Arc::new(SystemSubscriptionResolver));
        let proxy = transport
            .proxy_agent
            .config()
            .proxy()
            .expect("subscription proxy fallback must be configured");
        assert_eq!(proxy.uri().to_string(), SUBSCRIPTION_PROXY_URL);
        assert!(!proxy.resolve_target());
    }

    #[test]
    fn failed_direct_subscription_fetch_retries_through_mihomo_proxy() {
        let mut routes = Vec::new();
        let result = fetch_subscription_with_fallback("https://example.invalid/", |route, _url| {
            routes.push(route);
            match route {
                SubscriptionRoute::Direct => {
                    Err(PlatformError::Io("direct request failed".to_owned()))
                }
                SubscriptionRoute::MihomoProxy => Ok(b"proxies:\n  - name: test\n".to_vec()),
            }
        });

        assert_eq!(result, Ok(b"proxies:\n  - name: test\n".to_vec()));
        assert_eq!(
            routes,
            vec![SubscriptionRoute::Direct, SubscriptionRoute::MihomoProxy]
        );
    }

    #[test]
    fn generation_is_immutable_and_current_pointer_is_regular() {
        let root = temporary_root("generation");
        let store = SubscriptionStore::new(&root);
        if store.initialize().is_err() {
            return; // Root-only storage is intentionally unavailable to unprivileged tests.
        }
        let generation = GenerationId::parse("g1").unwrap();
        let subscription = parse_mihomo_subscription(b"proxies:\n  - name: a\n").unwrap();
        store.stage_generation(&generation, &subscription).unwrap();
        store.activate_generation(&generation).unwrap();
        assert_eq!(
            store.current_generation().unwrap(),
            Some(generation.clone())
        );
        assert!(store.stage_generation(&generation, &subscription).is_err());
        assert!(fs::symlink_metadata(root.join(CURRENT_FILE))
            .unwrap()
            .file_type()
            .is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn symlink_state_is_rejected() {
        let root = temporary_root("symlink");
        let store = SubscriptionStore::new(&root);
        if store.initialize().is_err() {
            return;
        }
        symlink("/etc/passwd", root.join(URL_FILE)).unwrap();
        assert!(store.load_url().is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn traversal_root_is_rejected() {
        let store = SubscriptionStore::new("/tmp/../tmp/subscriptions");
        assert_eq!(
            store.initialize(),
            Err(SubscriptionStorageError::UnsafePath)
        );
    }
}
