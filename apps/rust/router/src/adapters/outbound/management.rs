use super::{process::process_start_time, storage};
use crate::{
    application::{
        dhcp::{DhcpEvent, DhcpPlatformPort, DHCP_HOOK_ROLE_ENV},
        ports::PlatformError,
    },
    domain::network::{LAN_MEMBER, WAN_INTERFACE},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    net::Ipv4Addr,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const WPA_CONFIG: &str = "/userdata/hyz-router/wpa_supplicant.conf";
pub const HOSTAPD_SOURCE_CONFIG: &str = "/userdata/hyz-router/hostapd-sta-ap.conf";
pub const HOSTAPD_RUNTIME_CONFIG: &str = "/run/hyz-router/hostapd.rust.conf";
pub const DNSMASQ_RUNTIME_CONFIG: &str = "/run/hyz-router/dnsmasq.rust.conf";
pub const ROUTER_EXECUTABLE: &str = "/usr/bin/hyz-router";
const RESOLV_CONFIG: &str = "/etc/resolv.conf";
const DHCP_OWNERSHIP_RECORD: &str = "/run/hyz-router/udhcpc.lease-generation.json";
const PROCESS_WAIT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const MAX_CONFIG_SIZE: usize = 1024 * 1024;
const MAX_RESOLV_SIZE: usize = 64 * 1024;

pub const DNSMASQ_CONFIG: &str = "# DHCP and DNS proxy for the isolated br-lan LAN.\n\
interface=br-lan\n\
bind-interfaces\n\
listen-address=192.168.8.1\n\
except-interface=lo\n\
domain-needed\n\
bogus-priv\n\
dhcp-authoritative\n\
dhcp-range=192.168.8.100,192.168.8.199,255.255.255.0,10m\n\
dhcp-option=3,192.168.8.1\n\
dhcp-option=6,192.168.8.1\n\
dhcp-leasefile=/run/hyz-router/dnsmasq.leases\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DhcpOwnership {
    version: u8,
    address: OwnedAddress,
    routes: Vec<OwnedRoute>,
    resolver_entries: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedAddress {
    cidr: String,
    broadcast: Option<Ipv4Addr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedRoute {
    destination: String,
    gateway: Ipv4Addr,
    metric: Option<u32>,
}

impl DhcpOwnership {
    fn from_lease(lease: &crate::application::dhcp::DhcpLease) -> Self {
        let routes = if lease.static_routes.is_empty() {
            lease
                .routers
                .iter()
                .map(|gateway| OwnedRoute {
                    destination: "default".to_owned(),
                    gateway: *gateway,
                    metric: Some(600),
                })
                .collect()
        } else {
            lease
                .static_routes
                .iter()
                .map(|(destination, gateway)| OwnedRoute {
                    metric: matches!(destination.as_str(), "default" | "0.0.0.0/0").then_some(600),
                    destination: destination.clone(),
                    gateway: *gateway,
                })
                .collect()
        };
        Self {
            version: 1,
            address: OwnedAddress {
                cidr: format!("{}/{}", lease.address, lease.prefix),
                broadcast: lease.broadcast,
            },
            routes,
            resolver_entries: lease.resolver_lines(WAN_INTERFACE),
        }
    }

    fn validate(&self) -> Result<(), PlatformError> {
        if self.version != 1 || self.routes.len() > 64 || self.resolver_entries.len() > 32 {
            return Err(invalid_dhcp_ownership());
        }
        validate_ipv4_cidr(&self.address.cidr).map_err(|_| invalid_dhcp_ownership())?;
        for route in &self.routes {
            if route.destination != "default" && route.destination != "0.0.0.0/0" {
                validate_ipv4_cidr(&route.destination).map_err(|_| invalid_dhcp_ownership())?;
            }
            if route.metric.is_some() && route.metric != Some(600) {
                return Err(invalid_dhcp_ownership());
            }
            if matches!(route.destination.as_str(), "default" | "0.0.0.0/0")
                != (route.metric == Some(600))
            {
                return Err(invalid_dhcp_ownership());
            }
        }
        if self.resolver_entries.iter().any(|entry| {
            entry.contains('\n')
                || !entry.ends_with(&format!("# {WAN_INTERFACE}"))
                || !(entry.starts_with("search ") || entry.starts_with("nameserver "))
        }) {
            return Err(invalid_dhcp_ownership());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ManagementService {
    WpaSupplicant,
    Udhcpc,
    Hostapd,
    Dnsmasq,
}

const SERVICES: [ManagementService; 4] = [
    ManagementService::WpaSupplicant,
    ManagementService::Udhcpc,
    ManagementService::Hostapd,
    ManagementService::Dnsmasq,
];

impl ManagementService {
    pub(crate) const fn executable(self) -> &'static str {
        match self {
            Self::WpaSupplicant => "/usr/sbin/wpa_supplicant",
            Self::Udhcpc => "/sbin/udhcpc",
            Self::Hostapd => "/usr/sbin/hostapd",
            Self::Dnsmasq => "/usr/sbin/dnsmasq",
        }
    }

    pub(crate) const fn argv(self) -> &'static [&'static str] {
        match self {
            Self::WpaSupplicant => &["-i", WAN_INTERFACE, "-D", "nl80211,wext", "-c", WPA_CONFIG],
            Self::Udhcpc => &["-f", "-i", WAN_INTERFACE, "-s", ROUTER_EXECUTABLE],
            Self::Hostapd => &[HOSTAPD_RUNTIME_CONFIG],
            Self::Dnsmasq => &[
                "--no-daemon",
                "--conf-file=/run/hyz-router/dnsmasq.rust.conf",
            ],
        }
    }

    const fn record(self) -> &'static str {
        match self {
            Self::WpaSupplicant => "/run/hyz-router/wpa_supplicant.identity",
            Self::Udhcpc => "/run/hyz-router/udhcpc.identity",
            Self::Hostapd => "/run/hyz-router/hostapd.identity",
            Self::Dnsmasq => "/run/hyz-router/dnsmasq.identity",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::WpaSupplicant => "wpa_supplicant",
            Self::Udhcpc => "udhcpc",
            Self::Hostapd => "hostapd",
            Self::Dnsmasq => "dnsmasq",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceIdentity {
    pid: u32,
    start_time: u64,
    executable: PathBuf,
    argv: Vec<String>,
}

impl ServiceIdentity {
    fn encode(&self) -> String {
        let mut record = format!(
            "hyz-process-v1\n{}\n{}\n{}\n{}\n",
            self.pid,
            self.start_time,
            self.executable.display(),
            self.argv.len()
        );
        for argument in &self.argv {
            record.push_str(argument);
            record.push('\n');
        }
        record
    }

    fn decode(record: &str, service: ManagementService) -> Result<Self, PlatformError> {
        let mut lines = record.lines();
        if lines.next() != Some("hyz-process-v1") {
            return Err(invalid_identity(service));
        }
        let pid = lines
            .next()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|pid| *pid > 1)
            .ok_or_else(|| invalid_identity(service))?;
        let start_time = lines
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| invalid_identity(service))?;
        let executable = lines.next().ok_or_else(|| invalid_identity(service))?;
        if executable.is_empty() {
            return Err(invalid_identity(service));
        }
        let argument_count = lines
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|count| *count <= 16)
            .ok_or_else(|| invalid_identity(service))?;
        let argv = lines
            .by_ref()
            .take(argument_count)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if argv.len() != argument_count || lines.next().is_some() {
            return Err(invalid_identity(service));
        }
        Ok(Self {
            pid,
            start_time,
            executable: PathBuf::from(executable),
            argv,
        })
    }

    fn matches(&self, service: ManagementService) -> Result<bool, PlatformError> {
        if self.argv != expected_command_line(service)? {
            return Ok(false);
        }
        if process_start_time(self.pid)? != Some(self.start_time) {
            return Ok(false);
        }
        let executable = match fs::read_link(format!("/proc/{}/exe", self.pid)) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "inspect {} executable: {error}",
                    service.label()
                )))
            }
        };
        if executable != self.executable || executable != resolved_executable(service)? {
            return Ok(false);
        }
        Ok(read_process_argv(self.pid)? == self.argv)
    }
}

