use super::{
    paths::{
        TAILSCALED_EXECUTABLE, TAILSCALE_DATA_DIR, TAILSCALE_FIREWALL_OWNER, TAILSCALE_LOG,
        TAILSCALE_MODE_FILE, TAILSCALE_PID_RECORD, TAILSCALE_RUNTIME_DIR, TAILSCALE_SOCKET,
        TAILSCALE_STATE_FILE, TAILSCALE_SUBNET_FIREWALL_OWNER,
    },
    process::{
        count_executable_processes, process_identity_matches, process_start_time,
        process_stat_identity, reap_in_background, wait_for_identity_match, LinuxRouterPlatform,
        Tool, NETWORK_LOCK,
    },
    storage,
    system::{
        chain_output_is_exact, exact_chain_references, forward_hook_order_is_exact,
        normalized_chain_rules, owned_forward_hook_is_exact,
    },
};
use crate::{
    application::ports::{
        LifecycleLease, PlatformError, TailscalePlatformPort, TailscaleProbePort,
    },
    domain::{
        network::{
            OwnedResource, Probe, LAN_BRIDGE, LAN_SUBNET, ROUTER_FILTER_CHAIN, WAN_INTERFACE,
        },
        proxy::MIHOMO_FILTER_CHAIN,
        tailscale::{
            TailscaleAction, TailscaleBackendState, TailscaleConnectionKind, TailscaleLoginUrl,
            TailscaleMode, TailscaleObserved, TailscalePreferences, TailscaleProcessState,
            TAILSCALE_CGNAT_SUBNET, TAILSCALE_FORWARD_CHAIN, TAILSCALE_INPUT_CHAIN,
            TAILSCALE_INTERFACE, TAILSCALE_LAN_ROUTE, TAILSCALE_MANAGEMENT_HTTP_PORT,
            TAILSCALE_NAT_CHAIN, TAILSCALE_UDP_PORT,
        },
    },
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io,
    net::Ipv4Addr,
    os::unix::{
        ffi::OsStrExt,
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_IDENTITY_RECORD: usize = 8 * 1024;
const MAX_PREFS_JSON: usize = 64 * 1024;
const MAX_STATUS_JSON: usize = 256 * 1024;
const START_IDENTITY_ATTEMPTS: usize = 60;
const START_IDENTITY_POLL: Duration = Duration::from_millis(50);
const BACKEND_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const BACKEND_WAIT_POLL: Duration = Duration::from_millis(50);
const LOGIN_URL_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const LOGIN_URL_WAIT_POLL: Duration = Duration::from_millis(50);
const EXIT_ATTEMPTS: usize = 60;
const EXIT_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Default, Clone)]
pub struct LinuxTailscalePlatform {
    platform: LinuxRouterPlatform,
}

impl LinuxTailscalePlatform {
    pub const fn new() -> Self {
        Self {
            platform: LinuxRouterPlatform::new(),
        }
    }

    fn apply(&self, action: &TailscaleAction) -> Result<(), PlatformError> {
        match action {
            TailscaleAction::StartBackend { token } => self.start_backend(token),
            TailscaleAction::WaitForBackend => self.wait_for_backend(),
            TailscaleAction::StopBackend { token } => self.stop_backend(token),
            TailscaleAction::SetFixedPreferences => {
                self.set_preferences(TailscalePreferences::FIXED, None)
            }
            TailscaleAction::RestorePreferences { preferences } => {
                self.set_preferences(*preferences, None)
            }
            TailscaleAction::AdvertiseLanRoute => {
                self.set_preferences(TailscalePreferences::FIXED, Some(true))
            }
            TailscaleAction::ClearAdvertisedRoute => {
                self.set_preferences(TailscalePreferences::FIXED, Some(false))
            }
            TailscaleAction::InstallRouterFirewall { token } => self.install_router_firewall(token),
            TailscaleAction::RemoveRouterFirewall { token } => self.remove_router_firewall(token),
            TailscaleAction::InstallSubnetFirewall { token } => self.install_subnet_firewall(token),
            TailscaleAction::RemoveSubnetFirewall { token } => self.remove_subnet_firewall(token),
            TailscaleAction::CommitDesiredMode { mode } => self.write_mode(Some(*mode)),
            TailscaleAction::RestoreDesiredMode { mode } => self.write_mode(*mode),
            TailscaleAction::StartManagementListener { .. }
            | TailscaleAction::StopManagementListener { .. } => Err(PlatformError::InvalidState(
                "composition must intercept Tailscale management-listener actions".to_owned(),
            )),
        }
    }

    fn tailscale(&self, args: &[&str]) -> Result<super::process::FixedOutput, PlatformError> {
        let mut fixed = vec!["--socket".to_owned(), TAILSCALE_SOCKET.to_owned()];
        fixed.extend(args.iter().map(|value| (*value).to_owned()));
        self.platform.run(Tool::Tailscale, &fixed)
    }

    fn tailscale_probe(&self, args: &[&str]) -> Result<super::process::FixedOutput, PlatformError> {
        let mut fixed = vec!["--socket".to_owned(), TAILSCALE_SOCKET.to_owned()];
        fixed.extend(args.iter().map(|value| (*value).to_owned()));
        self.platform.run_probe(Tool::Tailscale, &fixed)
    }

    fn tailscale_until(
        &self,
        args: &[&str],
        deadline: Instant,
    ) -> Result<super::process::FixedOutput, PlatformError> {
        let timeout = remaining_command_timeout(deadline).ok_or_else(|| {
            PlatformError::UnsafeToCutOver(
                "Tailscale operation reached its absolute deadline".to_owned(),
            )
        })?;
        let mut fixed = vec!["--socket".to_owned(), TAILSCALE_SOCKET.to_owned()];
        fixed.extend(args.iter().map(|value| (*value).to_owned()));
        self.platform
            .run_with_timeout(Tool::Tailscale, &fixed, timeout)
    }

    fn tailscale_probe_until(
        &self,
        args: &[&str],
        deadline: Instant,
    ) -> Result<super::process::FixedOutput, PlatformError> {
        let timeout = remaining_command_timeout(deadline).ok_or_else(|| {
            PlatformError::UnsafeToCutOver(
                "Tailscale probe reached its absolute deadline".to_owned(),
            )
        })?;
        let mut fixed = vec!["--socket".to_owned(), TAILSCALE_SOCKET.to_owned()];
        fixed.extend(args.iter().map(|value| (*value).to_owned()));
        self.platform
            .run_probe_with_timeout(Tool::Tailscale, &fixed, timeout)
    }

    fn iptables(&self, args: &[&str]) -> Result<(), PlatformError> {
        self.platform
            .run(Tool::Iptables, &strings(args))
            .map(|_| ())
    }

