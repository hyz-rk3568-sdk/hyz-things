use super::ota_cli::OtaCommand;
use crate::{
    application::{
        dhcp::DhcpEvent,
        wifi::{ApPrepareRequest, StaCandidateRequest, WifiScanEntry},
    },
    domain::{
        network_config::{NetworkConfigSummary, PendingNetworkConfigSummary},
        panel::{
            valid_control_name, DisplayRequest, PanelSnapshot, ProxyDelayRefreshRequest,
            ProxyDelayRequest, ProxyDelayResult, ProxyGroup, ProxySelectionRequest,
        },
        status::StatusSnapshot,
    },
};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{watch, Semaphore},
    task::JoinSet,
    time::timeout,
};

pub const CONTROL_SOCKET: &str = "/run/hyz-router/control.sock";
pub const CONTROL_RUNTIME_DIR: &str = "/run/hyz-router";
pub const DAEMON_LOCK_DIR: &str = "/run/hyz-router/daemon.lock";
const DAEMON_OWNER_FILE: &str = "/run/hyz-router/daemon.lock/owner";
pub const PROTOCOL_VERSION: u16 = 2;
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const MAX_CONNECTIONS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub version: u16,
    pub operation: ControlOperation,
}

impl ControlRequest {
    pub const fn new(operation: ControlOperation) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            operation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlOperation {
    Status {},
    PanelStatus {},
    Display { request: DisplayRequest },
    Router { enabled: bool },
    Proxy { mode: ControlProxyMode },
    ProxySelection { request: ProxySelectionRequest },
    ProxyDelay { request: ProxyDelayRequest },
    ProxyDelayRefresh { request: ProxyDelayRefreshRequest },
    Ota { command: OtaCommand },
    Dhcp { event: DhcpEvent },
    WifiStatus {},
    WifiScan {},
    WifiStaApply { request: StaCandidateRequest },
    WifiApPrepare { request: ApPrepareRequest },
    WifiApApply {},
    WifiApConfirm {},
    WifiApCancel {},
}

impl ControlOperation {
    pub const fn mutates(&self) -> bool {
        !matches!(
            self,
            Self::Status { .. }
                | Self::PanelStatus { .. }
                | Self::WifiStatus { .. }
                | Self::WifiScan { .. }
                | Self::Ota {
                    command: OtaCommand::Verify { .. }
                }
        )
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Status { .. }
            | Self::PanelStatus { .. }
            | Self::ProxyDelayRefresh { .. }
            | Self::Router { .. }
            | Self::Proxy { .. }
            | Self::WifiStatus { .. }
            | Self::WifiScan { .. }
            | Self::WifiStaApply { .. }
            | Self::WifiApPrepare { .. }
            | Self::WifiApApply { .. }
            | Self::WifiApConfirm { .. }
            | Self::WifiApCancel { .. } => Ok(()),
            Self::Display { request } => validate_display(request),
            Self::ProxySelection { request } => {
                if valid_control_name(&request.group) && valid_control_name(&request.proxy) {
                    Ok(())
                } else {
                    Err("proxy selection names are invalid")
                }
            }
            Self::ProxyDelay { request } => {
                if valid_control_name(&request.proxy) {
                    Ok(())
                } else {
                    Err("proxy delay name is invalid")
                }
            }
            Self::Ota { command } => validate_ota(command),
            Self::Dhcp { event } => validate_dhcp(event),
        }
    }
}

fn validate_display(request: &DisplayRequest) -> Result<(), &'static str> {
    match (request.enabled, request.brightness) {
        (true, None) | (true, Some(1..=u16::MAX)) | (false, None) => Ok(()),
        (true, Some(0)) => Err("enabled display brightness must be non-zero"),
        (false, Some(_)) => Err("disabled display request may not include brightness"),
    }
}