impl super::process::LinuxRouterPlatform {
    pub(crate) fn ensure_management_services(&self) -> Result<(), PlatformError> {
        validate_private_source(WPA_CONFIG)?;
        validate_private_source(HOSTAPD_SOURCE_CONFIG)?;

        wait_for_interface_presence(WAN_INTERFACE, Duration::from_secs(20))?;
        wait_for_interface_presence(LAN_MEMBER, Duration::from_secs(20))?;
        self.stop_owned_management_services()?;
        self.refuse_foreign_management_processes()?;
        prepare_runtime_configs()?;

        let mut started = Vec::new();
        let result = (|| {
            run_ip(&["link", "set", "dev", WAN_INTERFACE, "up"])?;
            self.start_service(ManagementService::WpaSupplicant)?;
            started.push(ManagementService::WpaSupplicant);

            // udhcpc keeps retrying while the STA is disconnected. LAN management must come up
            // even when no upstream AP or DHCP lease is currently available.
            self.start_service(ManagementService::Udhcpc)?;
            started.push(ManagementService::Udhcpc);

            run_ip(&["link", "set", "dev", LAN_MEMBER, "up"])?;
            self.start_service(ManagementService::Hostapd)?;
            started.push(ManagementService::Hostapd);
            self.wait_for_hostapd(Duration::from_secs(15))?;

            self.start_service(ManagementService::Dnsmasq)?;
            started.push(ManagementService::Dnsmasq);
            self.wait_for_identity(ManagementService::Dnsmasq, PROCESS_WAIT)?;

            if !self.management_services_ready()? {
                return Err(PlatformError::ProbeFailed(
                    "management services did not reach ready state".to_owned(),
                ));
            }
            Ok(())
        })();
        if result.is_err() {
            for service in started.into_iter().rev() {
                let _ = self.stop_service(service);
            }
        }
        result
    }

    pub(crate) fn management_services_ready(&self) -> Result<bool, PlatformError> {
        for service in SERVICES {
            let Some(identity) = self.read_service_identity(service)? else {
                if service_executable_process_count(service)? != 0
                    || foreign_candidate_exists(service, None)?
                {
                    return Err(PlatformError::Conflict(format!(
                        "unowned {} process is present",
                        service.label()
                    )));
                }
                return Ok(false);
            };
            if process_start_time(identity.pid)? != Some(identity.start_time) {
                return Ok(false);
            }
            if !identity.matches(service)? {
                return Err(PlatformError::Conflict(format!(
                    "{} identity does not match its live process",
                    service.label()
                )));
            }
            if foreign_candidate_exists(service, Some(identity.pid))? {
                return Err(PlatformError::Conflict(format!(
                    "conflicting {} process signature is present",
                    service.label()
                )));
            }
            match service_executable_process_count(service)? {
                1 => {}
                0 => return Ok(false),
                _ => {
                    return Err(PlatformError::Conflict(format!(
                        "multiple {} executable instances are present",
                        service.label()
                    )))
                }
            }
        }
        // Process identity/cardinality plus an enabled AP define management health. STA
        // association and DHCP ownership are forwarding readiness, not LAN readiness.
        self.hostapd_enabled()
    }