    fn start_backend(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        if storage::read_private_small_optional(TAILSCALE_PID_RECORD, MAX_IDENTITY_RECORD)?
            .is_some()
        {
            return Err(PlatformError::Conflict(
                "Tailscale PID identity record already exists".to_owned(),
            ));
        }
        if count_executable_processes(Path::new(TAILSCALED_EXECUTABLE))? != 0 {
            return Err(PlatformError::Conflict(
                "tailscaled is already running without an owned identity record".to_owned(),
            ));
        }
        if fs::symlink_metadata(TAILSCALE_SOCKET).is_ok()
            || fs::symlink_metadata(format!("/sys/class/net/{TAILSCALE_INTERFACE}")).is_ok()
        {
            return Err(PlatformError::Conflict(
                "stale or foreign Tailscale socket/interface blocks startup".to_owned(),
            ));
        }
        storage::ensure_private_dir(TAILSCALE_DATA_DIR)?;
        storage::ensure_private_dir(TAILSCALE_RUNTIME_DIR)?;
        storage::require_private_root_file_optional(TAILSCALE_STATE_FILE)?;
        let log = private_log(TAILSCALE_LOG)?;
        let stderr = log
            .try_clone()
            .map_err(|error| PlatformError::Io(format!("clone tailscaled log: {error}")))?;
        let argv = tailscaled_argv();
        let mut child = Command::new(TAILSCALED_EXECUTABLE)
            .args(
                argv.iter()
                    .skip(1)
                    .map(|arg| std::ffi::OsStr::from_bytes(arg)),
            )
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr))
            .spawn()
            .map_err(|error| PlatformError::Io(format!("start fixed tailscaled: {error}")))?;
        let pid = child.id();
        let start_time = process_start_time(pid)?.ok_or_else(|| {
            PlatformError::InvalidState("new tailscaled process disappeared".to_owned())
        })?;
        let identity = TailscaledIdentity {
            pid,
            start_time,
            executable: PathBuf::from(TAILSCALED_EXECUTABLE),
            argv,
            token: token.to_owned(),
            socket: None,
            interface_ifindex: None,
        };
        if let Err(error) =
            storage::atomic_write_private(TAILSCALE_PID_RECORD, identity.serialize().as_bytes())
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let identity_match =
            wait_for_identity_match(START_IDENTITY_ATTEMPTS, START_IDENTITY_POLL, || {
                identity.matches_live_process()
            });
        match identity_match {
            Ok(true) => {}
            Ok(false) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = storage::remove_file_durable(TAILSCALE_PID_RECORD);
                return Err(PlatformError::InvalidState(
                    "new tailscaled identity did not match exact PID/start/exe/argv".to_owned(),
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = storage::remove_file_durable(TAILSCALE_PID_RECORD);
                return Err(error);
            }
        }
        reap_in_background(child, "tailscaled");
        Ok(())
    }

    fn wait_for_backend(&self) -> Result<(), PlatformError> {
        let deadline = Instant::now() + BACKEND_WAIT_TIMEOUT;
        loop {
            let identity = self.exact_identity()?.ok_or_else(|| {
                PlatformError::InvalidState(
                    "tailscaled exited before its local API became ready".to_owned(),
                )
            })?;
            if socket_owned_by_root(TAILSCALE_SOCKET)? {
                let status = self.tailscale_probe_until(&["status", "--json"], deadline)?;
                if status.success {
                    let parsed = parse_status_json(&status.stdout)?;
                    if backend_is_stable(parsed.backend) && identity.matches_live_process()? {
                        storage::require_private_root_file(TAILSCALE_STATE_FILE)?;
                        self.record_runtime_nodes(&identity)?;
                        return Ok(());
                    }
                }
            }
            if !sleep_until(deadline, BACKEND_WAIT_POLL) {
                return Err(PlatformError::UnsafeToCutOver(
                    "tailscaled local API did not become ready before the fixed deadline"
                        .to_owned(),
                ));
            }
        }
    }

    fn record_runtime_nodes(&self, identity: &TailscaledIdentity) -> Result<(), PlatformError> {
        let socket = socket_identity(TAILSCALE_SOCKET)?.ok_or_else(|| {
            PlatformError::UnsafeToCutOver(
                "tailscaled socket disappeared during ownership capture".to_owned(),
            )
        })?;
        let interface_index = interface_ifindex()?.ok_or_else(|| {
            PlatformError::UnsafeToCutOver(
                "tailscale0 disappeared during ownership capture".to_owned(),
            )
        })?;
        if !identity.matches_live_process()? || self.read_identity()?.as_ref() != Some(identity) {
            return Err(PlatformError::Conflict(
                "tailscaled identity changed during runtime ownership capture".to_owned(),
            ));
        }
        let mut bound = identity.clone();
        bound.socket = Some(socket);
        bound.interface_ifindex = Some(interface_index);
        storage::atomic_write_private(TAILSCALE_PID_RECORD, bound.serialize().as_bytes())?;
        if !bound.matches_live_process()?
            || socket_identity(TAILSCALE_SOCKET)? != Some(socket)
            || interface_ifindex()? != Some(interface_index)
        {
            return Err(PlatformError::UnsafeToCutOver(
                "tailscaled runtime ownership changed after identity capture".to_owned(),
            ));
        }
        Ok(())
    }

    fn stop_backend(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        let identity = self.read_identity()?.ok_or_else(|| {
            PlatformError::Conflict("Tailscale PID identity record is absent".to_owned())
        })?;
        if identity.token != token {
            return Err(PlatformError::Conflict(
                "refusing to stop tailscaled with a mismatched ownership token".to_owned(),
            ));
        }
        match identity.process_state()? {
            IdentityProcessState::Live => {
                self.platform
                    .run(Tool::Kill, &["-TERM".to_owned(), identity.pid.to_string()])?;
                if !wait_for_identity_exit(EXIT_ATTEMPTS, EXIT_POLL, || identity.process_state())? {
                    match identity.process_state()? {
                        IdentityProcessState::Live => {
                            self.platform
                                .run(Tool::Kill, &["-KILL".to_owned(), identity.pid.to_string()])?;
                        }
                        IdentityProcessState::Exited => {}
                        IdentityProcessState::Replaced => {
                            return Err(PlatformError::Conflict(
                                "refusing to signal a replaced tailscaled process identity"
                                    .to_owned(),
                            ))
                        }
                    }
                    if !wait_for_identity_exit(EXIT_ATTEMPTS, EXIT_POLL, || {
                        identity.process_state()
                    })? {
                        return Err(PlatformError::Conflict(
                            "exact tailscaled identity remained live after TERM and KILL"
                                .to_owned(),
                        ));
                    }
                }
            }
            IdentityProcessState::Exited => {}
            IdentityProcessState::Replaced => {
                return Err(PlatformError::Conflict(
                    "refusing to clean a replaced tailscaled process identity".to_owned(),
                ))
            }
        }
        if identity.process_state()? != IdentityProcessState::Exited
            || count_executable_processes(Path::new(TAILSCALED_EXECUTABLE))? != 0
            || self.read_identity()?.as_ref() != Some(&identity)
        {
            return Err(PlatformError::Conflict(
                "tailscaled identity changed before owned runtime cleanup".to_owned(),
            ));
        }
        let socket = socket_identity(TAILSCALE_SOCKET)?;
        let interface = interface_ifindex()?;
        if !recorded_node_removal_allowed(identity.socket, socket)
            || !recorded_node_removal_allowed(identity.interface_ifindex, interface)
        {
            return Err(PlatformError::Conflict(
                "refusing owned cleanup while a Tailscale runtime node is mismatched".to_owned(),
            ));
        }
        remove_recorded_socket(TAILSCALE_SOCKET, identity.socket)?;
        remove_recorded_interface(&self.platform, identity.interface_ifindex)?;
        if self.read_identity()?.as_ref() != Some(&identity) {
            return Err(PlatformError::Conflict(
                "tailscaled identity record changed before removal".to_owned(),
            ));
        }
        storage::remove_file_durable(TAILSCALE_PID_RECORD)
    }

    fn set_preferences(
        &self,
        preferences: TailscalePreferences,
        route: Option<bool>,
    ) -> Result<(), PlatformError> {
        let accept_dns = bool_arg(preferences.accept_dns);
        let accept_routes = bool_arg(preferences.accept_routes);
        let advertise_exit_node = bool_arg(preferences.advertise_exit_node);
        let ssh = bool_arg(preferences.ssh_enabled);
        let netfilter = if preferences.netfilter_off {
            "off"
        } else {
            "on"
        };
        let route = match route {
            Some(true) => TAILSCALE_LAN_ROUTE,
            Some(false) | None => "",
        };
        self.tailscale(&[
            "set",
            &format!("--accept-dns={accept_dns}"),
            &format!("--accept-routes={accept_routes}"),
            &format!("--advertise-exit-node={advertise_exit_node}"),
            "--exit-node=",
            &format!("--ssh={ssh}"),
            &format!("--netfilter-mode={netfilter}"),
            &format!("--advertise-routes={route}"),
        ])?;
        let (observed_preferences, observed_route) = self.observe_preferences()?;
        if observed_preferences != preferences || observed_route != (route == TAILSCALE_LAN_ROUTE) {
            return Err(PlatformError::UnsafeToCutOver(
                "tailscale did not confirm the fixed preferences and route advertisement"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    fn request_login_url(&self) -> Result<TailscaleLoginUrl, PlatformError> {
        let status = self.tailscale(&["status", "--json"])?;
        if let Some(url) = parse_status_login_url(&status.stdout)? {
            return Ok(url);
        }
        self.tailscale_probe(&login_args())?;
        let deadline = Instant::now() + LOGIN_URL_WAIT_TIMEOUT;
        wait_for_login_url_until(deadline, LOGIN_URL_WAIT_POLL, || {
            self.tailscale_until(&["status", "--json"], deadline)
                .map(|status| status.stdout)
        })
    }

    fn logout_backend(&self) -> Result<(), PlatformError> {
        self.tailscale(&["logout"])?;
        let status = self.tailscale(&["status", "--json"])?;
        let parsed = parse_status_json(&status.stdout)?;
        if parsed.backend != TailscaleBackendState::NeedsLogin
            || parsed.authenticated
            || parsed.ipv4.is_some()
        {
            return Err(PlatformError::UnsafeToCutOver(
                "tailscale logout did not confirm NeedsLogin without an IPv4".to_owned(),
            ));
        }
        Ok(())
    }

    fn write_mode(&self, mode: Option<TailscaleMode>) -> Result<(), PlatformError> {
        match mode {
            Some(mode) => storage::atomic_write_private(
                TAILSCALE_MODE_FILE,
                format!("{}\n", mode_text(mode)).as_bytes(),
            ),
            None => storage::remove_file_durable(TAILSCALE_MODE_FILE),
        }
    }

    fn read_mode(&self) -> Probe<Option<TailscaleMode>> {
        match storage::read_private_small_optional(TAILSCALE_MODE_FILE, 64) {
            Ok(None) => Probe::Known(None),
            Ok(Some(value)) => match value.trim() {
                "disabled" => Probe::Known(Some(TailscaleMode::Disabled)),
                "router_only" => Probe::Known(Some(TailscaleMode::RouterOnly)),
                "lan_subnet_access" => Probe::Known(Some(TailscaleMode::LanSubnetAccess)),
                _ => Probe::Unknown("persisted Tailscale mode is invalid".to_owned()),
            },
            Err(error) => Probe::Unknown(error.to_string()),
        }
    }

    fn read_identity(&self) -> Result<Option<TailscaledIdentity>, PlatformError> {
        let Some(record) =
            storage::read_private_small_optional(TAILSCALE_PID_RECORD, MAX_IDENTITY_RECORD)?
        else {
            return Ok(None);
        };
        TailscaledIdentity::parse(&record).map(Some)
    }

    fn exact_identity(&self) -> Result<Option<TailscaledIdentity>, PlatformError> {
        let Some(identity) = self.read_identity()? else {
            return Ok(None);
        };
        Ok(identity.matches_live_process()?.then_some(identity))
    }

    fn observe_process(&self) -> Probe<TailscaleProcessState> {
        match (
            self.read_identity(),
            count_executable_processes(Path::new(TAILSCALED_EXECUTABLE)),
        ) {
            (Ok(None), Ok(0)) => Probe::Known(TailscaleProcessState::Absent),
            (Ok(None), Ok(_)) => Probe::Known(TailscaleProcessState::Foreign),
            (Ok(Some(identity)), Ok(count)) => match identity.process_state() {
                Ok(IdentityProcessState::Live) if count == 1 => {
                    Probe::Known(TailscaleProcessState::OwnedLive {
                        token: identity.token,
                    })
                }
                Ok(IdentityProcessState::Exited) if count == 0 => {
                    Probe::Known(TailscaleProcessState::OwnedExited {
                        token: identity.token,
                    })
                }
                Ok(IdentityProcessState::Live | IdentityProcessState::Exited)
                | Ok(IdentityProcessState::Replaced) => {
                    Probe::Known(TailscaleProcessState::Foreign)
                }
                Err(error) => Probe::Unknown(error.to_string()),
            },
            (Err(error), _) | (_, Err(error)) => Probe::Unknown(error.to_string()),
        }
    }

    fn observe_socket(&self, process: &Probe<TailscaleProcessState>) -> Probe<OwnedResource> {
        let metadata = match fs::symlink_metadata(TAILSCALE_SOCKET) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Probe::Unknown(format!("inspect Tailscale socket: {error}")),
        };
        let Some(metadata) = metadata else {
            return Probe::Known(OwnedResource::Absent);
        };
        let expected = match self.read_identity() {
            Ok(Some(identity)) => identity,
            Ok(None) => return Probe::Known(OwnedResource::Foreign),
            Err(error) => return Probe::Unknown(error.to_string()),
        };
        match process {
            Probe::Known(
                TailscaleProcessState::OwnedLive { token }
                | TailscaleProcessState::OwnedExited { token },
            ) if *token == expected.token
                && expected.socket == Some(RuntimeNodeIdentity::from_metadata(&metadata))
                && metadata.file_type().is_socket()
                && metadata.uid() == 0 =>
            {
                Probe::Known(OwnedResource::Owned {
                    token: token.clone(),
                })
            }
            Probe::Unknown(reason) => Probe::Unknown(reason.clone()),
            _ => Probe::Known(OwnedResource::Foreign),
        }
    }

    fn observe_interface(&self, process: &Probe<TailscaleProcessState>) -> Probe<OwnedResource> {
        let actual = match interface_ifindex() {
            Ok(actual) => actual,
            Err(error) => return Probe::Unknown(error.to_string()),
        };
        let Some(actual) = actual else {
            return Probe::Known(OwnedResource::Absent);
        };
        let expected = match self.read_identity() {
            Ok(Some(identity)) => identity,
            Ok(None) => return Probe::Known(OwnedResource::Foreign),
            Err(error) => return Probe::Unknown(error.to_string()),
        };
        match process {
            Probe::Known(
                TailscaleProcessState::OwnedLive { token }
                | TailscaleProcessState::OwnedExited { token },
            ) if *token == expected.token && expected.interface_ifindex == Some(actual) => {
                Probe::Known(OwnedResource::Owned {
                    token: token.clone(),
                })
            }
            Probe::Unknown(reason) => Probe::Unknown(reason.clone()),
            _ => Probe::Known(OwnedResource::Foreign),
        }
    }

    fn observe_preferences(&self) -> Result<(TailscalePreferences, bool), PlatformError> {
        let output = self.tailscale(&["debug", "prefs"])?;
        if output.stdout.len() > MAX_PREFS_JSON {
            return Err(PlatformError::ProbeFailed(
                "Tailscale prefs JSON exceeds limit".to_owned(),
            ));
        }
        parse_prefs_json(&output.stdout)
    }

    fn install_router_firewall(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        if storage::read_private_small_optional(TAILSCALE_FIREWALL_OWNER, 128)?.is_some() {
            return Err(PlatformError::Conflict(
                "Tailscale firewall owner marker already exists".to_owned(),
            ));
        }
        let forward = self
            .platform
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", "FORWARD"]),
            )?
            .stdout;
        if !exact_chain_references(&forward, TAILSCALE_FORWARD_CHAIN)
            .is_some_and(|references| references.is_empty())
        {
            return Err(PlatformError::Conflict(
                "Tailscale FORWARD references already exist or are unparseable".to_owned(),
            ));
        }
        let mihomo =
            owner_hook_is_exact(&forward, storage::TUN_FIREWALL_OWNER, MIHOMO_FILTER_CHAIN)?;
        let router = owner_hook_is_exact(
            &forward,
            storage::ROUTER_FIREWALL_OWNER,
            ROUTER_FILTER_CHAIN,
        )?;
        if !forward_hook_order_is_exact(&forward, mihomo, false, router) {
            return Err(PlatformError::Conflict(
                "existing FORWARD hooks are not in Mihomo, router order".to_owned(),
            ));
        }

        self.iptables(&["-w", "-t", "filter", "-N", TAILSCALE_INPUT_CHAIN])?;
        let mut input_hook_created = false;
        let mut forward_hook_created = false;
        let result = (|| {
            self.iptables(&["-w", "-t", "filter", "-N", TAILSCALE_FORWARD_CHAIN])?;
            for rule in tailscale_input_rules(token) {
                append_rule(&self.platform, "filter", TAILSCALE_INPUT_CHAIN, &rule)?;
            }
            for rule in tailscale_forward_rules(token, None) {
                append_rule(&self.platform, "filter", TAILSCALE_FORWARD_CHAIN, &rule)?;
            }
            self.iptables(&[
                "-w",
                "-t",
                "filter",
                "-I",
                "INPUT",
                "1",
                "-m",
                "comment",
                "--comment",
                token,
                "-j",
                TAILSCALE_INPUT_CHAIN,
            ])?;
            input_hook_created = true;
            let position = 1 + usize::from(mihomo);
            self.iptables(&[
                "-w",
                "-t",
                "filter",
                "-I",
                "FORWARD",
                &position.to_string(),
                "-m",
                "comment",
                "--comment",
                token,
                "-j",
                TAILSCALE_FORWARD_CHAIN,
            ])?;
            forward_hook_created = true;
            storage::atomic_write_private(TAILSCALE_FIREWALL_OWNER, format!("{token}\n").as_bytes())
        })();
        if result.is_err() {
            if forward_hook_created {
                let _ = delete_hook(
                    &self.platform,
                    "filter",
                    "FORWARD",
                    token,
                    TAILSCALE_FORWARD_CHAIN,
                );
            }
            if input_hook_created {
                let _ = delete_hook(
                    &self.platform,
                    "filter",
                    "INPUT",
                    token,
                    TAILSCALE_INPUT_CHAIN,
                );
            }
            rollback_chain(
                &self.platform,
                "filter",
                TAILSCALE_FORWARD_CHAIN,
                &tailscale_forward_rules(token, None),
            );
            rollback_chain(
                &self.platform,
                "filter",
                TAILSCALE_INPUT_CHAIN,
                &tailscale_input_rules(token),
            );
        }
        result
    }

    fn remove_router_firewall(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        require_marker(TAILSCALE_FIREWALL_OWNER, token)?;
        if storage::read_private_small_optional(TAILSCALE_SUBNET_FIREWALL_OWNER, 128)?.is_some() {
            return Err(PlatformError::Conflict(
                "subnet firewall must be removed before the RouterOnly firewall".to_owned(),
            ));
        }
        self.verify_router_firewall(token, None)?;
        delete_hook(
            &self.platform,
            "filter",
            "INPUT",
            token,
            TAILSCALE_INPUT_CHAIN,
        )?;
        delete_hook(
            &self.platform,
            "filter",
            "FORWARD",
            token,
            TAILSCALE_FORWARD_CHAIN,
        )?;
        remove_exact_chain(
            &self.platform,
            "filter",
            TAILSCALE_INPUT_CHAIN,
            &tailscale_input_rules(token),
        )?;
        remove_exact_chain(
            &self.platform,
            "filter",
            TAILSCALE_FORWARD_CHAIN,
            &tailscale_forward_rules(token, None),
        )?;
        storage::remove_file_durable(TAILSCALE_FIREWALL_OWNER)
    }

    fn install_subnet_firewall(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        if storage::read_private_small_optional(TAILSCALE_SUBNET_FIREWALL_OWNER, 128)?.is_some() {
            return Err(PlatformError::Conflict(
                "Tailscale subnet firewall marker already exists".to_owned(),
            ));
        }
        let router_token = marker_token(TAILSCALE_FIREWALL_OWNER)?;
        self.verify_router_firewall(&router_token, None)?;
        let ordinary_router_token = marker_token(storage::ROUTER_FIREWALL_OWNER).map_err(|_| {
            PlatformError::UnsafeToCutOver(
                "ordinary router firewall must be owned before Tailscale subnet NAT".to_owned(),
            )
        })?;
        let postrouting = self
            .platform
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "nat", "-S", "POSTROUTING"]),
            )?
            .stdout;
        let router_hook = words(&[
            "-A",
            "POSTROUTING",
            "-m",
            "comment",
            "--comment",
            &ordinary_router_token,
            "-j",
            crate::domain::network::ROUTER_NAT_CHAIN,
        ]);
        let postrouting_rules =
            normalized_chain_rules(&postrouting, "POSTROUTING").ok_or_else(|| {
                PlatformError::ProbeFailed(
                    "cannot parse POSTROUTING before Tailscale NAT".to_owned(),
                )
            })?;
        if postrouting_rules.first() != Some(&router_hook)
            || exact_chain_references(&postrouting, crate::domain::network::ROUTER_NAT_CHAIN)
                != Some(vec![router_hook])
            || !exact_chain_references(&postrouting, TAILSCALE_NAT_CHAIN)
                .is_some_and(|references| references.is_empty())
        {
            return Err(PlatformError::Conflict(
                "ordinary router NAT is not exact and first before Tailscale insertion".to_owned(),
            ));
        }
        let base = tailscale_forward_rules(&router_token, None);
        let expanded = tailscale_forward_rules(&router_token, Some(token));
        replace_chain_body(
            &self.platform,
            "filter",
            TAILSCALE_FORWARD_CHAIN,
            &base,
            &expanded,
        )?;
        let result = (|| {
            self.iptables(&["-w", "-t", "nat", "-N", TAILSCALE_NAT_CHAIN])?;
            for rule in tailscale_nat_rules(token) {
                append_rule(&self.platform, "nat", TAILSCALE_NAT_CHAIN, &rule)?;
            }
            self.iptables(&[
                "-w",
                "-t",
                "nat",
                "-I",
                "POSTROUTING",
                "2",
                "-m",
                "comment",
                "--comment",
                token,
                "-j",
                TAILSCALE_NAT_CHAIN,
            ])?;
            storage::atomic_write_private(
                TAILSCALE_SUBNET_FIREWALL_OWNER,
                format!("{token}\n").as_bytes(),
            )
        })();
        if result.is_err() {
            let _ = delete_hook(
                &self.platform,
                "nat",
                "POSTROUTING",
                token,
                TAILSCALE_NAT_CHAIN,
            );
            rollback_chain(
                &self.platform,
                "nat",
                TAILSCALE_NAT_CHAIN,
                &tailscale_nat_rules(token),
            );
            let _ = replace_chain_body(
                &self.platform,
                "filter",
                TAILSCALE_FORWARD_CHAIN,
                &expanded,
                &base,
            );
        }
        result
    }

    fn remove_subnet_firewall(&self, token: &str) -> Result<(), PlatformError> {
        storage::validate_token(token)?;
        require_marker(TAILSCALE_SUBNET_FIREWALL_OWNER, token)?;
        let router_token = marker_token(TAILSCALE_FIREWALL_OWNER)?;
        self.verify_router_firewall(&router_token, Some(token))?;
        verify_chain(
            &self.platform,
            "nat",
            TAILSCALE_NAT_CHAIN,
            &tailscale_nat_rules(token),
        )?;
        verify_exact_hook(
            &self.platform,
            "nat",
            "POSTROUTING",
            token,
            TAILSCALE_NAT_CHAIN,
            Some(1),
        )?;
        delete_hook(
            &self.platform,
            "nat",
            "POSTROUTING",
            token,
            TAILSCALE_NAT_CHAIN,
        )?;
        remove_exact_chain(
            &self.platform,
            "nat",
            TAILSCALE_NAT_CHAIN,
            &tailscale_nat_rules(token),
        )?;
        let expanded = tailscale_forward_rules(&router_token, Some(token));
        let base = tailscale_forward_rules(&router_token, None);
        replace_chain_body(
            &self.platform,
            "filter",
            TAILSCALE_FORWARD_CHAIN,
            &expanded,
            &base,
        )?;
        storage::remove_file_durable(TAILSCALE_SUBNET_FIREWALL_OWNER)
    }

    fn verify_router_firewall(
        &self,
        token: &str,
        subnet_token: Option<&str>,
    ) -> Result<(), PlatformError> {
        verify_chain(
            &self.platform,
            "filter",
            TAILSCALE_INPUT_CHAIN,
            &tailscale_input_rules(token),
        )?;
        verify_chain(
            &self.platform,
            "filter",
            TAILSCALE_FORWARD_CHAIN,
            &tailscale_forward_rules(token, subnet_token),
        )?;
        verify_exact_hook(
            &self.platform,
            "filter",
            "INPUT",
            token,
            TAILSCALE_INPUT_CHAIN,
            Some(0),
        )?;
        verify_exact_hook(
            &self.platform,
            "filter",
            "FORWARD",
            token,
            TAILSCALE_FORWARD_CHAIN,
            None,
        )?;
        let forward = self
            .platform
            .run(
                Tool::Iptables,
                &strings(&["-w", "-t", "filter", "-S", "FORWARD"]),
            )?
            .stdout;
        let mihomo = owned_forward_hook_is_exact(
            &forward,
            storage::TUN_FIREWALL_OWNER,
            MIHOMO_FILTER_CHAIN,
        )?;
        let router = owned_forward_hook_is_exact(
            &forward,
            storage::ROUTER_FIREWALL_OWNER,
            ROUTER_FILTER_CHAIN,
        )?;
        if !forward_hook_order_is_exact(&forward, mihomo, true, router) {
            return Err(PlatformError::Conflict(
                "FORWARD hooks are not in exact Mihomo, Tailscale, router order".to_owned(),
            ));
        }
        Ok(())
    }

    fn observe_firewalls(&self) -> (Probe<OwnedResource>, Probe<OwnedResource>) {
        let router_marker =
            match storage::read_private_small_optional(TAILSCALE_FIREWALL_OWNER, 128) {
                Ok(marker) => marker,
                Err(error) => {
                    let unknown = Probe::Unknown(error.to_string());
                    return (unknown.clone(), unknown);
                }
            };
        let subnet_marker =
            match storage::read_private_small_optional(TAILSCALE_SUBNET_FIREWALL_OWNER, 128) {
                Ok(marker) => marker,
                Err(error) => {
                    let unknown = Probe::Unknown(error.to_string());
                    return (unknown.clone(), unknown);
                }
            };
        match (router_marker, subnet_marker) {
            (None, None) => {
                if chains_absent(&self.platform) {
                    (
                        Probe::Known(OwnedResource::Absent),
                        Probe::Known(OwnedResource::Absent),
                    )
                } else {
                    (
                        Probe::Known(OwnedResource::Foreign),
                        Probe::Known(OwnedResource::Foreign),
                    )
                }
            }
            (Some(router), subnet) => {
                let router = router.trim();
                if storage::validate_token(router).is_err() {
                    return (
                        Probe::Known(OwnedResource::Foreign),
                        Probe::Known(OwnedResource::Foreign),
                    );
                }
                let subnet_token = subnet.as_deref().map(str::trim);
                if subnet_token.is_some_and(|token| storage::validate_token(token).is_err()) {
                    return (
                        Probe::Known(OwnedResource::Foreign),
                        Probe::Known(OwnedResource::Foreign),
                    );
                }
                if self.verify_router_firewall(router, subnet_token).is_err() {
                    return (
                        Probe::Known(OwnedResource::Foreign),
                        Probe::Known(OwnedResource::Foreign),
                    );
                }
                let router_probe = Probe::Known(OwnedResource::Owned {
                    token: router.to_owned(),
                });
                match subnet_token {
                    None => (router_probe, Probe::Known(OwnedResource::Absent)),
                    Some(token)
                        if verify_chain(
                            &self.platform,
                            "nat",
                            TAILSCALE_NAT_CHAIN,
                            &tailscale_nat_rules(token),
                        )
                        .and_then(|_| {
                            verify_exact_hook(
                                &self.platform,
                                "nat",
                                "POSTROUTING",
                                token,
                                TAILSCALE_NAT_CHAIN,
                                Some(1),
                            )
                        })
                        .is_ok() =>
                    {
                        (
                            router_probe,
                            Probe::Known(OwnedResource::Owned {
                                token: token.to_owned(),
                            }),
                        )
                    }
                    Some(_) => (router_probe, Probe::Known(OwnedResource::Foreign)),
                }
            }
            (None, Some(_)) => (
                Probe::Known(OwnedResource::Foreign),
                Probe::Known(OwnedResource::Foreign),
            ),
        }
    }
}