fn validate_ota(command: &OtaCommand) -> Result<(), &'static str> {
    match command {
        OtaCommand::Verify { firmware, expected }
        | OtaCommand::Install {
            firmware, expected, ..
        }
        | OtaCommand::InstallRecovery {
            firmware, expected, ..
        } => {
            validate_path(firmware)?;
            validate_digest(expected)
        }
        OtaCommand::Download { source, expected }
        | OtaCommand::Apply {
            source, expected, ..
        } => {
            validate_source(source)?;
            validate_digest(expected)
        }
    }
}

fn validate_path(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > 4096
        || !value.starts_with('/')
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        Err("firmware path must be an absolute bounded path without control characters")
    } else {
        Ok(())
    }
}

fn validate_source(value: &str) -> Result<(), &'static str> {
    let remainder = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"));
    let authority = remainder
        .and_then(|remainder| remainder.split(['/', '?', '#']).next())
        .unwrap_or_default();
    if value.len() > 2048
        || authority.is_empty()
        || authority.contains('@')
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        Err("firmware source must be a bounded HTTP(S) URL with a host and no userinfo or whitespace")
    } else {
        Ok(())
    }
}

fn validate_digest(value: &str) -> Result<(), &'static str> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("firmware digest must be exactly 64 hexadecimal characters")
    }
}

fn validate_dhcp(event: &DhcpEvent) -> Result<(), &'static str> {
    let DhcpEvent::Lease { lease } = event else {
        return Ok(());
    };
    if lease.prefix > 32
        || (lease.routers.is_empty() && lease.static_routes.is_empty())
        || lease.routers.len() > 16
        || lease.static_routes.len() > 64
        || lease.dns.len() > 16
        || lease.search.len() > 16
    {
        return Err("DHCP lease exceeds semantic count or prefix limits");
    }
    for (destination, _) in &lease.static_routes {
        if destination.len() > 64 || !valid_route_destination(destination) {
            return Err("DHCP static route destination is invalid");
        }
    }
    if lease.search.iter().any(|domain| {
        domain.is_empty()
            || domain.len() > 253
            || !domain
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    }) {
        return Err("DHCP search domain is invalid");
    }
    Ok(())
}

fn valid_route_destination(value: &str) -> bool {
    if value == "default" {
        return true;
    }
    let Some((address, prefix)) = value.split_once('/') else {
        return false;
    };
    address.parse::<std::net::Ipv4Addr>().is_ok()
        && prefix.parse::<u8>().ok().is_some_and(|prefix| prefix <= 32)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlProxyMode {
    Explicit,
    Tun,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlResponse {
    pub version: u16,
    pub result: Result<ControlResult, ControlError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlResult {
    Status {
        snapshot: Box<StatusSnapshot>,
    },
    PanelStatus {
        snapshot: Box<PanelSnapshot>,
    },
    ProxyDelay {
        result: ProxyDelayResult,
    },
    ProxyDelays {
        groups: Vec<ProxyGroup>,
    },
    WifiConfig {
        config: NetworkConfigSummary,
    },
    WifiPending {
        pending: PendingNetworkConfigSummary,
    },
    WifiScan {
        entries: Vec<WifiScanEntry>,
    },
    Completed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlError {
    pub code: String,
    pub message: String,
}

impl ControlResponse {
    pub fn success(result: ControlResult) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            result: Ok(result),
        }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            result: Err(ControlError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

#[async_trait]
pub trait ControlHandler: Send + Sync + 'static {
    async fn handle(&self, operation: ControlOperation) -> Result<ControlResult, String>;
}

pub struct DaemonOwnership {
    identity: String,
    directory_device: u64,
    directory_inode: u64,
    socket_identity: Option<(u64, u64)>,
}

pub fn acquire_daemon_ownership() -> io::Result<DaemonOwnership> {
    ensure_runtime_directory()?;
    match fs::DirBuilder::new().mode(0o700).create(DAEMON_LOCK_DIR) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "daemon ownership lock already exists; stale or malformed locks require explicit operator removal",
            ));
        }
        Err(error) => return Err(error),
    }
    let result: io::Result<DaemonOwnership> = (|| {
        fs::set_permissions(DAEMON_LOCK_DIR, fs::Permissions::from_mode(0o700))?;
        let directory = fs::symlink_metadata(DAEMON_LOCK_DIR)?;
        if directory.uid() != 0
            || directory.mode() & 0o777 != 0o700
            || !directory.file_type().is_dir()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "daemon lock must be a root-owned mode 0700 directory",
            ));
        }
        let identity = current_daemon_identity()?;
        let mut owner = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(DAEMON_OWNER_FILE)?;
        owner.write_all(identity.as_bytes())?;
        owner.sync_all()?;
        let owner_metadata = fs::symlink_metadata(DAEMON_OWNER_FILE)?;
        if owner_metadata.uid() != 0
            || owner_metadata.mode() & 0o777 != 0o600
            || !owner_metadata.file_type().is_file()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "daemon owner identity must be a root-owned mode 0600 file",
            ));
        }
        Ok(DaemonOwnership {
            identity,
            directory_device: directory.dev(),
            directory_inode: directory.ino(),
            socket_identity: None,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(DAEMON_OWNER_FILE);
        let _ = fs::remove_dir(DAEMON_LOCK_DIR);
    }
    result
}