    fn start_service(&self, service: ManagementService) -> Result<(), PlatformError> {
        if self.read_service_identity(service)?.is_some() {
            return Err(PlatformError::Conflict(format!(
                "{} identity already exists",
                service.label()
            )));
        }
        let mut command = Command::new(service.executable());
        command.args(service.argv()).env_clear().env("LC_ALL", "C");
        if service == ManagementService::Udhcpc {
            command.env(DHCP_HOOK_ROLE_ENV, "v1");
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|error| {
            PlatformError::Io(format!("start {} directly: {error}", service.label()))
        })?;
        let pid = child.id();
        let identity = match identify_spawned_service(service, pid, PROCESS_WAIT) {
            Ok(identity) => identity,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        if let Err(error) =
            storage::atomic_write_private(service.record(), identity.encode().as_bytes())
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        super::process::reap_in_background(child, service.label());
        Ok(())
    }

    pub(crate) fn stop_owned_management_services(&self) -> Result<(), PlatformError> {
        for service in SERVICES.into_iter().rev() {
            self.stop_service(service)?;
        }
        // udhcpc normally emits deconfig on USR2 through the daemon control socket. Perform the
        // same exact owned-generation cleanup directly as well so daemon shutdown remains correct
        // after control accepts have stopped, and so a failed callback cannot strand routes/DNS.
        self.apply_dhcp_event(DhcpEvent::Deconfig)
    }

    fn stop_service(&self, service: ManagementService) -> Result<(), PlatformError> {
        let Some(identity) = self.read_service_identity(service)? else {
            return Ok(());
        };
        if process_start_time(identity.pid)? != Some(identity.start_time) {
            fs::remove_file(service.record()).map_err(|error| {
                PlatformError::Io(format!(
                    "remove stale {} identity: {error}",
                    service.label()
                ))
            })?;
            return Ok(());
        }
        if !identity.matches(service)? {
            return Err(PlatformError::Conflict(format!(
                "refusing to stop {} because PID/start/exe/argv identity is not exact",
                service.label()
            )));
        }
        if service == ManagementService::Udhcpc {
            let _ = signal(identity.pid, "-USR2");
            thread::sleep(Duration::from_secs(1));
            if !identity.matches(service)? {
                fs::remove_file(service.record()).map_err(|error| {
                    PlatformError::Io(format!("remove {} identity: {error}", service.label()))
                })?;
                return Ok(());
            }
        }
        signal(identity.pid, "-TERM")?;
        let deadline = Instant::now() + PROCESS_WAIT;
        while Instant::now() < deadline {
            if !identity.matches(service)? {
                fs::remove_file(service.record()).map_err(|error| {
                    PlatformError::Io(format!("remove {} identity: {error}", service.label()))
                })?;
                return Ok(());
            }
            thread::sleep(POLL_INTERVAL);
        }
        if !identity.matches(service)? {
            fs::remove_file(service.record()).map_err(|error| {
                PlatformError::Io(format!("remove {} identity: {error}", service.label()))
            })?;
            return Ok(());
        }
        signal(identity.pid, "-KILL")?;
        let kill_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < kill_deadline {
            if !identity.matches(service)? {
                fs::remove_file(service.record()).map_err(|error| {
                    PlatformError::Io(format!("remove {} identity: {error}", service.label()))
                })?;
                return Ok(());
            }
            thread::sleep(POLL_INTERVAL);
        }
        Err(PlatformError::Conflict(format!(
            "{} remained alive after bounded stop; identity record retained",
            service.label()
        )))
    }

    fn read_service_identity(
        &self,
        service: ManagementService,
    ) -> Result<Option<ServiceIdentity>, PlatformError> {
        storage::read_small_optional(service.record(), 4096)?
            .map(|record| ServiceIdentity::decode(&record, service))
            .transpose()
    }

    fn wait_for_identity(
        &self,
        service: ManagementService,
        timeout: Duration,
    ) -> Result<(), PlatformError> {
        wait_until(
            timeout,
            || {
                let Some(identity) = self.read_service_identity(service)? else {
                    return Ok(false);
                };
                Ok(identity.matches(service)?
                    && has_exact_management_cardinality(service_executable_process_count(service)?))
            },
            service.label(),
        )
    }

    fn wait_for_hostapd(&self, timeout: Duration) -> Result<(), PlatformError> {
        wait_until(timeout, || self.hostapd_enabled(), "AP enablement")
    }

    pub(crate) fn wait_for_sta_route(&self, timeout: Duration) -> Result<(), PlatformError> {
        wait_until(
            timeout,
            || self.owned_sta_address_and_route_ready(),
            "recorded DHCP STA address and exact owned default route with metric 600",
        )
    }

    fn hostapd_enabled(&self) -> Result<bool, PlatformError> {
        let output =
            self.run_management_probe("/usr/bin/hostapd_cli", &["-i", LAN_MEMBER, "status"])?;
        Ok(output.lines().any(|line| line == "state=ENABLED"))
    }

    pub(crate) fn owned_sta_address_and_route_ready(&self) -> Result<bool, PlatformError> {
        let Some(ownership) = read_dhcp_ownership()? else {
            return Ok(false);
        };
        let addresses = self.run_management_probe(
            "/usr/sbin/ip",
            &["-o", "-4", "address", "show", "dev", WAN_INTERFACE],
        )?;
        if !address_output_contains(&addresses, &ownership.address.cidr) {
            return Ok(false);
        }

        let owned_defaults = ownership
            .routes
            .iter()
            .filter(|route| {
                matches!(route.destination.as_str(), "default" | "0.0.0.0/0")
                    && route.metric == Some(600)
            })
            .collect::<Vec<_>>();
        if owned_defaults.len() != 1 {
            return Ok(false);
        }
        let routes =
            self.run_management_probe("/usr/sbin/ip", &["-4", "route", "show", "default"])?;
        let live = routes
            .lines()
            .filter(|line| route_line_uses_interface(line, WAN_INTERFACE))
            .collect::<Vec<_>>();
        Ok(live.len() == 1 && exact_default_route_line(live[0], owned_defaults[0]))
    }

    fn run_management_probe(
        &self,
        executable: &'static str,
        argv: &[&str],
    ) -> Result<String, PlatformError> {
        run_bounded(executable, argv, Duration::from_secs(3))
    }

    fn refuse_foreign_management_processes(&self) -> Result<(), PlatformError> {
        if !self
            .run_management_probe("/usr/sbin/wpa_cli", &["-i", WAN_INTERFACE, "status"])?
            .is_empty()
        {
            return Err(PlatformError::Conflict(
                "managed STA interface already responds to a supplicant control probe".to_owned(),
            ));
        }
        if !self
            .run_management_probe("/usr/bin/hostapd_cli", &["-i", LAN_MEMBER, "status"])?
            .is_empty()
        {
            return Err(PlatformError::Conflict(
                "managed AP interface already responds to an access-point control probe".to_owned(),
            ));
        }
        for service in SERVICES {
            if service_executable_process_count(service)? != 0
                || foreign_candidate_exists(service, None)?
            {
                return Err(PlatformError::Conflict(format!(
                    "refusing to take over an unowned {} process",
                    service.label()
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn apply_dhcp_event(&self, event: DhcpEvent) -> Result<(), PlatformError> {
        match event {
            DhcpEvent::Deconfig => {
                run_ip(&["link", "set", "dev", WAN_INTERFACE, "up"])?;
                let Some(previous) = read_dhcp_ownership()? else {
                    return Ok(());
                };
                if let Err(error) = remove_owned_generation(&previous) {
                    let _ = apply_owned_generation(&previous);
                    return Err(error);
                }
                if let Err(error) = storage::remove_file_durable(DHCP_OWNERSHIP_RECORD) {
                    let _ = apply_owned_generation(&previous);
                    return Err(error);
                }
                Ok(())
            }
            DhcpEvent::Lease { lease } => {
                let next = DhcpOwnership::from_lease(&lease);
                next.validate()?;
                let encoded = serde_json::to_vec(&next).map_err(|_| {
                    PlatformError::InvalidState("could not encode DHCP ownership record".to_owned())
                })?;
                let previous = read_dhcp_ownership()?;
                if let Some(previous) = &previous {
                    if let Err(error) = remove_owned_generation(previous) {
                        let _ = apply_owned_generation(previous);
                        return Err(error);
                    }
                }
                if let Err(error) = apply_owned_generation(&next) {
                    rollback_generation(&next, previous.as_ref());
                    return Err(error);
                }
                if let Err(error) = storage::atomic_write_private(DHCP_OWNERSHIP_RECORD, &encoded) {
                    rollback_generation(&next, previous.as_ref());
                    return Err(error);
                }
                Ok(())
            }
            DhcpEvent::NoChange => Ok(()),
        }
    }
}

impl DhcpPlatformPort for super::process::LinuxRouterPlatform {
    fn apply_dhcp_event(&self, event: DhcpEvent) -> Result<(), PlatformError> {
        super::process::LinuxRouterPlatform::apply_dhcp_event(self, event)
    }
}

fn prepare_runtime_configs() -> Result<(), PlatformError> {
    let source = storage::read_small_optional(HOSTAPD_SOURCE_CONFIG, MAX_CONFIG_SIZE)?
        .ok_or_else(|| PlatformError::InvalidState("hostapd source config is absent".to_owned()))?;
    let hostapd = render_hostapd_config(&source)?;
    storage::atomic_write_private(HOSTAPD_RUNTIME_CONFIG, hostapd.as_bytes())?;
    storage::atomic_write_private(DNSMASQ_RUNTIME_CONFIG, DNSMASQ_CONFIG.as_bytes())
}

pub(crate) fn render_hostapd_config(source: &str) -> Result<String, PlatformError> {
    if source.as_bytes().contains(&0) {
        return Err(PlatformError::InvalidState(
            "hostapd source config contains NUL".to_owned(),
        ));
    }
    let mut output = String::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed
            .split_once('=')
            .is_some_and(|(key, _)| key.trim_end() == "bridge")
        {
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }
    output.push_str("bridge=br-lan\n");
    Ok(output)
}

fn validate_private_source(path: &str) -> Result<(), PlatformError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| PlatformError::Io(format!("inspect required private config: {error}")))?;
    if !metadata.file_type().is_file() || metadata.uid() != 0 || metadata.mode() & 0o077 != 0 {
        return Err(PlatformError::InvalidState(
            "required management config is not a root-owned private regular file".to_owned(),
        ));
    }
    if metadata.len() > MAX_CONFIG_SIZE as u64 {
        return Err(PlatformError::InvalidState(
            "required management config exceeds size limit".to_owned(),
        ));
    }
    Ok(())
}

fn expected_command_line(service: ManagementService) -> Result<Vec<String>, PlatformError> {
    let mut argv = vec![service.executable().to_owned()];
    argv.extend(service.argv().iter().map(|value| (*value).to_owned()));
    Ok(argv)
}

fn resolved_executable(service: ManagementService) -> Result<PathBuf, PlatformError> {
    fs::canonicalize(service.executable()).map_err(|error| {
        PlatformError::ProbeFailed(format!(
            "resolve fixed {} executable: {error}",
            service.label()
        ))
    })
}

fn read_process_argv(pid: u32) -> Result<Vec<String>, PlatformError> {
    let bytes = fs::read(format!("/proc/{pid}/cmdline"))
        .map_err(|error| PlatformError::ProbeFailed(format!("read process argv: {error}")))?;
    if bytes.len() > 16 * 1024 || bytes.last() != Some(&0) {
        return Err(PlatformError::ProbeFailed(
            "process argv has invalid framing".to_owned(),
        ));
    }
    bytes[..bytes.len() - 1]
        .split(|byte| *byte == 0)
        .map(|argument| {
            String::from_utf8(argument.to_vec())
                .map_err(|_| PlatformError::ProbeFailed("process argv is not UTF-8".to_owned()))
        })
        .collect()
}

fn identify_spawned_service(
    service: ManagementService,
    pid: u32,
    timeout: Duration,
) -> Result<ServiceIdentity, PlatformError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(start_time) = process_start_time(pid)? {
            let executable = fs::read_link(format!("/proc/{pid}/exe")).map_err(|error| {
                PlatformError::ProbeFailed(format!(
                    "identify new {} executable: {error}",
                    service.label()
                ))
            })?;
            let identity = ServiceIdentity {
                pid,
                start_time,
                executable,
                argv: read_process_argv(pid)?,
            };
            if identity.matches(service)? {
                return Ok(identity);
            }
            return Err(PlatformError::Conflict(format!(
                "new {} process did not have the fixed identity",
                service.label()
            )));
        }
        if Instant::now() >= deadline {
            return Err(PlatformError::ProbeFailed(format!(
                "{} disappeared during startup",
                service.label()
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn foreign_candidate_exists(
    service: ManagementService,
    excluded_pid: Option<u32>,
) -> Result<bool, PlatformError> {
    let entries = fs::read_dir("/proc")
        .map_err(|error| PlatformError::ProbeFailed(format!("scan processes: {error}")))?;
    let expected_executable = resolved_executable(service)?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            PlatformError::ProbeFailed(format!("inspect process directory entry: {error}"))
        })?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        if excluded_pid == Some(pid) {
            continue;
        }
        if !process_name_can_match_service(service, pid)? {
            continue;
        }
        match read_process_argv(pid) {
            Ok(argv) if command_line_has_service_signature(service, &argv) => return Ok(true),
            Ok(_) => {}
            Err(error) => match fs::read_link(entry.path().join("exe")) {
                Ok(executable) if executable == expected_executable => return Err(error),
                Ok(_) => {}
                Err(io_error) if io_error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(error),
            },
        }
    }
    Ok(false)
}

fn has_exact_management_cardinality(count: usize) -> bool {
    count == 1
}

fn service_executable_process_count(service: ManagementService) -> Result<usize, PlatformError> {
    let expected = resolved_executable(service)?;
    let entries = fs::read_dir("/proc")
        .map_err(|error| PlatformError::ProbeFailed(format!("scan processes: {error}")))?;
    let mut count = 0;
    for entry in entries {
        let entry = entry.map_err(|error| {
            PlatformError::ProbeFailed(format!("inspect process directory entry: {error}"))
        })?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        if !process_name_can_match_service(service, pid)? {
            continue;
        }
        match fs::read_link(entry.path().join("exe")) {
            Ok(executable) if executable == expected => {
                let argv = read_process_argv(pid)?;
                if !executable_instance_matches_service(service, &argv) {
                    continue;
                }
                process_start_time(pid)?.ok_or_else(|| {
                    PlatformError::ProbeFailed(
                        "relevant process disappeared during identity inspection".to_owned(),
                    )
                })?;
                count += 1;
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                let argv = read_process_argv(pid)?;
                if command_line_has_service_signature(service, &argv) {
                    return Err(PlatformError::ProbeFailed(format!(
                        "inspect relevant {} process executable",
                        service.label()
                    )));
                }
            }
        }
    }
    Ok(count)
}

fn process_name_can_match_service(
    service: ManagementService,
    pid: u32,
) -> Result<bool, PlatformError> {
    if service != ManagementService::Udhcpc {
        return Ok(true);
    }
    match fs::read_to_string(format!("/proc/{pid}/comm")) {
        Ok(comm) => Ok(multicall_process_name_matches(service, comm.trim())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(PlatformError::ProbeFailed(format!(
            "read multicall process name: {error}"
        ))),
    }
}

fn multicall_process_name_matches(service: ManagementService, comm: &str) -> bool {
    service != ManagementService::Udhcpc || comm == service.label()
}

fn executable_instance_matches_service(service: ManagementService, argv: &[String]) -> bool {
    // udhcpc is a BusyBox applet, so canonicalizing /sbin/udhcpc yields /bin/busybox. Counting
    // executable identities alone would classify every live BusyBox applet as another DHCP client.
    service != ManagementService::Udhcpc || command_line_has_service_signature(service, argv)
}

pub(crate) fn command_line_has_service_signature(
    service: ManagementService,
    argv: &[String],
) -> bool {
    let Some(argv0) = argv.first() else {
        return false;
    };
    let argv0_matches = argv0 == service.executable()
        || Path::new(argv0).file_name().and_then(|name| name.to_str()) == Some(service.label());
    if !argv0_matches {
        return false;
    }
    match service {
        ManagementService::WpaSupplicant => interface_argument_matches(argv, WAN_INTERFACE),
        ManagementService::Udhcpc => interface_argument_matches(argv, WAN_INTERFACE),
        ManagementService::Hostapd | ManagementService::Dnsmasq => true,
    }
}

fn interface_argument_matches(argv: &[String], interface: &str) -> bool {
    contains_pair(argv, "-i", interface)
        || argv
            .iter()
            .any(|argument| argument.strip_prefix("-i") == Some(interface))
}

fn contains_pair(argv: &[String], key: &str, value: &str) -> bool {
    argv.windows(2)
        .any(|pair| pair[0] == key && pair[1] == value)
}

fn signal(pid: u32, signal: &str) -> Result<(), PlatformError> {
    let pid = pid.to_string();
    if run_status_bounded("/bin/kill", &[signal, pid.as_str()], Duration::from_secs(3))? {
        Ok(())
    } else {
        Err(PlatformError::CommandFailed(
            "signal of exactly identified process failed".to_owned(),
        ))
    }
}

pub(crate) fn wait_for_interface_presence(
    interface: &'static str,
    timeout: Duration,
) -> Result<(), PlatformError> {
    wait_until(
        timeout,
        || Ok(Path::new("/sys/class/net").join(interface).exists()),
        "required network interface",
    )
}

fn wait_until(
    timeout: Duration,
    mut probe: impl FnMut() -> Result<bool, PlatformError>,
    label: &str,
) -> Result<(), PlatformError> {
    let deadline = Instant::now() + timeout;
    loop {
        if probe()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(PlatformError::ProbeFailed(format!(
                "{label} exceeded its bounded readiness wait"
            )));
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn run_status_bounded(
    executable: &'static str,
    argv: &[&str],
    timeout: Duration,
) -> Result<bool, PlatformError> {
    let mut child = Command::new(executable)
        .args(argv)
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| PlatformError::Io(format!("start fixed operation: {error}")))?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| PlatformError::Io(format!("wait for fixed operation: {error}")))?
        {
            return Ok(status.success());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PlatformError::CommandFailed(
                "fixed operation exceeded its deadline".to_owned(),
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn run_bounded(
    executable: &'static str,
    argv: &[&str],
    timeout: Duration,
) -> Result<String, PlatformError> {
    let mut child = Command::new(executable)
        .args(argv)
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| PlatformError::Io(format!("start fixed management probe: {error}")))?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| PlatformError::Io(format!("wait for management probe: {error}")))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PlatformError::ProbeFailed(
                "management probe exceeded its deadline".to_owned(),
            ));
        }
        thread::sleep(Duration::from_millis(20));
    };
    let output = child
        .wait_with_output()
        .map_err(|error| PlatformError::Io(format!("collect management probe: {error}")))?;
    if !status.success() {
        return Ok(String::new());
    }
    if output.stdout.len() > 64 * 1024 {
        return Err(PlatformError::ProbeFailed(
            "management probe output exceeded limit".to_owned(),
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| PlatformError::ProbeFailed("management probe output is not UTF-8".to_owned()))
}

fn address_output_contains(output: &str, cidr: &str) -> bool {
    output.lines().any(|line| {
        line.split_whitespace()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair == ["inet", cidr])
    })
}

fn route_line_uses_interface(line: &str, interface: &str) -> bool {
    line.split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| pair == ["dev", interface])
}

fn exact_default_route_line(line: &str, route: &OwnedRoute) -> bool {
    let gateway = route.gateway.to_string();
    line.split_whitespace().collect::<Vec<_>>()
        == [
            "default",
            "via",
            gateway.as_str(),
            "dev",
            WAN_INTERFACE,
            "metric",
            "600",
        ]
}

fn read_dhcp_ownership() -> Result<Option<DhcpOwnership>, PlatformError> {
    let Some(record) = storage::read_small_optional(DHCP_OWNERSHIP_RECORD, 16 * 1024)? else {
        return Ok(None);
    };
    let ownership =
        serde_json::from_str::<DhcpOwnership>(&record).map_err(|_| invalid_dhcp_ownership())?;
    ownership.validate()?;
    Ok(Some(ownership))
}

fn apply_owned_generation(ownership: &DhcpOwnership) -> Result<(), PlatformError> {
    run_ip_owned(&address_args("add", &ownership.address))?;
    for route in &ownership.routes {
        run_ip_owned(&route_args("add", route))?;
    }
    replace_resolver_entries(&[], &ownership.resolver_entries)
}

fn remove_owned_generation(ownership: &DhcpOwnership) -> Result<(), PlatformError> {
    for route in ownership.routes.iter().rev() {
        run_ip_owned(&route_args("del", route))?;
    }
    run_ip_owned(&address_args("del", &ownership.address))?;
    replace_resolver_entries(&ownership.resolver_entries, &[])
}

fn rollback_generation(next: &DhcpOwnership, previous: Option<&DhcpOwnership>) {
    for route in next.routes.iter().rev() {
        let _ = run_ip_owned(&route_args("del", route));
    }
    let _ = run_ip_owned(&address_args("del", &next.address));
    let _ = replace_resolver_entries(&next.resolver_entries, &[]);
    if let Some(previous) = previous {
        let _ = apply_owned_generation(previous);
    }
}

fn address_args(operation: &str, address: &OwnedAddress) -> Vec<String> {
    let mut args = vec![
        "-4".to_owned(),
        "address".to_owned(),
        operation.to_owned(),
        address.cidr.clone(),
    ];
    if operation == "add" {
        if let Some(broadcast) = address.broadcast {
            args.extend(["broadcast".to_owned(), broadcast.to_string()]);
        }
    }
    args.extend(["dev".to_owned(), WAN_INTERFACE.to_owned()]);
    args
}

fn route_args(operation: &str, route: &OwnedRoute) -> Vec<String> {
    let mut args = vec![
        "-4".to_owned(),
        "route".to_owned(),
        operation.to_owned(),
        route.destination.clone(),
        "via".to_owned(),
        route.gateway.to_string(),
        "dev".to_owned(),
        WAN_INTERFACE.to_owned(),
    ];
    if let Some(metric) = route.metric {
        args.extend(["metric".to_owned(), metric.to_string()]);
    }
    args
}

fn validate_ipv4_cidr(value: &str) -> Result<(), PlatformError> {
    let (address, prefix) = value
        .split_once('/')
        .ok_or_else(|| PlatformError::InvalidState("invalid classless route".to_owned()))?;
    address
        .parse::<Ipv4Addr>()
        .map_err(|_| PlatformError::InvalidState("invalid classless route".to_owned()))?;
    prefix
        .parse::<u8>()
        .ok()
        .filter(|prefix| *prefix <= 32)
        .ok_or_else(|| PlatformError::InvalidState("invalid classless route".to_owned()))?;
    Ok(())
}

fn replace_resolver_entries(previous: &[String], next: &[String]) -> Result<(), PlatformError> {
    let existing = match fs::read(RESOLV_CONFIG) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(PlatformError::Io(format!("read resolver config: {error}"))),
    };
    if existing.len() > MAX_RESOLV_SIZE {
        return Err(PlatformError::InvalidState(
            "resolver config exceeds size limit".to_owned(),
        ));
    }
    let existing = String::from_utf8(existing)
        .map_err(|_| PlatformError::InvalidState("resolver config is not UTF-8".to_owned()))?;
    let output = replace_resolver_text(&existing, previous, next);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(Path::new(RESOLV_CONFIG))
        .map_err(|error| PlatformError::Io(format!("open resolver config: {error}")))?;
    file.write_all(output.as_bytes())
        .map_err(|error| PlatformError::Io(format!("write resolver config: {error}")))
}

fn replace_resolver_text(existing: &str, previous: &[String], next: &[String]) -> String {
    let mut removals = HashMap::<&str, usize>::new();
    for entry in previous {
        *removals.entry(entry).or_default() += 1;
    }
    let mut output = String::new();
    for line in existing.lines() {
        let remove = removals.get_mut(line).is_some_and(|remaining| {
            if *remaining == 0 {
                false
            } else {
                *remaining -= 1;
                true
            }
        });
        if !remove {
            output.push_str(line);
            output.push('\n');
        }
    }
    for line in next {
        output.push_str(line);
        output.push('\n');
    }
    output
}

fn run_ip(args: &[&str]) -> Result<(), PlatformError> {
    let args = args
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    run_ip_owned(&args)
}

fn run_ip_owned(args: &[String]) -> Result<(), PlatformError> {
    let arguments = args.iter().map(String::as_str).collect::<Vec<_>>();
    if run_status_bounded("/usr/sbin/ip", &arguments, Duration::from_secs(3))? {
        Ok(())
    } else {
        Err(PlatformError::CommandFailed(
            "fixed ip operation failed".to_owned(),
        ))
    }
}

fn invalid_identity(service: ManagementService) -> PlatformError {
    PlatformError::InvalidState(format!("invalid {} identity record", service.label()))
}

fn invalid_dhcp_ownership() -> PlatformError {
    PlatformError::InvalidState("invalid DHCP ownership record".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_argv_are_exact_golden_values() {
        assert_eq!(
            ManagementService::WpaSupplicant.argv(),
            &["-i", "wlan0", "-D", "nl80211,wext", "-c", WPA_CONFIG,]
        );
        assert_eq!(
            ManagementService::Udhcpc.argv(),
            &["-f", "-i", "wlan0", "-s", "/usr/bin/hyz-router"]
        );
        assert_eq!(ManagementService::Hostapd.argv(), &[HOSTAPD_RUNTIME_CONFIG]);
        assert_eq!(
            ManagementService::Dnsmasq.argv(),
            &[
                "--no-daemon",
                "--conf-file=/run/hyz-router/dnsmasq.rust.conf"
            ]
        );
    }

    #[test]
    fn process_identity_record_round_trips_exact_argv() {
        let identity = ServiceIdentity {
            pid: 42,
            start_time: 9001,
            executable: PathBuf::from("/usr/sbin/wpa_supplicant"),
            argv: vec![
                "/usr/sbin/wpa_supplicant".to_owned(),
                "-i".to_owned(),
                "wlan0".to_owned(),
            ],
        };
        let encoded = "hyz-process-v1\n42\n9001\n/usr/sbin/wpa_supplicant\n3\n/usr/sbin/wpa_supplicant\n-i\nwlan0\n";
        assert_eq!(identity.encode(), encoded);
        assert_eq!(
            ServiceIdentity::decode(encoded, ManagementService::WpaSupplicant).unwrap(),
            identity
        );
    }

    #[test]
    fn hostapd_render_removes_every_bridge_assignment_and_appends_owned_bridge() {
        let source = "interface=p2p0\nbridge=old0\n  bridge = old1\nssid=test\n";
        assert_eq!(
            render_hostapd_config(source).unwrap(),
            "interface=p2p0\nssid=test\nbridge=br-lan\n"
        );
    }

    #[test]
    fn dnsmasq_config_matches_validated_overlay_golden() {
        assert_eq!(
            DNSMASQ_CONFIG,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/dnsmasq.conf"
            ))
        );
    }

    #[test]
    fn foreign_signature_is_conservative_but_not_name_only() {
        let shell_owned = vec![
            "/usr/sbin/wpa_supplicant".to_owned(),
            "-B".to_owned(),
            "-iwlan0".to_owned(),
            "-c".to_owned(),
            "/tmp/unrelated-name.conf".to_owned(),
        ];
        assert!(command_line_has_service_signature(
            ManagementService::WpaSupplicant,
            &shell_owned
        ));
        assert!(!command_line_has_service_signature(
            ManagementService::Hostapd,
            &shell_owned
        ));
        assert!(command_line_has_service_signature(
            ManagementService::Dnsmasq,
            &[
                "/usr/sbin/dnsmasq".to_owned(),
                "--conf-file=/tmp/other".to_owned()
            ]
        ));
    }

    #[test]
    fn busybox_cardinality_counts_only_the_udhcpc_applet_signature() {
        assert!(!multicall_process_name_matches(
            ManagementService::Udhcpc,
            "sh"
        ));
        assert!(multicall_process_name_matches(
            ManagementService::Udhcpc,
            "udhcpc"
        ));
        assert!(multicall_process_name_matches(
            ManagementService::Hostapd,
            "hostapd"
        ));
        assert!(!executable_instance_matches_service(
            ManagementService::Udhcpc,
            &["/bin/sh".to_owned(), "-c".to_owned(), "sleep 1".to_owned()]
        ));
        assert!(executable_instance_matches_service(
            ManagementService::Udhcpc,
            &[
                "/sbin/udhcpc".to_owned(),
                "-f".to_owned(),
                "-i".to_owned(),
                "wlan0".to_owned(),
            ]
        ));
        assert!(executable_instance_matches_service(
            ManagementService::Hostapd,
            &["unexpected-argv0".to_owned()]
        ));
    }

    #[test]
    fn management_cardinality_requires_exactly_one_instance() {
        assert!(!has_exact_management_cardinality(0));
        assert!(has_exact_management_cardinality(1));
        assert!(!has_exact_management_cardinality(2));
        assert!(!has_exact_management_cardinality(usize::MAX));
    }

    #[test]
    fn dhcp_ownership_round_trips_the_exact_installed_generation() {
        let lease = crate::application::dhcp::DhcpLease {
            address: "192.0.2.5".parse().unwrap(),
            prefix: 24,
            broadcast: Some("192.0.2.255".parse().unwrap()),
            routers: vec!["192.0.2.1".parse().unwrap()],
            static_routes: Vec::new(),
            dns: vec!["1.1.1.1".parse().unwrap()],
            search: vec!["example.test".to_owned()],
        };
        let ownership = DhcpOwnership::from_lease(&lease);
        let encoded = serde_json::to_string(&ownership).unwrap();
        let decoded: DhcpOwnership = serde_json::from_str(&encoded).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded, ownership);
        assert_eq!(
            address_args("del", &ownership.address),
            ["-4", "address", "del", "192.0.2.5/24", "dev", "wlan0"].map(str::to_owned)
        );
        assert_eq!(
            route_args("del", &ownership.routes[0]),
            [
                "-4",
                "route",
                "del",
                "default",
                "via",
                "192.0.2.1",
                "dev",
                "wlan0",
                "metric",
                "600",
            ]
            .map(str::to_owned)
        );
    }

    #[test]
    fn resolver_cleanup_removes_only_recorded_entries_with_cardinality() {
        let owned = vec!["nameserver 1.1.1.1 # wlan0".to_owned()];
        let existing =
            "nameserver 1.1.1.1 # wlan0\nnameserver 1.1.1.1 # wlan0\nnameserver 9.9.9.9 # wlan0\n";
        assert_eq!(
            replace_resolver_text(existing, &owned, &[]),
            "nameserver 1.1.1.1 # wlan0\nnameserver 9.9.9.9 # wlan0\n"
        );
    }

    #[test]
    fn wan_gate_requires_the_exact_owned_route_shape() {
        let route = OwnedRoute {
            destination: "default".to_owned(),
            gateway: "192.0.2.1".parse().unwrap(),
            metric: Some(600),
        };
        assert!(address_output_contains(
            "3: wlan0 inet 192.0.2.5/24 brd 192.0.2.255 scope global wlan0",
            "192.0.2.5/24"
        ));
        assert!(route_line_uses_interface(
            "default via 192.0.2.1 dev wlan0 metric 600",
            "wlan0"
        ));
        assert!(!route_line_uses_interface(
            "default via 192.0.2.1 dev eth0 metric 600",
            "wlan0"
        ));
        assert!(!route_line_uses_interface(
            "default via 192.0.2.1 metric 600",
            "wlan0"
        ));
        assert!(exact_default_route_line(
            "default via 192.0.2.1 dev wlan0 metric 600",
            &route
        ));
        assert!(!exact_default_route_line(
            "default via 192.0.2.1 dev wlan0 proto dhcp metric 600",
            &route
        ));
    }
}