impl TailscalePlatformPort for LinuxTailscalePlatform {
    fn acquire_tailscale_lock(&self) -> Result<LifecycleLease, PlatformError> {
        storage::acquire_lock(NETWORK_LOCK)
    }

    fn release_tailscale_lock(&self, lease: &LifecycleLease) -> Result<(), PlatformError> {
        storage::release_lock(lease)
    }

    fn apply_tailscale(&self, action: &TailscaleAction) -> Result<(), PlatformError> {
        self.apply(action)
    }

    fn request_login(&self) -> Result<TailscaleLoginUrl, PlatformError> {
        self.request_login_url()
    }

    fn logout(&self) -> Result<(), PlatformError> {
        self.logout_backend()
    }
}

impl TailscaleProbePort for LinuxTailscalePlatform {
    fn observe_tailscale(&self) -> Result<TailscaleObserved, PlatformError> {
        let process = self.observe_process();
        if matches!(
            &process,
            Probe::Known(TailscaleProcessState::OwnedLive { .. })
        ) {
            if let Some(identity) = self.read_identity()? {
                if identity.socket.is_none() || identity.interface_ifindex.is_none() {
                    self.record_runtime_nodes(&identity)?;
                }
            }
        }
        let socket = self.observe_socket(&process);
        let interface = self.observe_interface(&process);
        let (backend_state, authenticated, ipv4, preferences, route_advertised, connection) =
            match &process {
                Probe::Known(
                    TailscaleProcessState::Absent | TailscaleProcessState::OwnedExited { .. },
                ) => (
                    Probe::Known(TailscaleBackendState::Stopped),
                    Probe::Known(false),
                    Probe::Known(None),
                    Probe::Known(TailscalePreferences::FIXED),
                    Probe::Known(false),
                    Probe::Known(TailscaleConnectionKind::Unknown),
                ),
                Probe::Known(TailscaleProcessState::OwnedLive { .. }) => {
                    let status = self.tailscale(&["status", "--json"]);
                    let prefs = self.observe_preferences();
                    match (status, prefs) {
                        (Ok(status), Ok((prefs, route))) => {
                            if status.stdout.len() > MAX_STATUS_JSON {
                                let reason = "Tailscale status JSON exceeds limit".to_owned();
                                (
                                    Probe::Unknown(reason.clone()),
                                    Probe::Unknown(reason.clone()),
                                    Probe::Unknown(reason.clone()),
                                    Probe::Unknown(reason.clone()),
                                    Probe::Unknown(reason.clone()),
                                    Probe::Unknown(reason),
                                )
                            } else {
                                match parse_status_json(&status.stdout).and_then(|parsed| {
                                    if parsed.backend == TailscaleBackendState::Running {
                                        let ip = self.tailscale(&["ip", "-4"])?;
                                        let cli_ip = parse_ip_output(&ip.stdout)?;
                                        if parsed.ipv4 != cli_ip {
                                            return Err(PlatformError::ProbeFailed(
                                                "status and tailscale ip disagree".to_owned(),
                                            ));
                                        }
                                    }
                                    Ok(parsed)
                                }) {
                                    Ok(parsed) => (
                                        Probe::Known(parsed.backend),
                                        Probe::Known(parsed.authenticated),
                                        Probe::Known(parsed.ipv4),
                                        Probe::Known(prefs),
                                        Probe::Known(route),
                                        Probe::Known(parsed.connection),
                                    ),
                                    Err(error) => unknown_runtime(error.to_string()),
                                }
                            }
                        }
                        (Err(error), _) | (_, Err(error)) => unknown_runtime(error.to_string()),
                    }
                }
                Probe::Known(TailscaleProcessState::Foreign) => unknown_runtime(
                    "foreign tailscaled process prevents local API observation".to_owned(),
                ),
                Probe::Unknown(reason) => unknown_runtime(reason.clone()),
            };
        let (router_firewall, subnet_firewall) = self.observe_firewalls();
        Ok(TailscaleObserved {
            persisted_mode: self.read_mode(),
            backend_state,
            process,
            socket,
            interface,
            authenticated,
            ipv4,
            preferences,
            route_advertised,
            router_firewall,
            subnet_firewall,
            management_listener: Probe::Known(OwnedResource::Absent),
            management_listener_ipv4: Probe::Known(None),
            connection,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuntimeNodeIdentity {
    dev: u64,
    ino: u64,
}

impl RuntimeNodeIdentity {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentityProcessState {
    Live,
    Exited,
    Replaced,
}

fn classify_identity_process(
    expected_start: u64,
    observed: Option<(u64, u8)>,
    exact_match: bool,
) -> IdentityProcessState {
    match observed {
        None => IdentityProcessState::Exited,
        Some((start, state)) if start == expected_start && (state == b'Z' || state == b'X') => {
            IdentityProcessState::Exited
        }
        Some((start, _)) if start == expected_start && exact_match => IdentityProcessState::Live,
        Some(_) => IdentityProcessState::Replaced,
    }
}

fn wait_for_identity_exit<F>(
    attempts: usize,
    poll: Duration,
    mut process_state: F,
) -> Result<bool, PlatformError>
where
    F: FnMut() -> Result<IdentityProcessState, PlatformError>,
{
    for attempt in 0..attempts {
        match process_state()? {
            IdentityProcessState::Exited => return Ok(true),
            IdentityProcessState::Replaced => {
                return Err(PlatformError::Conflict(
                    "tailscaled process identity was replaced while waiting for exit".to_owned(),
                ))
            }
            IdentityProcessState::Live => {}
        }
        if attempt + 1 < attempts {
            thread::sleep(poll);
        }
    }
    Ok(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TailscaledIdentity {
    pid: u32,
    start_time: u64,
    executable: PathBuf,
    argv: Vec<Vec<u8>>,
    token: String,
    socket: Option<RuntimeNodeIdentity>,
    interface_ifindex: Option<u32>,
}

impl TailscaledIdentity {
    fn serialize(&self) -> String {
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
            self.pid,
            self.start_time,
            self.executable.display(),
            self.token,
            encode_argv(&self.argv),
            self.socket
                .map(|identity| format!("{}:{}", identity.dev, identity.ino))
                .unwrap_or_else(|| "-".to_owned()),
            self.interface_ifindex
                .map(|ifindex| ifindex.to_string())
                .unwrap_or_else(|| "-".to_owned())
        )
    }

    fn parse(record: &str) -> Result<Self, PlatformError> {
        let mut lines = record.lines();
        let pid = parse_nonzero(lines.next(), "tailscaled PID")?;
        let start_time = parse_nonzero(lines.next(), "tailscaled start time")?;
        let executable = PathBuf::from(lines.next().ok_or_else(|| {
            PlatformError::InvalidState("tailscaled executable record is absent".to_owned())
        })?);
        let token = lines
            .next()
            .ok_or_else(|| {
                PlatformError::InvalidState("tailscaled ownership token is absent".to_owned())
            })?
            .to_owned();
        storage::validate_token(&token)?;
        let argv = decode_argv(lines.next().ok_or_else(|| {
            PlatformError::InvalidState("tailscaled argv record is absent".to_owned())
        })?)?;
        let (socket, interface_ifindex) = match (lines.next(), lines.next()) {
            (None, None) => (None, None),
            (Some(socket), Some(ifindex)) => (
                parse_runtime_node_identity(Some(socket))?,
                parse_optional_nonzero_u32(Some(ifindex), "tailscale0 ifindex")?,
            ),
            _ => {
                return Err(PlatformError::InvalidState(
                    "tailscaled runtime identity record is partial".to_owned(),
                ))
            }
        };
        if lines.next().is_some()
            || executable != Path::new(TAILSCALED_EXECUTABLE)
            || argv != tailscaled_argv()
        {
            return Err(PlatformError::InvalidState(
                "tailscaled identity record has unexpected executable/argv/fields".to_owned(),
            ));
        }
        Ok(Self {
            pid: u32::try_from(pid).map_err(|_| {
                PlatformError::InvalidState("tailscaled PID record is out of range".to_owned())
            })?,
            start_time,
            executable,
            argv,
            token,
            socket,
            interface_ifindex,
        })
    }

    fn process_state(&self) -> Result<IdentityProcessState, PlatformError> {
        let observed = process_stat_identity(self.pid)?;
        match classify_identity_process(self.start_time, observed, false) {
            IdentityProcessState::Exited => return Ok(IdentityProcessState::Exited),
            IdentityProcessState::Replaced => {
                if !matches!(observed, Some((start, _)) if start == self.start_time) {
                    return Ok(IdentityProcessState::Replaced);
                }
            }
            IdentityProcessState::Live => unreachable!("exact match was not checked"),
        }
        if self.matches_live_process()? {
            return Ok(IdentityProcessState::Live);
        }
        Ok(classify_identity_process(
            self.start_time,
            process_stat_identity(self.pid)?,
            false,
        ))
    }

    fn matches_live_process(&self) -> Result<bool, PlatformError> {
        process_identity_matches(
            crate::application::ports::CoreIdentity {
                pid: self.pid,
                start_time: self.start_time,
            },
            &self.executable,
            &self.argv,
        )
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StatusFixture {
    backend_state: String,
    #[serde(rename = "TailscaleIPs")]
    tailscale_ips: Value,
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedStatus {
    backend: TailscaleBackendState,
    authenticated: bool,
    ipv4: Option<Ipv4Addr>,
    connection: TailscaleConnectionKind,
}

fn backend_is_stable(backend: TailscaleBackendState) -> bool {
    matches!(
        backend,
        TailscaleBackendState::NeedsLogin | TailscaleBackendState::Running
    )
}

fn parse_status_json(input: &str) -> Result<ParsedStatus, PlatformError> {
    if input.len() > MAX_STATUS_JSON {
        return Err(PlatformError::ProbeFailed(
            "Tailscale status JSON exceeds limit".to_owned(),
        ));
    }
    let fixture: StatusFixture = serde_json::from_str(input).map_err(|_| {
        PlatformError::ProbeFailed(
            "Tailscale status JSON lacks the narrow required shape".to_owned(),
        )
    })?;
    let backend = match fixture.backend_state.as_str() {
        "Stopped" => TailscaleBackendState::Stopped,
        "NeedsLogin" => TailscaleBackendState::NeedsLogin,
        "Running" => TailscaleBackendState::Running,
        _ => TailscaleBackendState::Unknown,
    };
    let address_values = match &fixture.tailscale_ips {
        Value::Null => &[][..],
        Value::Array(values) => values.as_slice(),
        _ => {
            return Err(PlatformError::ProbeFailed(
                "Tailscale status TailscaleIPs is neither null nor an array".to_owned(),
            ))
        }
    };
    let addresses = address_values
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| {
                    PlatformError::ProbeFailed(
                        "Tailscale status contains a non-string IP address".to_owned(),
                    )
                })?
                .parse::<std::net::IpAddr>()
                .map_err(|_| {
                    PlatformError::ProbeFailed(
                        "Tailscale status contains a malformed IP address".to_owned(),
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let ipv4s = addresses
        .into_iter()
        .filter_map(|address| match address {
            std::net::IpAddr::V4(address) => Some(address),
            std::net::IpAddr::V6(_) => None,
        })
        .collect::<Vec<_>>();
    if ipv4s.len() > 1
        || ipv4s
            .first()
            .is_some_and(|address| !is_tailscale_ipv4(*address))
    {
        return Err(PlatformError::ProbeFailed(
            "Tailscale status contains ambiguous or non-CGNAT IPv4 addresses".to_owned(),
        ));
    }
    let ipv4 = ipv4s.first().copied();
    let authenticated = backend == TailscaleBackendState::Running;
    if (authenticated && ipv4.is_none())
        || (backend == TailscaleBackendState::NeedsLogin && ipv4.is_some())
    {
        return Err(PlatformError::ProbeFailed(
            "Tailscale backend/authentication/IP fields are inconsistent".to_owned(),
        ));
    }
    let connection = TailscaleConnectionKind::Unknown;
    Ok(ParsedStatus {
        backend,
        authenticated,
        ipv4,
        connection,
    })
}

fn parse_ip_output(input: &str) -> Result<Option<Ipv4Addr>, PlatformError> {
    let lines = input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return Ok(None);
    }
    if lines.len() != 1 {
        return Err(PlatformError::ProbeFailed(
            "tailscale ip returned multiple IPv4 addresses".to_owned(),
        ));
    }
    let address = lines[0].parse::<Ipv4Addr>().map_err(|_| {
        PlatformError::ProbeFailed("tailscale ip returned malformed IPv4".to_owned())
    })?;
    if !is_tailscale_ipv4(address) {
        return Err(PlatformError::ProbeFailed(
            "tailscale ip returned an address outside 100.64.0.0/10".to_owned(),
        ));
    }
    Ok(Some(address))
}

fn parse_prefs_json(input: &str) -> Result<(TailscalePreferences, bool), PlatformError> {
    if input.len() > MAX_PREFS_JSON {
        return Err(PlatformError::ProbeFailed(
            "Tailscale prefs JSON exceeds limit".to_owned(),
        ));
    }
    let value: Value = serde_json::from_str(input)
        .map_err(|_| PlatformError::ProbeFailed("Tailscale prefs JSON is invalid".to_owned()))?;
    let object = value.as_object().ok_or_else(|| {
        PlatformError::ProbeFailed("Tailscale prefs JSON is not an object".to_owned())
    })?;
    let boolean = |name: &str| {
        object.get(name).and_then(Value::as_bool).ok_or_else(|| {
            PlatformError::ProbeFailed(format!("Tailscale prefs field {name} is absent or invalid"))
        })
    };
    let text = |name: &str| {
        object.get(name).and_then(Value::as_str).ok_or_else(|| {
            PlatformError::ProbeFailed(format!("Tailscale prefs field {name} is absent or invalid"))
        })
    };
    let routes = match object.get("AdvertiseRoutes") {
        Some(Value::Null) => &[][..],
        Some(Value::Array(routes)) => routes.as_slice(),
        _ => {
            return Err(PlatformError::ProbeFailed(
                "Tailscale prefs AdvertiseRoutes is absent or invalid".to_owned(),
            ))
        }
    };
    let routes = routes
        .iter()
        .map(|route| {
            route.as_str().ok_or_else(|| {
                PlatformError::ProbeFailed(
                    "Tailscale prefs AdvertiseRoutes contains a non-string".to_owned(),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let route_advertised = match routes.as_slice() {
        [] => false,
        [route] if *route == TAILSCALE_LAN_ROUTE => true,
        _ => {
            return Err(PlatformError::ProbeFailed(
                "Tailscale advertises an unexpected or ambiguous route set".to_owned(),
            ))
        }
    };
    let netfilter_off = match object.get("NetfilterMode") {
        Some(Value::Number(number)) if number.as_u64() == Some(0) => true,
        Some(Value::String(value)) if value.eq_ignore_ascii_case("off") => true,
        Some(Value::Number(_)) | Some(Value::String(_)) => false,
        _ => {
            return Err(PlatformError::ProbeFailed(
                "Tailscale prefs NetfilterMode is absent or invalid".to_owned(),
            ))
        }
    };
    let control_url = text("ControlURL")?;
    if !matches!(control_url, "" | "https://controlplane.tailscale.com") {
        return Err(PlatformError::ProbeFailed(
            "Tailscale prefs use a non-default control plane".to_owned(),
        ));
    }
    let exit_node_selected = !text("ExitNodeID")?.is_empty() || !text("ExitNodeIP")?.is_empty();
    let advertise_exit_node = routes
        .iter()
        .any(|route| matches!(*route, "0.0.0.0/0" | "::/0"));
    Ok((
        TailscalePreferences {
            accept_dns: boolean("CorpDNS")?,
            accept_routes: boolean("RouteAll")?,
            advertise_exit_node,
            exit_node_selected,
            ssh_enabled: boolean("RunSSH")?,
            netfilter_off,
        },
        route_advertised,
    ))
}

fn parse_status_login_url(input: &str) -> Result<Option<TailscaleLoginUrl>, PlatformError> {
    let parsed = parse_status_json(input)?;
    if parsed.backend != TailscaleBackendState::NeedsLogin
        || parsed.authenticated
        || parsed.ipv4.is_some()
    {
        return Err(PlatformError::ProbeFailed(
            "Tailscale status is not a safe NeedsLogin state".to_owned(),
        ));
    }
    let value: Value = serde_json::from_str(input)
        .map_err(|_| PlatformError::ProbeFailed("Tailscale status JSON is invalid".to_owned()))?;
    let url = value
        .as_object()
        .and_then(|object| object.get("AuthURL"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            PlatformError::ProbeFailed(
                "Tailscale NeedsLogin status lacks one AuthURL string".to_owned(),
            )
        })?;
    if url.is_empty() {
        return Ok(None);
    }
    TailscaleLoginUrl::new(url.to_owned())
        .map(Some)
        .ok_or_else(|| {
            PlatformError::ProbeFailed(
                "Tailscale NeedsLogin status contains a nonofficial AuthURL".to_owned(),
            )
        })
}

fn remaining_command_timeout(deadline: Instant) -> Option<Duration> {
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .min(Duration::from_secs(3));
    (!remaining.is_zero()).then_some(remaining)
}

fn sleep_until(deadline: Instant, poll: Duration) -> bool {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return false;
    }
    thread::sleep(poll.min(remaining));
    Instant::now() < deadline
}

fn wait_for_login_url_until(
    deadline: Instant,
    poll: Duration,
    mut read_status: impl FnMut() -> Result<String, PlatformError>,
) -> Result<TailscaleLoginUrl, PlatformError> {
    loop {
        if let Some(url) = parse_status_login_url(&read_status()?)? {
            return Ok(url);
        }
        if !sleep_until(deadline, poll) {
            return Err(PlatformError::UnsafeToCutOver(
                "Tailscale did not publish an official login URL before the fixed deadline"
                    .to_owned(),
            ));
        }
    }
}

#[cfg(test)]
fn wait_for_login_url(
    attempts: usize,
    poll: Duration,
    mut read_status: impl FnMut() -> Result<String, PlatformError>,
) -> Result<TailscaleLoginUrl, PlatformError> {
    for attempt in 0..attempts {
        if let Some(url) = parse_status_login_url(&read_status()?)? {
            return Ok(url);
        }
        if attempt + 1 < attempts && !poll.is_zero() {
            thread::sleep(poll);
        }
    }
    Err(PlatformError::UnsafeToCutOver(
        "Tailscale did not publish an official login URL before the fixed deadline".to_owned(),
    ))
}

fn login_args() -> [&'static str; 9] {
    [
        "login",
        "--timeout=2s",
        "--accept-dns=false",
        "--accept-routes=false",
        "--advertise-exit-node=false",
        "--exit-node=",
        "--ssh=false",
        "--netfilter-mode=off",
        "--advertise-routes=",
    ]
}

fn tailscaled_argv() -> Vec<Vec<u8>> {
    vec![
        TAILSCALED_EXECUTABLE.to_owned(),
        format!("--state={TAILSCALE_STATE_FILE}"),
        format!("--socket={TAILSCALE_SOCKET}"),
        format!("--tun={TAILSCALE_INTERFACE}"),
        format!("--port={TAILSCALE_UDP_PORT}"),
    ]
    .into_iter()
    .map(String::into_bytes)
    .collect()
}

fn tailscale_input_rules(token: &str) -> Vec<Vec<String>> {
    vec![
        words(&["-m", "comment", "--comment", token]),
        words(&[
            "-s",
            TAILSCALE_CGNAT_SUBNET,
            "!",
            "-i",
            TAILSCALE_INTERFACE,
            "-j",
            "DROP",
        ]),
        words(&[
            "-i",
            WAN_INTERFACE,
            "-p",
            "udp",
            "-m",
            "udp",
            "--dport",
            &TAILSCALE_UDP_PORT.to_string(),
            "-j",
            "ACCEPT",
        ]),
        words(&[
            "-i",
            TAILSCALE_INTERFACE,
            "-p",
            "tcp",
            "-m",
            "tcp",
            "--dport",
            &TAILSCALE_MANAGEMENT_HTTP_PORT.to_string(),
            "-j",
            "ACCEPT",
        ]),
        words(&["-i", TAILSCALE_INTERFACE, "-j", "DROP"]),
        words(&["-j", "RETURN"]),
    ]
}

fn tailscale_forward_rules(router_token: &str, subnet_token: Option<&str>) -> Vec<Vec<String>> {
    let mut rules = vec![
        words(&["-m", "comment", "--comment", router_token]),
        words(&[
            "-s",
            TAILSCALE_CGNAT_SUBNET,
            "!",
            "-i",
            TAILSCALE_INTERFACE,
            "-j",
            "DROP",
        ]),
    ];
    if let Some(token) = subnet_token {
        rules.push(words(&["-m", "comment", "--comment", token]));
        rules.push(words(&[
            "-d",
            LAN_SUBNET,
            "-i",
            TAILSCALE_INTERFACE,
            "-o",
            LAN_BRIDGE,
            "-m",
            "conntrack",
            "--ctstate",
            "NEW,RELATED,ESTABLISHED",
            "-j",
            "ACCEPT",
        ]));
        rules.push(words(&[
            "-s",
            LAN_SUBNET,
            "-d",
            TAILSCALE_CGNAT_SUBNET,
            "-i",
            LAN_BRIDGE,
            "-o",
            TAILSCALE_INTERFACE,
            "-m",
            "conntrack",
            "--ctstate",
            "RELATED,ESTABLISHED",
            "-j",
            "ACCEPT",
        ]));
        rules.push(words(&[
            "-s",
            LAN_SUBNET,
            "-d",
            TAILSCALE_CGNAT_SUBNET,
            "-i",
            LAN_BRIDGE,
            "-o",
            TAILSCALE_INTERFACE,
            "-m",
            "conntrack",
            "--ctstate",
            "NEW",
            "-j",
            "DROP",
        ]));
    }
    rules.extend([
        words(&["-i", TAILSCALE_INTERFACE, "-j", "DROP"]),
        words(&["-o", TAILSCALE_INTERFACE, "-j", "DROP"]),
        words(&["-j", "RETURN"]),
    ]);
    rules
}

fn tailscale_nat_rules(token: &str) -> Vec<Vec<String>> {
    vec![
        words(&["-m", "comment", "--comment", token]),
        words(&[
            "-s",
            TAILSCALE_CGNAT_SUBNET,
            "-d",
            TAILSCALE_LAN_ROUTE,
            "-o",
            LAN_BRIDGE,
            "-j",
            "MASQUERADE",
        ]),
    ]
}

fn owner_hook_is_exact(output: &str, marker: &str, chain: &str) -> Result<bool, PlatformError> {
    let marker = storage::read_private_small_optional(marker, 128)?;
    let references = exact_chain_references(output, chain)
        .ok_or_else(|| PlatformError::ProbeFailed(format!("cannot parse {chain} references")))?;
    match marker {
        Some(token) => {
            let token = token.trim();
            storage::validate_token(token)?;
            let expected = words(&[
                "-A",
                "FORWARD",
                "-m",
                "comment",
                "--comment",
                token,
                "-j",
                chain,
            ]);
            if references != [expected] {
                return Err(PlatformError::Conflict(format!(
                    "owned {chain} hook is not exact and unique"
                )));
            }
            Ok(true)
        }
        None if references.is_empty() => Ok(false),
        None => Err(PlatformError::Conflict(format!(
            "unowned {chain} references are present"
        ))),
    }
}

fn verify_chain(
    platform: &LinuxRouterPlatform,
    table: &str,
    chain: &str,
    expected: &[Vec<String>],
) -> Result<(), PlatformError> {
    let output = platform
        .run(Tool::Iptables, &strings(&["-w", "-t", table, "-S", chain]))?
        .stdout;
    let qualified = expected
        .iter()
        .map(|rule| {
            let mut qualified = words(&["-A", chain]);
            qualified.extend(rule.iter().cloned());
            qualified
        })
        .collect::<Vec<_>>();
    if !chain_output_is_exact(&output, chain, &qualified) {
        return Err(PlatformError::Conflict(format!(
            "live {table}/{chain} body is not the exact owned rule set"
        )));
    }
    Ok(())
}

fn verify_exact_hook(
    platform: &LinuxRouterPlatform,
    table: &str,
    parent: &str,
    token: &str,
    chain: &str,
    expected_position: Option<usize>,
) -> Result<(), PlatformError> {
    let output = platform
        .run(Tool::Iptables, &strings(&["-w", "-t", table, "-S"]))?
        .stdout;
    let expected = words(&[
        "-A",
        parent,
        "-m",
        "comment",
        "--comment",
        token,
        "-j",
        chain,
    ]);
    if exact_chain_references(&output, chain) != Some(vec![expected.clone()]) {
        return Err(PlatformError::Conflict(format!(
            "{table}/{parent} hook for {chain} is not exact and unique"
        )));
    }
    if let Some(position) = expected_position {
        let rules = normalized_chain_rules(&output, parent)
            .ok_or_else(|| PlatformError::ProbeFailed(format!("cannot parse {table}/{parent}")))?;
        if rules.get(position) != Some(&expected) {
            return Err(PlatformError::Conflict(format!(
                "{table}/{parent} hook for {chain} is out of order"
            )));
        }
    }
    Ok(())
}

fn append_rule(
    platform: &LinuxRouterPlatform,
    table: &str,
    chain: &str,
    rule: &[String],
) -> Result<(), PlatformError> {
    let mut args = strings(&["-w", "-t", table, "-A", chain]);
    args.extend(rule.iter().cloned());
    platform.run(Tool::Iptables, &args).map(|_| ())
}

fn delete_rule(
    platform: &LinuxRouterPlatform,
    table: &str,
    chain: &str,
    rule: &[String],
) -> Result<(), PlatformError> {
    let mut args = strings(&["-w", "-t", table, "-D", chain]);
    args.extend(rule.iter().cloned());
    platform.run(Tool::Iptables, &args).map(|_| ())
}

fn delete_hook(
    platform: &LinuxRouterPlatform,
    table: &str,
    parent: &str,
    token: &str,
    chain: &str,
) -> Result<(), PlatformError> {
    platform
        .run(
            Tool::Iptables,
            &strings(&[
                "-w",
                "-t",
                table,
                "-D",
                parent,
                "-m",
                "comment",
                "--comment",
                token,
                "-j",
                chain,
            ]),
        )
        .map(|_| ())
}

fn remove_exact_chain(
    platform: &LinuxRouterPlatform,
    table: &str,
    chain: &str,
    rules: &[Vec<String>],
) -> Result<(), PlatformError> {
    verify_chain(platform, table, chain, rules)?;
    let all = platform
        .run(Tool::Iptables, &strings(&["-w", "-t", table, "-S"]))?
        .stdout;
    if !exact_chain_references(&all, chain).is_some_and(|references| references.is_empty()) {
        return Err(PlatformError::Conflict(format!(
            "{table}/{chain} retained a foreign reference"
        )));
    }
    platform.run(Tool::Iptables, &strings(&["-w", "-t", table, "-F", chain]))?;
    platform
        .run(Tool::Iptables, &strings(&["-w", "-t", table, "-X", chain]))
        .map(|_| ())
}

fn replace_chain_body(
    platform: &LinuxRouterPlatform,
    table: &str,
    chain: &str,
    old: &[Vec<String>],
    new: &[Vec<String>],
) -> Result<(), PlatformError> {
    verify_chain(platform, table, chain, old)?;
    for rule in old.iter().rev() {
        delete_rule(platform, table, chain, rule)?;
    }
    let mut installed: Vec<Vec<String>> = Vec::new();
    for rule in new {
        if let Err(error) = append_rule(platform, table, chain, rule) {
            for rule in installed.iter().rev() {
                let _ = delete_rule(platform, table, chain, rule);
            }
            for rule in old {
                let _ = append_rule(platform, table, chain, rule);
            }
            return Err(error);
        }
        installed.push(rule.clone());
    }
    Ok(())
}

fn rollback_chain(platform: &LinuxRouterPlatform, table: &str, chain: &str, rules: &[Vec<String>]) {
    for rule in rules.iter().rev() {
        let _ = delete_rule(platform, table, chain, rule);
    }
    let _ = platform.run(Tool::Iptables, &strings(&["-w", "-t", table, "-X", chain]));
}

fn chains_absent(platform: &LinuxRouterPlatform) -> bool {
    [
        ("filter", TAILSCALE_INPUT_CHAIN),
        ("filter", TAILSCALE_FORWARD_CHAIN),
        ("nat", TAILSCALE_NAT_CHAIN),
    ]
    .into_iter()
    .all(|(table, chain)| {
        platform
            .run_probe(Tool::Iptables, &strings(&["-w", "-t", table, "-S", chain]))
            .is_ok_and(|output| !output.success)
    })
}

fn require_marker(path: &str, expected: &str) -> Result<(), PlatformError> {
    if marker_token(path)? != expected {
        return Err(PlatformError::Conflict(format!(
            "ownership marker {path} does not match"
        )));
    }
    Ok(())
}

fn marker_token(path: &str) -> Result<String, PlatformError> {
    let value = storage::read_private_small_optional(path, 128)?
        .ok_or_else(|| PlatformError::Conflict(format!("ownership marker {path} is absent")))?;
    let token = value.trim();
    storage::validate_token(token)?;
    Ok(token.to_owned())
}

fn private_log(path: &str) -> Result<File, PlatformError> {
    let mut options = OpenOptions::new();
    options.write(true).truncate(true).mode(0o600);
    match fs::symlink_metadata(path) {
        Ok(_) => storage::require_private_root_file(path)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            options.create_new(true);
        }
        Err(error) => return Err(PlatformError::Io(format!("inspect {path}: {error}"))),
    }
    options
        .open(path)
        .map_err(|error| PlatformError::Io(format!("open {path}: {error}")))
}

fn socket_owned_by_root(path: &str) -> Result<bool, PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_socket() && metadata.uid() == 0),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(PlatformError::ProbeFailed(format!(
            "inspect Tailscale socket: {error}"
        ))),
    }
}

fn socket_identity(path: &str) -> Result<Option<RuntimeNodeIdentity>, PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() && metadata.uid() == 0 => {
            Ok(Some(RuntimeNodeIdentity::from_metadata(&metadata)))
        }
        Ok(_) => Err(PlatformError::Conflict(format!(
            "Tailscale socket path {path} is not an owned root socket"
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PlatformError::ProbeFailed(format!(
            "inspect Tailscale socket: {error}"
        ))),
    }
}

fn interface_ifindex() -> Result<Option<u32>, PlatformError> {
    let path = format!("/sys/class/net/{TAILSCALE_INTERFACE}");
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(PlatformError::ProbeFailed(format!(
                "inspect tailscale0: {error}"
            )))
        }
    };
    if !metadata.file_type().is_dir() || !Path::new(&path).join("tun_flags").is_file() {
        return Err(PlatformError::Conflict(
            "tailscale0 is not the expected TUN interface".to_owned(),
        ));
    }
    let value = fs::read_to_string(Path::new(&path).join("ifindex"))
        .map_err(|error| PlatformError::ProbeFailed(format!("read tailscale0 ifindex: {error}")))?;
    parse_optional_nonzero_u32(Some(value.trim()), "tailscale0 ifindex")?
        .ok_or_else(|| PlatformError::ProbeFailed("tailscale0 ifindex is absent".to_owned()))
        .map(Some)
}

fn recorded_node_removal_allowed<T: Copy + Eq>(expected: Option<T>, actual: Option<T>) -> bool {
    actual.is_none() || expected == actual
}

fn remove_recorded_socket(
    path: &str,
    expected: Option<RuntimeNodeIdentity>,
) -> Result<(), PlatformError> {
    let actual = socket_identity(path)?;
    if !recorded_node_removal_allowed(expected, actual) {
        return Err(PlatformError::Conflict(
            "refusing to remove a mismatched tailscaled socket".to_owned(),
        ));
    }
    match (expected, actual) {
        (_, None) => Ok(()),
        (Some(expected), Some(actual)) if expected == actual => {
            if socket_identity(path)? != Some(expected) {
                return Err(PlatformError::Conflict(
                    "tailscaled socket identity changed before removal".to_owned(),
                ));
            }
            storage::remove_file_durable(path)
        }
        _ => Err(PlatformError::Conflict(
            "refusing to remove a mismatched tailscaled socket".to_owned(),
        )),
    }
}

fn remove_recorded_interface(
    platform: &LinuxRouterPlatform,
    expected: Option<u32>,
) -> Result<(), PlatformError> {
    let actual = interface_ifindex()?;
    if !recorded_node_removal_allowed(expected, actual) {
        return Err(PlatformError::Conflict(
            "refusing to remove a mismatched tailscale0 interface".to_owned(),
        ));
    }
    match (expected, actual) {
        (_, None) => Ok(()),
        (Some(expected), Some(actual)) if expected == actual => {
            if interface_ifindex()? != Some(expected) {
                return Err(PlatformError::Conflict(
                    "tailscale0 ifindex changed before removal".to_owned(),
                ));
            }
            platform.run(
                Tool::Ip,
                &strings(&["link", "delete", "dev", TAILSCALE_INTERFACE]),
            )?;
            if interface_ifindex()?.is_some() {
                return Err(PlatformError::Conflict(
                    "tailscale0 remained after exact owned deletion".to_owned(),
                ));
            }
            Ok(())
        }
        _ => Err(PlatformError::Conflict(
            "refusing to remove a mismatched tailscale0 interface".to_owned(),
        )),
    }
}

type UnknownRuntime = (
    Probe<TailscaleBackendState>,
    Probe<bool>,
    Probe<Option<Ipv4Addr>>,
    Probe<TailscalePreferences>,
    Probe<bool>,
    Probe<TailscaleConnectionKind>,
);

fn unknown_runtime(reason: String) -> UnknownRuntime {
    (
        Probe::Unknown(reason.clone()),
        Probe::Unknown(reason.clone()),
        Probe::Unknown(reason.clone()),
        Probe::Unknown(reason.clone()),
        Probe::Unknown(reason.clone()),
        Probe::Unknown(reason),
    )
}

fn is_tailscale_ipv4(address: Ipv4Addr) -> bool {
    let value = u32::from(address);
    let start = u32::from(Ipv4Addr::new(100, 64, 0, 0));
    let end = u32::from(Ipv4Addr::new(100, 127, 255, 255));
    (start..=end).contains(&value)
}

fn mode_text(mode: TailscaleMode) -> &'static str {
    match mode {
        TailscaleMode::Disabled => "disabled",
        TailscaleMode::RouterOnly => "router_only",
        TailscaleMode::LanSubnetAccess => "lan_subnet_access",
    }
}

fn bool_arg(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn words(values: &[&str]) -> Vec<String> {
    strings(values)
}

fn encode_argv(argv: &[Vec<u8>]) -> String {
    argv.iter()
        .map(|arg| {
            arg.iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(":")
}

fn decode_argv(encoded: &str) -> Result<Vec<Vec<u8>>, PlatformError> {
    encoded
        .split(':')
        .map(|arg| {
            if arg.is_empty() || arg.len() % 2 != 0 {
                return Err(PlatformError::InvalidState(
                    "tailscaled argv encoding is invalid".to_owned(),
                ));
            }
            (0..arg.len())
                .step_by(2)
                .map(|index| {
                    u8::from_str_radix(&arg[index..index + 2], 16).map_err(|_| {
                        PlatformError::InvalidState("tailscaled argv hex is invalid".to_owned())
                    })
                })
                .collect()
        })
        .collect()
}

fn parse_runtime_node_identity(
    value: Option<&str>,
) -> Result<Option<RuntimeNodeIdentity>, PlatformError> {
    let value = value.ok_or_else(|| {
        PlatformError::InvalidState("tailscaled socket identity record is absent".to_owned())
    })?;
    if value == "-" {
        return Ok(None);
    }
    let (dev, ino) = value.split_once(':').ok_or_else(|| {
        PlatformError::InvalidState("tailscaled socket identity record is invalid".to_owned())
    })?;
    let dev = dev
        .parse::<u64>()
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| {
            PlatformError::InvalidState("tailscaled socket device record is invalid".to_owned())
        })?;
    let ino = ino
        .parse::<u64>()
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| {
            PlatformError::InvalidState("tailscaled socket inode record is invalid".to_owned())
        })?;
    Ok(Some(RuntimeNodeIdentity { dev, ino }))
}

fn parse_optional_nonzero_u32(
    value: Option<&str>,
    label: &str,
) -> Result<Option<u32>, PlatformError> {
    let value =
        value.ok_or_else(|| PlatformError::InvalidState(format!("{label} record is absent")))?;
    if value == "-" {
        return Ok(None);
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value != 0)
        .map(Some)
        .ok_or_else(|| PlatformError::InvalidState(format!("invalid {label} record")))
}

fn parse_nonzero(value: Option<&str>, label: &str) -> Result<u64, PlatformError> {
    value
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value != 0)
        .ok_or_else(|| PlatformError::InvalidState(format!("invalid {label} record")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS_RUNNING: &str =
        include_str!("../../../tests/fixtures/tailscale-status-running.json");
    const STATUS_NEEDS_LOGIN: &str =
        include_str!("../../../tests/fixtures/tailscale-status-needs-login.json");
    const PREFS_ROUTER_ONLY: &str =
        include_str!("../../../tests/fixtures/tailscale-prefs-router-only.json");
    const PREFS_NEEDS_LOGIN: &str =
        include_str!("../../../tests/fixtures/tailscale-prefs-needs-login.json");
    const PREFS_LAN: &str = include_str!("../../../tests/fixtures/tailscale-prefs-lan.json");

    #[test]
    fn selected_status_fixture_requires_one_cgnat_ipv4_and_narrow_backend_shape() {
        let parsed = parse_status_json(STATUS_RUNNING).unwrap();
        assert_eq!(parsed.backend, TailscaleBackendState::Running);
        assert!(parsed.authenticated);
        assert_eq!(parsed.ipv4, Some(Ipv4Addr::new(100, 64, 0, 7)));
        assert_eq!(parsed.connection, TailscaleConnectionKind::Unknown);
        assert!(parse_status_json(
            &STATUS_RUNNING.replace("\"100.64.0.7\"", "\"100.64.0.7\", \"100.64.0.8\"")
        )
        .is_err());
        assert!(parse_status_json(&STATUS_RUNNING.replace("Running", "FutureState")).is_ok());
    }

    #[test]
    fn selected_needs_login_fixture_accepts_required_null_ip_field() {
        let parsed = parse_status_json(STATUS_NEEDS_LOGIN).unwrap();
        assert_eq!(parsed.backend, TailscaleBackendState::NeedsLogin);
        assert!(!parsed.authenticated);
        assert_eq!(parsed.ipv4, None);
        assert_eq!(
            parse_status_login_url(STATUS_NEEDS_LOGIN)
                .unwrap()
                .unwrap()
                .as_str(),
            "https://login.tailscale.com/a/redacted"
        );
        assert!(parse_status_login_url(
            &STATUS_NEEDS_LOGIN.replace("https://login.tailscale.com", "https://example.com")
        )
        .is_err());
        assert!(parse_status_login_url(&STATUS_NEEDS_LOGIN.replace(
            "  \"AuthURL\": \"https://login.tailscale.com/a/redacted\",\n",
            ""
        ))
        .is_err());
        assert!(
            parse_status_json(&STATUS_NEEDS_LOGIN.replace("  \"TailscaleIPs\": null,\n", ""))
                .is_err()
        );
        assert!(parse_status_json(
            &STATUS_NEEDS_LOGIN.replace("\"TailscaleIPs\": null", "\"TailscaleIPs\": {}")
        )
        .is_err());
    }

    #[test]
    fn long_poll_commands_use_only_the_remaining_absolute_deadline() {
        assert!(remaining_command_timeout(Instant::now()).is_none());
        let timeout = remaining_command_timeout(Instant::now() + Duration::from_secs(60)).unwrap();
        assert!(timeout <= Duration::from_secs(3));
        assert!(timeout > Duration::ZERO);
    }

    #[test]
    fn backend_wait_accepts_only_stable_login_or_running_states() {
        assert!(!backend_is_stable(TailscaleBackendState::Stopped));
        assert!(!backend_is_stable(TailscaleBackendState::Unknown));
        assert!(backend_is_stable(TailscaleBackendState::NeedsLogin));
        assert!(backend_is_stable(TailscaleBackendState::Running));
        assert_eq!(
            parse_status_json(&STATUS_RUNNING.replace("\"Running\"", "\"Starting\""))
                .unwrap()
                .backend,
            TailscaleBackendState::Unknown
        );
    }

    #[test]
    fn login_url_wait_retries_only_an_empty_safe_needs_login_url() {
        let empty = STATUS_NEEDS_LOGIN.replace("https://login.tailscale.com/a/redacted", "");
        assert_eq!(parse_status_login_url(&empty).unwrap(), None);

        let mut statuses =
            vec![empty.clone(), empty.clone(), STATUS_NEEDS_LOGIN.to_owned()].into_iter();
        assert_eq!(
            wait_for_login_url(3, Duration::ZERO, || Ok(statuses.next().unwrap()))
                .unwrap()
                .as_str(),
            "https://login.tailscale.com/a/redacted"
        );
        assert!(matches!(
            wait_for_login_url(2, Duration::ZERO, || Ok(empty.clone())),
            Err(PlatformError::UnsafeToCutOver(_))
        ));

        let mut probes = 0;
        assert!(wait_for_login_url(3, Duration::ZERO, || {
            probes += 1;
            Ok(STATUS_NEEDS_LOGIN.replace("https://login.tailscale.com", "https://example.com"))
        })
        .is_err());
        assert_eq!(probes, 1);
    }

    #[test]
    fn unauthenticated_prefs_accept_only_the_selected_empty_defaults() {
        assert_eq!(
            parse_prefs_json(PREFS_NEEDS_LOGIN).unwrap(),
            (
                TailscalePreferences {
                    accept_dns: true,
                    accept_routes: false,
                    advertise_exit_node: false,
                    exit_node_selected: false,
                    ssh_enabled: false,
                    netfilter_off: false,
                },
                false,
            )
        );
        assert!(parse_prefs_json(&PREFS_NEEDS_LOGIN.replace(
            "\"ControlURL\": \"\"",
            "\"ControlURL\": \"https://example.com\""
        ))
        .is_err());
        assert!(parse_prefs_json(
            &PREFS_NEEDS_LOGIN.replace("\"AdvertiseRoutes\": null", "\"AdvertiseRoutes\": {}")
        )
        .is_err());
    }

    #[test]
    fn prefs_fixtures_accept_only_fixed_flags_and_exact_route_set() {
        assert_eq!(
            parse_prefs_json(PREFS_ROUTER_ONLY).unwrap(),
            (TailscalePreferences::FIXED, false)
        );
        assert_eq!(
            parse_prefs_json(PREFS_LAN).unwrap(),
            (TailscalePreferences::FIXED, true)
        );
        assert!(parse_prefs_json(
            &PREFS_LAN.replace("\"192.168.8.0/24\"", "\"192.168.8.0/24\", \"10.0.0.0/8\"")
        )
        .is_err());
        assert!(
            parse_prefs_json(&PREFS_LAN.replace("\"CorpDNS\": false", "\"CorpDNS\": null"))
                .is_err()
        );
    }

    #[test]
    fn login_uses_the_fixed_login_subcommand_and_complete_safe_preferences() {
        assert_eq!(
            login_args(),
            [
                "login",
                "--timeout=2s",
                "--accept-dns=false",
                "--accept-routes=false",
                "--advertise-exit-node=false",
                "--exit-node=",
                "--ssh=false",
                "--netfilter-mode=off",
                "--advertise-routes=",
            ]
        );
    }

    #[test]
    fn ip_parser_rejects_ambiguity_and_non_cgnat_addresses() {
        assert_eq!(
            parse_ip_output("100.100.100.100\n").unwrap(),
            Some(Ipv4Addr::new(100, 100, 100, 100))
        );
        assert!(parse_ip_output("100.64.0.1\n100.64.0.2\n").is_err());
        assert!(parse_ip_output("192.168.8.1\n").is_err());
    }

    #[test]
    fn firewall_rules_match_target_iptables_canonical_output() {
        let input = tailscale_input_rules("router");
        let forward = tailscale_forward_rules("router", None);
        let canonical_drop = words(&[
            "-s",
            TAILSCALE_CGNAT_SUBNET,
            "!",
            "-i",
            TAILSCALE_INTERFACE,
            "-j",
            "DROP",
        ]);
        assert_eq!(input[1], canonical_drop);
        assert_eq!(forward[1], canonical_drop);

        let input_output = format!(
            "-N {TAILSCALE_INPUT_CHAIN}\n-A {TAILSCALE_INPUT_CHAIN} -m comment --comment router\n-A {TAILSCALE_INPUT_CHAIN} -s {TAILSCALE_CGNAT_SUBNET} ! -i {TAILSCALE_INTERFACE} -j DROP\n-A {TAILSCALE_INPUT_CHAIN} -i {WAN_INTERFACE} -p udp -m udp --dport {TAILSCALE_UDP_PORT} -j ACCEPT\n-A {TAILSCALE_INPUT_CHAIN} -i {TAILSCALE_INTERFACE} -p tcp -m tcp --dport {TAILSCALE_MANAGEMENT_HTTP_PORT} -j ACCEPT\n-A {TAILSCALE_INPUT_CHAIN} -i {TAILSCALE_INTERFACE} -j DROP\n-A {TAILSCALE_INPUT_CHAIN} -j RETURN\n"
        );
        let qualified = input
            .iter()
            .map(|rule| {
                let mut qualified = words(&["-A", TAILSCALE_INPUT_CHAIN]);
                qualified.extend(rule.iter().cloned());
                qualified
            })
            .collect::<Vec<_>>();
        assert!(chain_output_is_exact(
            &input_output,
            TAILSCALE_INPUT_CHAIN,
            &qualified
        ));

        let expanded = tailscale_forward_rules("router", Some("subnet"));
        let forward_output = format!(
            "-N {TAILSCALE_FORWARD_CHAIN}\n-A {TAILSCALE_FORWARD_CHAIN} -m comment --comment router\n-A {TAILSCALE_FORWARD_CHAIN} -s {TAILSCALE_CGNAT_SUBNET} ! -i {TAILSCALE_INTERFACE} -j DROP\n-A {TAILSCALE_FORWARD_CHAIN} -m comment --comment subnet\n-A {TAILSCALE_FORWARD_CHAIN} -d {LAN_SUBNET} -i {TAILSCALE_INTERFACE} -o {LAN_BRIDGE} -m conntrack --ctstate NEW,RELATED,ESTABLISHED -j ACCEPT\n-A {TAILSCALE_FORWARD_CHAIN} -s {LAN_SUBNET} -d {TAILSCALE_CGNAT_SUBNET} -i {LAN_BRIDGE} -o {TAILSCALE_INTERFACE} -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT\n-A {TAILSCALE_FORWARD_CHAIN} -s {LAN_SUBNET} -d {TAILSCALE_CGNAT_SUBNET} -i {LAN_BRIDGE} -o {TAILSCALE_INTERFACE} -m conntrack --ctstate NEW -j DROP\n-A {TAILSCALE_FORWARD_CHAIN} -i {TAILSCALE_INTERFACE} -j DROP\n-A {TAILSCALE_FORWARD_CHAIN} -o {TAILSCALE_INTERFACE} -j DROP\n-A {TAILSCALE_FORWARD_CHAIN} -j RETURN\n"
        );
        let qualified = expanded
            .iter()
            .map(|rule| {
                let mut qualified = words(&["-A", TAILSCALE_FORWARD_CHAIN]);
                qualified.extend(rule.iter().cloned());
                qualified
            })
            .collect::<Vec<_>>();
        assert!(chain_output_is_exact(
            &forward_output,
            TAILSCALE_FORWARD_CHAIN,
            &qualified
        ));
    }

    #[test]
    fn firewall_rules_are_fixed_and_subnet_rules_precede_router_only_drops() {
        let rules = tailscale_forward_rules("router", Some("subnet"));
        let allow = rules
            .iter()
            .position(|rule| rule.iter().any(|word| word == "NEW,RELATED,ESTABLISHED"))
            .unwrap();
        let drop = rules
            .iter()
            .position(|rule| rule == &words(&["-i", TAILSCALE_INTERFACE, "-j", "DROP"]))
            .unwrap();
        assert!(allow < drop);
        assert_eq!(
            tailscale_nat_rules("subnet")[1],
            words(&[
                "-s",
                TAILSCALE_CGNAT_SUBNET,
                "-d",
                TAILSCALE_LAN_ROUTE,
                "-o",
                LAN_BRIDGE,
                "-j",
                "MASQUERADE",
            ])
        );
        assert!(tailscale_input_rules("router").iter().any(|rule| {
            rule.iter()
                .any(|word| word == &TAILSCALE_MANAGEMENT_HTTP_PORT.to_string())
                && rule.iter().any(|word| word == "ACCEPT")
        }));
    }

    #[test]
    fn exited_identity_is_distinct_from_replaced_or_unknown_process_identity() {
        assert_eq!(
            classify_identity_process(34, None, false),
            IdentityProcessState::Exited
        );
        assert_eq!(
            classify_identity_process(34, Some((35, b'S')), false),
            IdentityProcessState::Replaced
        );
        assert_eq!(
            classify_identity_process(34, Some((34, b'S')), false),
            IdentityProcessState::Replaced
        );
        assert_eq!(
            classify_identity_process(34, Some((34, b'S')), true),
            IdentityProcessState::Live
        );
        assert_eq!(
            classify_identity_process(34, Some((34, b'Z')), false),
            IdentityProcessState::Exited
        );
        assert_eq!(
            classify_identity_process(34, Some((34, b'X')), false),
            IdentityProcessState::Exited
        );
    }

    #[test]
    fn exit_wait_does_not_treat_a_terminating_process_as_replaced() {
        let mut states = [
            IdentityProcessState::Live,
            IdentityProcessState::Live,
            IdentityProcessState::Exited,
        ]
        .into_iter();
        assert!(
            wait_for_identity_exit(3, Duration::ZERO, || { Ok(states.next().unwrap()) }).unwrap()
        );
    }

    #[test]
    fn exit_wait_rejects_replaced_identity_without_waiting_for_timeout() {
        let error =
            wait_for_identity_exit(3, Duration::ZERO, || Ok(IdentityProcessState::Replaced))
                .unwrap_err();
        assert!(matches!(error, PlatformError::Conflict(_)));
    }

    #[test]
    fn exit_wait_reports_a_still_live_identity_after_the_bound() {
        assert!(
            !wait_for_identity_exit(2, Duration::ZERO, || { Ok(IdentityProcessState::Live) })
                .unwrap()
        );
    }

    #[test]
    fn runtime_node_cleanup_requires_exact_recorded_identity() {
        let recorded = RuntimeNodeIdentity { dev: 12, ino: 34 };
        assert!(recorded_node_removal_allowed(Some(recorded), None));
        assert!(recorded_node_removal_allowed(
            Some(recorded),
            Some(recorded)
        ));
        assert!(!recorded_node_removal_allowed(
            Some(recorded),
            Some(RuntimeNodeIdentity { dev: 12, ino: 35 })
        ));
        assert!(!recorded_node_removal_allowed(None, Some(recorded)));
        assert!(!recorded_node_removal_allowed(Some(9_u32), Some(10_u32)));
    }

    #[test]
    fn tailscaled_identity_argv_is_fixed_and_round_trips_without_shell() {
        let identity = TailscaledIdentity {
            pid: 12,
            start_time: 34,
            executable: PathBuf::from(TAILSCALED_EXECUTABLE),
            argv: tailscaled_argv(),
            token: "hyz-tailscale-test".to_owned(),
            socket: Some(RuntimeNodeIdentity { dev: 56, ino: 78 }),
            interface_ifindex: Some(9),
        };
        assert_eq!(
            TailscaledIdentity::parse(&identity.serialize()).unwrap(),
            identity
        );
        let legacy = format!(
            "12\n34\n{}\nhyz-tailscale-test\n{}\n",
            TAILSCALED_EXECUTABLE,
            encode_argv(&tailscaled_argv())
        );
        let legacy = TailscaledIdentity::parse(&legacy).unwrap();
        assert_eq!(legacy.socket, None);
        assert_eq!(legacy.interface_ifindex, None);
        let args = tailscaled_argv()
            .into_iter()
            .map(|arg| String::from_utf8(arg).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                TAILSCALED_EXECUTABLE,
                "--state=/userdata/hyz-router/tailscale/tailscaled.state",
                "--socket=/run/hyz-tailscale/tailscaled.sock",
                "--tun=tailscale0",
                "--port=41641",
            ]
        );
    }
}