impl DaemonOwnership {
    fn verify_owned(&self) -> io::Result<()> {
        if current_daemon_identity()? != self.identity {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "daemon process identity changed",
            ));
        }
        let directory = fs::symlink_metadata(DAEMON_LOCK_DIR)?;
        if directory.uid() != 0
            || directory.mode() & 0o777 != 0o700
            || !directory.file_type().is_dir()
            || directory.dev() != self.directory_device
            || directory.ino() != self.directory_inode
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "daemon lock directory identity changed",
            ));
        }
        let owner = fs::symlink_metadata(DAEMON_OWNER_FILE)?;
        if owner.uid() != 0
            || owner.mode() & 0o777 != 0o600
            || !owner.file_type().is_file()
            || owner.len() > 8192
            || fs::read_to_string(DAEMON_OWNER_FILE)? != self.identity
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "daemon owner identity changed",
            ));
        }
        Ok(())
    }

    pub fn release(self) -> io::Result<()> {
        self.verify_owned()?;
        if self.socket_identity.is_some() {
            match fs::symlink_metadata(CONTROL_SOCKET) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "refusing to release daemon ownership while the control socket exists",
                    ));
                }
                Err(error) => return Err(error),
            }
        }
        fs::remove_file(DAEMON_OWNER_FILE)?;
        fs::remove_dir(DAEMON_LOCK_DIR)
    }
}

fn current_daemon_identity() -> io::Result<String> {
    let pid = std::process::id();
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let end = stat.rfind(')').ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon process stat is malformed",
        )
    })?;
    let start_time = stat[end + 1..]
        .split_whitespace()
        .nth(19)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "daemon start time is malformed")
        })?;
    let executable = fs::read_link(format!("/proc/{pid}/exe"))?;
    let executable = executable.to_str().filter(|value| {
        !value.is_empty() && !value.bytes().any(|byte| matches!(byte, 0 | b'\n' | b'\r'))
    });
    let executable = executable.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "daemon executable identity is invalid",
        )
    })?;
    Ok(format!(
        "hyz-daemon-v1\n{pid}\n{start_time}\n{executable}\n"
    ))
}

pub fn bind_control_socket(ownership: &mut DaemonOwnership) -> io::Result<UnixListener> {
    ownership.verify_owned()?;
    let path = Path::new(CONTROL_SOCKET);
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            let description = if metadata.uid() == 0 && metadata.file_type().is_socket() {
                "control socket already exists; refusing automatic stale or active takeover"
            } else {
                "control path exists with an unsafe identity"
            };
            return Err(io::Error::new(io::ErrorKind::AddrInUse, description));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(path)?;
    let secure = (|| {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        let metadata = fs::symlink_metadata(path)?;
        if metadata.uid() != 0
            || metadata.mode() & 0o777 != 0o600
            || !metadata.file_type().is_socket()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "control socket must be root-owned mode 0600",
            ));
        }
        Ok((metadata.dev(), metadata.ino()))
    })();
    let socket_identity = match secure {
        Ok(identity) => identity,
        Err(error) => {
            drop(listener);
            let _ = fs::remove_file(path);
            return Err(error);
        }
    };
    ownership.socket_identity = Some(socket_identity);
    Ok(listener)
}

pub fn remove_control_socket(ownership: &DaemonOwnership) -> io::Result<()> {
    ownership.verify_owned()?;
    let expected = ownership.socket_identity.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "control socket was not owned",
        )
    })?;
    let metadata = fs::symlink_metadata(CONTROL_SOCKET)?;
    if metadata.uid() != 0
        || metadata.mode() & 0o777 != 0o600
        || !metadata.file_type().is_socket()
        || (metadata.dev(), metadata.ino()) != expected
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to remove a changed control socket identity",
        ));
    }
    fs::remove_file(CONTROL_SOCKET)
}

fn ensure_runtime_directory() -> io::Result<()> {
    match fs::symlink_metadata(CONTROL_RUNTIME_DIR) {
        Ok(metadata) if !metadata.file_type().is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "control runtime path is not a directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(CONTROL_RUNTIME_DIR)?;
        }
        Err(error) => return Err(error),
    }
    fs::set_permissions(CONTROL_RUNTIME_DIR, fs::Permissions::from_mode(0o700))?;
    let metadata = fs::symlink_metadata(CONTROL_RUNTIME_DIR)?;
    if metadata.uid() != 0 || metadata.mode() & 0o777 != 0o700 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "control runtime directory must be root-owned mode 0700",
        ));
    }
    Ok(())
}

pub async fn serve_control(
    listener: UnixListener,
    handler: std::sync::Arc<dyn ControlHandler>,
    mut shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let concurrency = std::sync::Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut connections = JoinSet::new();
    let mut accept_error = None;
    loop {
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let (stream, _) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => {
                        accept_error = Some(error);
                        break;
                    }
                };
                let permit = match concurrency.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => continue,
                };
                let handler = handler.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let _ = handle_connection(stream, handler).await;
                });
            }
            _ = connections.join_next(), if !connections.is_empty() => {}
        }
    }
    drop(listener);
    while connections.join_next().await.is_some() {}
    match accept_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

async fn handle_connection(
    mut stream: UnixStream,
    handler: std::sync::Arc<dyn ControlHandler>,
) -> io::Result<()> {
    let credentials = stream.peer_cred()?;
    if credentials.uid() != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "control peer is not root",
        ));
    }
    let request: ControlRequest = timeout(IO_TIMEOUT, read_frame(&mut stream))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "control request timed out"))??;
    let response = if request.version != PROTOCOL_VERSION {
        ControlResponse::error(
            "unsupported_version",
            "unsupported control protocol version",
        )
    } else if let Err(message) = request.operation.validate() {
        ControlResponse::error("invalid_request", message)
    } else {
        // The connection has a bounded response deadline, but a timed-out operation is still
        // awaited. This preserves mutation locks and lets graceful shutdown drain every handler.
        let operation = request.operation;
        let operation_handler = handler.clone();
        let mut task = tokio::spawn(async move { operation_handler.handle(operation).await });
        match timeout(OPERATION_TIMEOUT, &mut task).await {
            Ok(Ok(Ok(result))) => ControlResponse::success(result),
            Ok(Ok(Err(message))) => ControlResponse::error("operation_failed", message),
            Ok(Err(_)) => ControlResponse::error(
                "operation_failed",
                "control operation task terminated unexpectedly",
            ),
            Err(_) => {
                let _ = task.await;
                ControlResponse::error("operation_timeout", "control operation timed out")
            }
        }
    };
    timeout(IO_TIMEOUT, write_frame(&mut stream, &response))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "control response timed out"))??;
    Ok(())
}

pub async fn request(operation: ControlOperation) -> io::Result<ControlResult> {
    operation
        .validate()
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    if operation.mutates() {
        require_root("control mutation")?;
    }
    let mut stream = timeout(IO_TIMEOUT, UnixStream::connect(CONTROL_SOCKET))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "control connect timed out"))??;
    timeout(
        IO_TIMEOUT,
        write_frame(&mut stream, &ControlRequest::new(operation)),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "control request timed out"))??;
    let response: ControlResponse =
        timeout(OPERATION_TIMEOUT + IO_TIMEOUT, read_frame(&mut stream))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "control response timed out"))??;
    if response.version != PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported control response version",
        ));
    }
    response
        .result
        .map_err(|error| io::Error::other(format!("{}: {}", error.code, error.message)))
}

pub fn require_root(role: &str) -> io::Result<()> {
    if effective_uid()? == 0 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("{role} requires effective UID 0"),
        ))
    }
}

fn effective_uid() -> io::Result<u32> {
    let status = fs::read_to_string("/proc/self/status")?;
    let line = status
        .lines()
        .find(|line| line.starts_with("Uid:"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "process UID is unavailable"))?;
    line.split_whitespace()
        .nth(2)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "effective UID is malformed"))
}

async fn read_frame<T: DeserializeOwned>(stream: &mut UnixStream) -> io::Result<T> {
    let length = stream.read_u32().await? as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control frame length is outside bounds",
        ));
    }
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).await?;
    serde_json::from_slice(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

async fn write_frame<T: Serialize>(stream: &mut UnixStream, value: &T) -> io::Result<()> {
    let payload = serde_json::to_vec(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control frame exceeds size limit",
        ));
    }
    stream.write_u32(payload.len() as u32).await?;
    stream.write_all(&payload).await?;
    stream.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_rejects_unknown_fields_and_is_bounded_by_version() {
        let unknown = br#"{"version":1,"operation":{"op":"status","extra":true}}"#;
        assert!(serde_json::from_slice::<ControlRequest>(unknown).is_err());
        let request = ControlRequest::new(ControlOperation::Status {});
        let encoded = serde_json::to_vec(&request).unwrap();
        assert!(encoded.len() < MAX_FRAME_BYTES);
        assert_eq!(
            serde_json::from_slice::<ControlRequest>(&encoded).unwrap(),
            request
        );
    }

    #[test]
    fn only_verify_is_a_read_only_ota_operation() {
        assert!(!ControlOperation::Status {}.mutates());
        assert!(!ControlOperation::Ota {
            command: OtaCommand::Verify {
                firmware: "/tmp/x".to_owned(),
                expected: "0".repeat(64),
            }
        }
        .mutates());
        assert!(ControlOperation::Router { enabled: true }.mutates());
    }

    #[test]
    fn semantic_validation_bounds_ota_and_dhcp_payloads() {
        assert!(ControlOperation::Ota {
            command: OtaCommand::Verify {
                firmware: "relative.fw".to_owned(),
                expected: "0".repeat(64),
            }
        }
        .validate()
        .is_err());
        assert!(ControlOperation::Ota {
            command: OtaCommand::Download {
                source: "file:///tmp/update.fw".to_owned(),
                expected: "not-a-digest".to_owned(),
            }
        }
        .validate()
        .is_err());

        let lease = crate::application::dhcp::DhcpLease {
            address: "192.0.2.10".parse().unwrap(),
            prefix: 24,
            broadcast: None,
            routers: vec!["192.0.2.1".parse().unwrap(); 17],
            static_routes: Vec::new(),
            dns: Vec::new(),
            search: Vec::new(),
        };
        assert!(ControlOperation::Dhcp {
            event: DhcpEvent::Lease { lease }
        }
        .validate()
        .is_err());
    }

    #[test]
    fn singleton_and_shutdown_guards_remain_in_the_control_source() {
        let source = include_str!("control.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        assert!(production.contains("stale or malformed locks require explicit operator removal"));
        assert!(production.contains("while connections.join_next().await.is_some()"));
        assert!(production.contains("current_daemon_identity()? != self.identity"));
    }
}
