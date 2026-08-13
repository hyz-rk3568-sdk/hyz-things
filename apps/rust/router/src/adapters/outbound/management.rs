use super::{
    network_config::{
        render_hostapd_on_channel, render_wpa_supplicant, ApRadioChannel, NetworkConfigStore,
    },
    process::process_start_time,
    storage,
};
use crate::{
    application::{
        dhcp::{DhcpEvent, DhcpPlatformPort, DHCP_HOOK_ROLE_ENV},
        ports::{ClockPort, PlatformError},
        wifi::{WifiPlatformPort, WifiScanEntry},
    },
    domain::{
        network::{LAN_BRIDGE, LAN_MEMBER, WAN_INTERFACE},
        network_config::{
            ApConfig, NetworkConfigSummary, NetworkConfigV1, PendingNetworkConfigSummary,
            PendingNetworkConfigV1, StaConfig, WifiCountry, WifiSsid,
        },
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    net::Ipv4Addr,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub const WPA_RUNTIME_CONFIG: &str = "/run/hyz-router/wpa_supplicant.rust.conf";
pub const HOSTAPD_RUNTIME_CONFIG: &str = "/run/hyz-router/hostapd.rust.conf";
pub const DNSMASQ_RUNTIME_CONFIG: &str = "/run/hyz-router/dnsmasq.rust.conf";
pub const ROUTER_EXECUTABLE: &str = "/usr/bin/hyz-router";
const RESOLV_CONFIG: &str = "/etc/resolv.conf";
const DHCP_OWNERSHIP_RECORD: &str = "/run/hyz-router/udhcpc.lease-generation.json";
const AP_PENDING_APPLIED_RECORD: &str = "/run/hyz-router/ap-pending-applied-v1";
const PROCESS_WAIT: Duration = Duration::from_secs(5);
const STA_CHANNEL_WAIT: Duration = Duration::from_secs(45);
const AP_READY_WAIT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const MAX_RESOLV_SIZE: usize = 64 * 1024;

pub const DNSMASQ_CONFIG: &str = "# DHCP and DNS proxy for the isolated br-lan LAN.\n\
interface=br-lan\n\
bind-interfaces\n\
listen-address=192.168.8.1\n\
except-interface=lo\n\
domain-needed\n\
bogus-priv\n\
dhcp-authoritative\n\
dhcp-range=192.168.8.100,192.168.8.249,255.255.255.0,10m\n\
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
            Self::WpaSupplicant => &[
                "-i",
                WAN_INTERFACE,
                "-D",
                "nl80211,wext",
                "-c",
                WPA_RUNTIME_CONFIG,
            ],
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
        self.restart_management_services(&committed_network_config()?, false)
    }

    fn restart_management_services(
        &self,
        config: &NetworkConfigV1,
        attach_after_start: bool,
    ) -> Result<(), PlatformError> {
        wait_for_interface_presence(WAN_INTERFACE, Duration::from_secs(20))?;
        wait_for_interface_presence(LAN_MEMBER, Duration::from_secs(20))?;
        let restore_attachment = attach_after_start || ap_attached_to_lan()?;
        // RTL8852BS can reject beacon programming when p2p0 remains enslaved while hostapd is
        // recreated. Detach before every transaction and attach only after AP readiness. A failed
        // candidate therefore leaves a clean detached interface for the committed rollback.
        self.detach_ap()?;
        self.stop_owned_management_services()?;
        self.refuse_foreign_management_processes()?;
        prepare_runtime_configs(config, ApRadioChannel::DEFAULT)?;

        let mut started = Vec::new();
        let result = (|| {
            run_ip(&["link", "set", "dev", WAN_INTERFACE, "up"])?;
            self.start_service(ManagementService::WpaSupplicant)?;
            started.push(ManagementService::WpaSupplicant);

            // udhcpc keeps retrying while the STA is disconnected. LAN management must come up
            // even when no upstream AP or DHCP lease is currently available.
            self.start_service(ManagementService::Udhcpc)?;
            started.push(ManagementService::Udhcpc);

            // RTL8852BS concurrent mode shares one radio channel. Give an available committed STA
            // a bounded association window before AP startup, but keep LAN fallback independent
            // from DHCP/default-route readiness.
            let channel = self
                .wait_for_sta_channel(STA_CHANNEL_WAIT)?
                .unwrap_or(ApRadioChannel::DEFAULT);
            self.start_hostapd_on_channel(&config.ap, channel)?;
            started.push(ManagementService::Hostapd);

            self.start_service(ManagementService::Dnsmasq)?;
            started.push(ManagementService::Dnsmasq);
            self.wait_for_identity(ManagementService::Dnsmasq, PROCESS_WAIT)?;
            if restore_attachment {
                self.attach_ap()?;
            }

            if !self.management_services_ready()? {
                return Err(PlatformError::ProbeFailed(
                    "management services did not reach ready state".to_owned(),
                ));
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(()),
            Err(primary) => {
                // This action is not recorded by the outer reconciler when it fails, so it must
                // complete its own compensation. Detach before stopping hostapd and retain every
                // exact identity record whose process cannot be stopped.
                let mut cleanup_errors = Vec::new();
                if let Err(error) = self.detach_ap() {
                    cleanup_errors.push(format!("detach AP: {error}"));
                }
                for service in started.into_iter().rev() {
                    if let Err(error) = self.stop_service(service) {
                        cleanup_errors.push(format!("stop {}: {error}", service.label()));
                    }
                }
                if cleanup_errors.is_empty() {
                    Err(primary)
                } else {
                    Err(PlatformError::InvalidState(format!(
                        "management restart failed: {primary}; exact cleanup also failed: {}",
                        cleanup_errors.join("; ")
                    )))
                }
            }
        }
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

    fn wait_for_hostapd(
        &self,
        channel: ApRadioChannel,
        timeout: Duration,
    ) -> Result<(), PlatformError> {
        wait_until(
            timeout,
            || self.hostapd_enabled_on_channel(channel),
            "AP enablement with the exact radio profile on the associated STA channel",
        )
    }

    fn start_hostapd_on_channel(
        &self,
        config: &ApConfig,
        channel: ApRadioChannel,
    ) -> Result<(), PlatformError> {
        apply_wifi_country(config.country)?;
        let mut first_error = None;
        for attempt in 0..2 {
            self.stop_service(ManagementService::Hostapd)?;
            run_ip(&["link", "set", "dev", LAN_MEMBER, "down"])?;
            let hostapd = render_hostapd_on_channel(config, channel);
            storage::atomic_write_private(HOSTAPD_RUNTIME_CONFIG, hostapd.as_bytes())?;
            run_ip(&["link", "set", "dev", LAN_MEMBER, "up"])?;
            let result = self
                .start_service(ManagementService::Hostapd)
                .and_then(|()| self.wait_for_hostapd(channel, AP_READY_WAIT));
            match result {
                Ok(()) => return Ok(()),
                Err(error) => {
                    if let Err(cleanup) = self.stop_service(ManagementService::Hostapd) {
                        return Err(PlatformError::InvalidState(format!(
                            "hostapd attempt failed: {error}; exact process cleanup also failed: {cleanup}"
                        )));
                    }
                    if attempt == 0 {
                        first_error = Some(error);
                    } else {
                        return Err(PlatformError::ProbeFailed(format!(
                            "hostapd failed after one clean retry: first={}; second={error}",
                            first_error.as_ref().expect("first hostapd error recorded")
                        )));
                    }
                }
            }
        }
        unreachable!("fixed hostapd retry loop returns on every second attempt")
    }

    fn wait_for_sta_channel(
        &self,
        timeout: Duration,
    ) -> Result<Option<ApRadioChannel>, PlatformError> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.current_sta_channel() {
                Ok(Some(channel)) => return Ok(Some(channel)),
                Ok(None) => {}
                Err(error) if is_management_probe_timeout(&error) => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_secs(1));
        }
    }

    fn current_sta_channel(&self) -> Result<Option<ApRadioChannel>, PlatformError> {
        let status =
            self.run_management_probe("/usr/sbin/wpa_cli", &["-i", WAN_INTERFACE, "status"])?;
        let frequency = status.lines().find_map(|line| {
            line.strip_prefix("freq=")
                .and_then(|value| value.parse::<u16>().ok())
        });
        frequency
            .map(|frequency| {
                frequency_to_ap_channel(frequency).ok_or_else(|| {
                    PlatformError::InvalidState(format!(
                        "associated STA frequency {frequency} MHz is not a supported non-DFS AP channel"
                    ))
                })
            })
            .transpose()
    }

    fn sta_associated_with(
        &self,
        candidate: &StaConfig,
        expected_channel: ApRadioChannel,
    ) -> Result<bool, PlatformError> {
        let status =
            self.run_management_probe("/usr/sbin/wpa_cli", &["-i", WAN_INTERFACE, "status"])?;
        Ok(sta_status_ready(
            &status,
            candidate.ssid.as_str(),
            expected_channel,
        ))
    }

    fn follow_sta_channel(
        &self,
        config: &NetworkConfigV1,
    ) -> Result<ApRadioChannel, PlatformError> {
        let channel = self.current_sta_channel()?.ok_or_else(|| {
            PlatformError::ProbeFailed(
                "ready STA did not report an associated frequency".to_owned(),
            )
        })?;
        self.detach_ap()?;
        self.start_hostapd_on_channel(&config.ap, channel)?;
        self.attach_ap()?;
        Ok(channel)
    }

    pub(crate) fn wait_for_sta_route(&self, timeout: Duration) -> Result<(), PlatformError> {
        wait_until(
            timeout,
            || self.owned_sta_address_and_route_ready(),
            "recorded DHCP STA address and exact owned default route with metric 600",
        )
    }

    fn hostapd_enabled(&self) -> Result<bool, PlatformError> {
        let output = match self
            .run_management_probe("/usr/bin/hostapd_cli", &["-i", LAN_MEMBER, "status"])
        {
            Ok(output) => output,
            // hostapd_cli can block while the driver is applying its regulatory/channel update.
            // The outer AP readiness deadline remains authoritative; one bounded probe timeout is
            // transient "not enabled yet", not proof that management startup is unsafe.
            Err(error) if is_management_probe_timeout(&error) => return Ok(false),
            Err(error) => return Err(error),
        };
        Ok(hostapd_status_ready(&output, None))
    }

    fn hostapd_enabled_on_channel(&self, channel: ApRadioChannel) -> Result<bool, PlatformError> {
        let output = match self
            .run_management_probe("/usr/bin/hostapd_cli", &["-i", LAN_MEMBER, "status"])
        {
            Ok(output) => output,
            Err(error) if is_management_probe_timeout(&error) => return Ok(false),
            Err(error) => return Err(error),
        };
        Ok(hostapd_status_ready(&output, Some(channel)))
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

impl WifiPlatformPort for super::process::LinuxRouterPlatform {
    fn recover_interrupted_ap_transaction(&self) -> Result<(), PlatformError> {
        let store = NetworkConfigStore::default();
        if let Some(committed) = store.read_sta_rollback().map_err(network_config_error)? {
            // A durable pre-transaction snapshot is authoritative until candidate persistence and
            // journal removal both complete. Startup restores it before recreating any runtime
            // service, resolving crashes and ambiguous rename/fsync outcomes fail-closed.
            persist_network_config_verified(&store, &committed)?;
            store.remove_sta_rollback().map_err(network_config_error)?;
        }
        if store
            .read_pending()
            .map_err(network_config_error)?
            .is_some()
        {
            // Runtime files and processes are recreated from committed state during startup. A
            // durable pending record can therefore never become committed merely by a restart.
            committed_network_config()?;
            store.remove_pending().map_err(network_config_error)?;
        }
        storage::remove_file_durable(AP_PENDING_APPLIED_RECORD)
    }

    fn scan(&self) -> Result<Vec<WifiScanEntry>, PlatformError> {
        let response =
            self.run_management_probe("/usr/sbin/wpa_cli", &["-i", WAN_INTERFACE, "scan"])?;
        if !response.lines().any(|line| line == "OK") {
            return Err(PlatformError::CommandFailed(
                "wpa_cli rejected the Wi-Fi scan".to_owned(),
            ));
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let output = self.run_management_probe(
                "/usr/sbin/wpa_cli",
                &["-i", WAN_INTERFACE, "scan_results"],
            )?;
            let entries = parse_scan_results(&output)?;
            if !entries.is_empty() || Instant::now() >= deadline {
                return Ok(entries);
            }
            thread::sleep(Duration::from_millis(250));
        }
    }

    fn committed_config(&self) -> Result<NetworkConfigSummary, PlatformError> {
        Ok(committed_network_config()?.summary())
    }

    fn begin_sta_candidate(&self) -> Result<NetworkConfigV1, PlatformError> {
        let store = NetworkConfigStore::default();
        if store
            .read_sta_rollback()
            .map_err(network_config_error)?
            .is_some()
        {
            return Err(PlatformError::Conflict(
                "an interrupted STA transaction requires recovery".to_owned(),
            ));
        }
        if store
            .read_pending()
            .map_err(network_config_error)?
            .is_some()
        {
            return Err(PlatformError::Conflict(
                "an AP transaction is already pending".to_owned(),
            ));
        }
        let committed = committed_network_config()?;
        store
            .persist_sta_rollback(&committed)
            .map_err(network_config_error)?;
        Ok(committed)
    }

    fn apply_sta_candidate(&self, candidate: &StaConfig) -> Result<(), PlatformError> {
        let mut config = committed_network_config()?;
        config.sta = candidate.clone();
        self.restart_management_services(&config, true)
    }

    fn sta_candidate_ready(&self, candidate: &StaConfig) -> Result<bool, PlatformError> {
        if self.wait_for_sta_route(Duration::from_secs(30)).is_err() {
            return Ok(false);
        }
        let mut config = committed_network_config()?;
        config.sta = candidate.clone();
        let channel = self.follow_sta_channel(&config)?;
        // Require two coherent post-restart observations so a driver disconnect cannot be hidden
        // briefly by a stale DHCP address/default route while udhcpc processes deconfiguration.
        for observation in 0..2 {
            if !self.sta_associated_with(candidate, channel)?
                || !self.hostapd_enabled_on_channel(channel)?
                || !self.owned_sta_address_and_route_ready()?
            {
                return Ok(false);
            }
            if observation == 0 {
                thread::sleep(Duration::from_millis(500));
            }
        }
        Ok(true)
    }

    fn commit_sta_candidate(
        &self,
        candidate: StaConfig,
    ) -> Result<NetworkConfigSummary, PlatformError> {
        let store = NetworkConfigStore::default();
        let committed = store
            .read_sta_rollback()
            .map_err(network_config_error)?
            .ok_or_else(|| {
                PlatformError::InvalidState("STA rollback journal is absent".to_owned())
            })?;
        let mut config = committed.clone();
        config.sta = candidate;
        persist_network_config_verified(&store, &config)?;
        if let Err(removal) = store.remove_sta_rollback().map_err(network_config_error) {
            // An unlink followed by a failed directory fsync is ambiguous. Recreate the old
            // journal and restore the canonical file before reporting failure; the application
            // still owns the in-memory snapshot and will restore runtime services next.
            let journal = store
                .persist_sta_rollback(&committed)
                .map_err(network_config_error);
            let persistence = persist_network_config_verified(&store, &committed);
            return match (journal, persistence) {
                (Ok(()), Ok(())) => Err(removal),
                (journal, persistence) => Err(PlatformError::InvalidState(format!(
                    "STA commit journal removal failed: {removal}; journal restoration={journal:?}; canonical restoration={persistence:?}"
                ))),
            };
        }
        Ok(config.summary())
    }

    fn rollback_sta_candidate(&self, committed: &NetworkConfigV1) -> Result<(), PlatformError> {
        let store = NetworkConfigStore::default();
        let persistence = persist_network_config_verified(&store, committed);
        let runtime = self
            .restart_management_services(committed, true)
            .and_then(|()| self.wait_for_sta_route(Duration::from_secs(30)))
            .and_then(|()| {
                if self.management_services_ready()? && self.owned_sta_address_and_route_ready()? {
                    Ok(())
                } else {
                    Err(PlatformError::UnsafeToCutOver(
                        "committed STA/AP restoration did not pass strict readiness".to_owned(),
                    ))
                }
            });
        match (persistence, runtime) {
            (Ok(()), Ok(())) => store
                .remove_sta_rollback()
                .map_err(network_config_error),
            (Err(persistence), Ok(())) => Err(persistence),
            (Ok(()), Err(runtime)) => Err(runtime),
            (Err(persistence), Err(runtime)) => Err(PlatformError::InvalidState(format!(
                "committed persistence restoration failed: {persistence}; runtime restoration also failed: {runtime}"
            ))),
        }
    }

    fn prepare_ap_candidate(
        &self,
        candidate: ApConfig,
        staged_at_unix_ms: u64,
    ) -> Result<PendingNetworkConfigSummary, PlatformError> {
        let store = NetworkConfigStore::default();
        refuse_outstanding_sta_transaction(&store)?;
        if store
            .read_pending()
            .map_err(network_config_error)?
            .is_some()
        {
            return Err(PlatformError::Conflict(
                "an AP transaction is already pending".to_owned(),
            ));
        }
        let mut config = committed_network_config()?;
        config.ap = candidate;
        let pending = PendingNetworkConfigV1::new(staged_at_unix_ms, config);
        store
            .persist_pending(&pending)
            .map_err(network_config_error)?;
        storage::remove_file_durable(AP_PENDING_APPLIED_RECORD)?;
        Ok(pending.summary())
    }

    fn apply_ap_candidate(
        &self,
        _applied_at_unix_ms: u64,
    ) -> Result<PendingNetworkConfigSummary, PlatformError> {
        let store = NetworkConfigStore::default();
        refuse_outstanding_sta_transaction(&store)?;
        if storage::read_small_optional(AP_PENDING_APPLIED_RECORD, 64)?.is_some() {
            return Err(PlatformError::Conflict(
                "pending AP candidate has already been applied".to_owned(),
            ));
        }
        let prepared = store
            .read_pending()
            .map_err(network_config_error)?
            .ok_or_else(|| {
                PlatformError::InvalidState("no AP transaction is pending".to_owned())
            })?;
        if let Err(error) = self.restart_management_services(&prepared.config, true) {
            let rollback = self.restart_management_services(&committed_network_config()?, true);
            if let Err(rollback) = rollback {
                return Err(PlatformError::InvalidState(format!(
                    "AP apply failed: {error}; committed rollback also failed: {rollback}"
                )));
            }
            return Err(error);
        }
        // The confirmation window starts only after hostapd and the management LAN are ready.
        let pending = PendingNetworkConfigV1::new(self.unix_time_millis(), prepared.config);
        if let Err(error) = store
            .persist_pending(&pending)
            .map_err(network_config_error)
        {
            let rollback = self.restart_management_services(&committed_network_config()?, true);
            if let Err(rollback) = rollback {
                return Err(PlatformError::InvalidState(format!(
                    "recording AP readiness failed: {error}; committed rollback also failed: {rollback}"
                )));
            }
            return Err(error);
        }
        if let Err(error) = storage::atomic_write_private(AP_PENDING_APPLIED_RECORD, b"applied\n") {
            let rollback = self.restart_management_services(&committed_network_config()?, true);
            if let Err(rollback) = rollback {
                return Err(PlatformError::InvalidState(format!(
                    "recording applied AP candidate failed: {error}; committed rollback also failed: {rollback}"
                )));
            }
            return Err(error);
        }
        Ok(pending.summary())
    }

    fn confirm_ap_candidate(&self) -> Result<NetworkConfigSummary, PlatformError> {
        if storage::read_small_optional(AP_PENDING_APPLIED_RECORD, 64)?.as_deref()
            != Some("applied\n")
        {
            return Err(PlatformError::InvalidState(
                "AP candidate has not been successfully applied".to_owned(),
            ));
        }
        let store = NetworkConfigStore::default();
        refuse_outstanding_sta_transaction(&store)?;
        let pending = store
            .read_pending()
            .map_err(network_config_error)?
            .ok_or_else(|| {
                PlatformError::InvalidState("no AP transaction is pending".to_owned())
            })?;
        // Canonical persistence is the commit point and is never reached before apply readiness.
        if let Err(error) = store.persist(&pending.config).map_err(network_config_error) {
            // A failed parent-directory fsync occurs after rename. Treat an exact read-back as
            // committed so timeout/cancel cannot incorrectly restore the already-replaced state.
            if store.read().map_err(network_config_error)?.as_ref() != Some(&pending.config) {
                return Err(error);
            }
        }
        store.remove_pending().map_err(network_config_error)?;
        storage::remove_file_durable(AP_PENDING_APPLIED_RECORD)?;
        Ok(pending.config.summary())
    }

    fn cancel_ap_candidate(&self) -> Result<NetworkConfigSummary, PlatformError> {
        let store = NetworkConfigStore::default();
        refuse_outstanding_sta_transaction(&store)?;
        let committed = committed_network_config()?;
        if store
            .read_pending()
            .map_err(network_config_error)?
            .is_none()
        {
            return Err(PlatformError::InvalidState(
                "no AP transaction is pending".to_owned(),
            ));
        }
        self.restart_management_services(&committed, true)?;
        store.remove_pending().map_err(network_config_error)?;
        storage::remove_file_durable(AP_PENDING_APPLIED_RECORD)?;
        Ok(committed.summary())
    }

    fn pending_ap_candidate(&self) -> Result<Option<PendingNetworkConfigSummary>, PlatformError> {
        Ok(NetworkConfigStore::default()
            .read_pending()
            .map_err(network_config_error)?
            .map(|pending| pending.summary()))
    }

    fn ap_candidate_applied(&self) -> Result<bool, PlatformError> {
        Ok(
            storage::read_small_optional(AP_PENDING_APPLIED_RECORD, 64)?.as_deref()
                == Some("applied\n"),
        )
    }
}

fn parse_scan_results(output: &str) -> Result<Vec<WifiScanEntry>, PlatformError> {
    let mut entries = Vec::new();
    for line in output.lines().skip(1) {
        let mut fields = line.splitn(5, '\t');
        let (Some(bssid), Some(frequency), Some(signal), Some(flags), Some(ssid)) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            continue;
        };
        let Ok(ssid) = WifiSsid::new(ssid) else {
            continue;
        };
        if bssid.len() != 17
            || !bssid.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 2 | 5 | 8 | 11 | 14) {
                    byte == b':'
                } else {
                    byte.is_ascii_hexdigit()
                }
            })
        {
            continue;
        }
        let frequency_mhz = frequency.parse::<u16>().map_err(|_| {
            PlatformError::ProbeFailed("wpa_cli scan frequency is invalid".to_owned())
        })?;
        let signal_dbm = signal
            .parse::<i16>()
            .map_err(|_| PlatformError::ProbeFailed("wpa_cli scan signal is invalid".to_owned()))?;
        entries.push(WifiScanEntry {
            ssid,
            bssid: bssid.to_owned(),
            frequency_mhz,
            signal_dbm,
            secured: flags.contains("WPA"),
        });
    }
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.signal_dbm));
    entries.dedup_by(|left, right| left.ssid == right.ssid && left.bssid == right.bssid);
    Ok(entries)
}

fn apply_wifi_country(country: WifiCountry) -> Result<(), PlatformError> {
    run_bounded(
        "/usr/sbin/iw",
        &["reg", "set", country.as_str()],
        Duration::from_secs(3),
    )?;
    let output = run_bounded("/usr/sbin/iw", &["reg", "get"], Duration::from_secs(3))?;
    let observed = self_managed_country(&output).ok_or_else(|| {
        PlatformError::ProbeFailed(
            "iw reg get did not report exactly one self-managed phy#0 country".to_owned(),
        )
    })?;
    if observed == country.as_str() || observed == "00" {
        Ok(())
    } else {
        Err(PlatformError::Conflict(format!(
            "self-managed phy retained foreign country {observed} after requesting {}",
            country.as_str()
        )))
    }
}

fn self_managed_country(output: &str) -> Option<&str> {
    let mut matches = output
        .lines()
        .enumerate()
        .filter_map(|(index, line)| (line.trim() == "phy#0 (self-managed)").then_some(index));
    let index = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let country_line = output
        .lines()
        .skip(index + 1)
        .find(|line| !line.trim().is_empty())?;
    let country = country_line
        .trim()
        .strip_prefix("country ")?
        .split_once(':')?
        .0;
    (country.len() == 2
        && country
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()))
    .then_some(country)
}

fn prepare_runtime_configs(
    config: &NetworkConfigV1,
    channel: ApRadioChannel,
) -> Result<(), PlatformError> {
    let wpa = render_wpa_supplicant(&config.sta);
    let hostapd = render_hostapd_on_channel(&config.ap, channel);
    storage::atomic_write_private(WPA_RUNTIME_CONFIG, wpa.as_bytes())?;
    storage::atomic_write_private(HOSTAPD_RUNTIME_CONFIG, hostapd.as_bytes())?;
    storage::atomic_write_private(DNSMASQ_RUNTIME_CONFIG, DNSMASQ_CONFIG.as_bytes())
}

fn committed_network_config() -> Result<NetworkConfigV1, PlatformError> {
    NetworkConfigStore::default()
        .read_or_migrate()
        .map_err(network_config_error)?
        .ok_or_else(|| {
            PlatformError::InvalidState(
                "canonical network-config-v1 is absent and legacy migration was unavailable"
                    .to_owned(),
            )
        })
}

fn refuse_outstanding_sta_transaction(store: &NetworkConfigStore) -> Result<(), PlatformError> {
    if store
        .read_sta_rollback()
        .map_err(network_config_error)?
        .is_some()
    {
        return Err(PlatformError::Conflict(
            "an interrupted STA transaction requires recovery before AP mutation".to_owned(),
        ));
    }
    Ok(())
}

fn persist_network_config_verified(
    store: &NetworkConfigStore,
    config: &NetworkConfigV1,
) -> Result<(), PlatformError> {
    let Err(write_error) = store.persist(config).map_err(network_config_error) else {
        return Ok(());
    };
    match store.read().map_err(network_config_error) {
        Ok(Some(actual)) if actual == *config => Ok(()),
        Ok(_) => Err(write_error),
        Err(read_error) => Err(PlatformError::InvalidState(format!(
            "network config persistence was ambiguous: {write_error}; read-back also failed: {read_error}"
        ))),
    }
}

fn network_config_error(error: super::network_config::NetworkConfigError) -> PlatformError {
    PlatformError::InvalidState(error.to_string())
}

fn frequency_to_ap_channel(frequency_mhz: u16) -> Option<ApRadioChannel> {
    match frequency_mhz {
        2_412..=2_472 if (frequency_mhz - 2_407) % 5 == 0 => {
            ApRadioChannel::ghz2(((frequency_mhz - 2_407) / 5) as u8)
        }
        2_484 => ApRadioChannel::ghz2(14),
        5_180..=5_240 | 5_745..=5_825 if (frequency_mhz - 5_000) % 5 == 0 => {
            ApRadioChannel::ghz5(((frequency_mhz - 5_000) / 5) as u8)
        }
        _ => None,
    }
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

fn ap_attached_to_lan() -> Result<bool, PlatformError> {
    let master = Path::new("/sys/class/net").join(LAN_MEMBER).join("master");
    match fs::read_link(&master) {
        Ok(target) if target.file_name().and_then(|name| name.to_str()) == Some(LAN_BRIDGE) => {
            Ok(true)
        }
        Ok(target) => Err(PlatformError::Conflict(format!(
            "managed AP interface has foreign master {}",
            target.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(PlatformError::ProbeFailed(format!(
            "inspect managed AP bridge attachment: {error}"
        ))),
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

fn sta_status_ready(output: &str, expected_ssid: &str, expected_channel: ApRadioChannel) -> bool {
    let expected_ssid = format!("ssid={expected_ssid}");
    let completed = output.lines().any(|line| line == "wpa_state=COMPLETED");
    let matching_ssid = output.lines().any(|line| line == expected_ssid);
    let matching_channel = output.lines().any(|line| {
        line.strip_prefix("freq=")
            .and_then(|value| value.parse::<u16>().ok())
            .and_then(frequency_to_ap_channel)
            == Some(expected_channel)
    });
    completed && matching_ssid && matching_channel
}

fn hostapd_status_ready(output: &str, expected_channel: Option<ApRadioChannel>) -> bool {
    if unique_status_value(output, "state") != Some("ENABLED") {
        return false;
    }
    let Some(channel) = expected_channel else {
        return true;
    };
    if unique_status_value(output, "channel").and_then(|value| value.parse::<u8>().ok())
        != Some(channel.number())
        || unique_status_value(output, "secondary_channel")
            .and_then(|value| value.parse::<i8>().ok())
            != Some(channel.secondary_channel())
        || unique_status_value(output, "ieee80211n") != Some("1")
        || unique_status_value(output, "ieee80211ac")
            != Some(if channel.ieee80211ac() { "1" } else { "0" })
    {
        return false;
    }
    match channel.vht_geometry() {
        Some((width, center)) => {
            unique_status_value(output, "vht_oper_chwidth")
                .and_then(|value| value.parse::<u8>().ok())
                == Some(width)
                && unique_status_value(output, "vht_oper_centr_freq_seg0_idx")
                    .and_then(|value| value.parse::<u8>().ok())
                    == Some(center)
        }
        None => {
            unique_status_value(output, "vht_oper_chwidth").is_none()
                && unique_status_value(output, "vht_oper_centr_freq_seg0_idx").is_none()
        }
    }
}

fn unique_status_value<'a>(output: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}=");
    let mut values = output.lines().filter_map(|line| line.strip_prefix(&prefix));
    let value = values.next()?;
    if values.next().is_some() {
        None
    } else {
        Some(value)
    }
}

fn is_management_probe_timeout(error: &PlatformError) -> bool {
    matches!(
        error,
        PlatformError::ProbeFailed(detail)
            if detail == "management probe exceeded its deadline"
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
    fn ap_mutations_refuse_an_outstanding_sta_rollback_journal() {
        let source = include_str!("management.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert_eq!(
            source
                .matches("refuse_outstanding_sta_transaction(&store)?")
                .count(),
            4
        );
    }

    #[test]
    fn only_the_fixed_management_timeout_is_transient_during_readiness() {
        assert!(is_management_probe_timeout(&PlatformError::ProbeFailed(
            "management probe exceeded its deadline".to_owned()
        )));
        assert!(!is_management_probe_timeout(&PlatformError::ProbeFailed(
            "other probe failure".to_owned()
        )));
        assert!(!is_management_probe_timeout(&PlatformError::Conflict(
            "management probe exceeded its deadline".to_owned()
        )));
    }

    #[test]
    fn supported_ap_frequencies_map_to_typed_non_dfs_channels() {
        assert_eq!(frequency_to_ap_channel(2_437), ApRadioChannel::ghz2(6));
        assert_eq!(frequency_to_ap_channel(5_180), ApRadioChannel::ghz5(36));
        assert_eq!(frequency_to_ap_channel(5_805), ApRadioChannel::ghz5(161));
        assert_eq!(frequency_to_ap_channel(5_825), ApRadioChannel::ghz5(165));
        assert_eq!(frequency_to_ap_channel(5_260), None);
        assert_eq!(frequency_to_ap_channel(5_500), None);
        assert_eq!(frequency_to_ap_channel(5_845), None);
    }

    #[test]
    fn sta_status_requires_candidate_completed_on_expected_channel() {
        let channel_6 = ApRadioChannel::ghz2(6).unwrap();
        let channel_11 = ApRadioChannel::ghz2(11).unwrap();
        let channel_161 = ApRadioChannel::ghz5(161).unwrap();
        let ready = "ssid=candidate\nfreq=2437\nwpa_state=COMPLETED\n";
        assert!(sta_status_ready(ready, "candidate", channel_6));
        assert!(!sta_status_ready(ready, "other", channel_6));
        assert!(!sta_status_ready(ready, "candidate", channel_11));
        assert!(!sta_status_ready(
            "ssid=candidate\nfreq=2437\nwpa_state=DISCONNECTED\n",
            "candidate",
            channel_6
        ));
        assert!(!sta_status_ready(
            "ssid=candidate\nfreq=invalid\nwpa_state=COMPLETED\n",
            "candidate",
            channel_6
        ));
        assert!(sta_status_ready(
            "ssid=candidate\nfreq=5805\nwpa_state=COMPLETED\n",
            "candidate",
            channel_161
        ));
    }

    #[test]
    fn hostapd_status_requires_exact_ht_and_vht_radio_profile() {
        let channel_6 = ApRadioChannel::ghz2(6).unwrap();
        let channel_161 = ApRadioChannel::ghz5(161).unwrap();
        let ghz2 = "state=ENABLED\nchannel=6\nsecondary_channel=0\nieee80211n=1\nieee80211ac=0\n";
        let ghz5 = "state=ENABLED\nchannel=161\nsecondary_channel=-1\nieee80211n=1\nieee80211ac=1\nvht_oper_chwidth=1\nvht_oper_centr_freq_seg0_idx=155\n";
        assert!(hostapd_status_ready(ghz2, None));
        assert!(hostapd_status_ready(ghz2, Some(channel_6)));
        assert!(hostapd_status_ready(ghz5, Some(channel_161)));
        assert!(!hostapd_status_ready(
            &ghz2.replace("ieee80211n=1", "ieee80211n=0"),
            Some(channel_6)
        ));
        assert!(!hostapd_status_ready(
            &ghz5.replace("ieee80211ac=1", "ieee80211ac=0"),
            Some(channel_161)
        ));
        assert!(!hostapd_status_ready(
            &ghz5.replace(
                "vht_oper_centr_freq_seg0_idx=155",
                "vht_oper_centr_freq_seg0_idx=42"
            ),
            Some(channel_161)
        ));
        assert!(!hostapd_status_ready(
            &format!("{ghz5}channel=161\n"),
            Some(channel_161)
        ));
        assert!(!hostapd_status_ready(
            "state=DISABLED\nchannel=6\nsecondary_channel=0\nieee80211n=1\nieee80211ac=0\n",
            Some(channel_6)
        ));
    }

    #[test]
    fn country_request_accepts_exact_or_driver_world_readback_and_precedes_hostapd() {
        let exact = "global\ncountry 00: DFS-UNSET\n\nphy#0 (self-managed)\ncountry NZ: DFS-ETSI\n\t(2402 - 2482 @ 40), (N/A, 20), (N/A)\n";
        let driver_world = exact.replace("country NZ: DFS-ETSI", "country 00: DFS-UNSET");
        assert_eq!(self_managed_country(exact), Some("NZ"));
        assert_eq!(self_managed_country(&driver_world), Some("00"));
        assert_eq!(
            self_managed_country(&exact.replace("phy#0 (self-managed)", "phy#0")),
            None
        );
        assert_eq!(
            self_managed_country(&format!(
                "{exact}\nphy#0 (self-managed)\ncountry NZ: DFS-ETSI\n"
            )),
            None
        );

        let source = include_str!("management.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let helper = source
            .split_once("fn start_hostapd_on_channel")
            .unwrap()
            .1
            .split_once("fn wait_for_sta_channel")
            .unwrap()
            .0;
        assert!(helper.find("apply_wifi_country").unwrap() < helper.find("start_service").unwrap());
        assert!(source.contains("[\"reg\", \"set\", country.as_str()]"));
        assert!(source.contains("[\"reg\", \"get\"]"));
        assert!(!source.contains("set txpower"));
        assert!(!source.contains("rtw_tx_pwr_lmt_enable"));
    }

    #[test]
    fn management_restart_detaches_before_hostapd_and_reattaches_after_readiness() {
        let source = include_str!("management.rs");
        assert!(source
            .contains("self.restart_management_services(&committed_network_config()?, false)"));
        let body = source
            .split_once("fn restart_management_services")
            .unwrap()
            .1
            .split_once("pub(crate) fn management_services_ready")
            .unwrap()
            .0;
        let preserve = body.find("let restore_attachment").unwrap();
        let detach = body.find("self.detach_ap()?").unwrap();
        let stop = body.find("self.stop_owned_management_services()?").unwrap();
        let hostapd = body.find("self.start_hostapd_on_channel").unwrap();
        let attach = body.find("if restore_attachment").unwrap();
        assert!(preserve < detach && detach < stop && stop < hostapd && hostapd < attach);
        let cleanup = body.split_once("Err(primary) =>").unwrap().1;
        assert!(cleanup.find("self.detach_ap()").unwrap() < cleanup.find("for service").unwrap());

        let helper = source
            .split_once("fn start_hostapd_on_channel")
            .unwrap()
            .1
            .split_once("fn wait_for_sta_channel")
            .unwrap()
            .0;
        let start = helper
            .find(".start_service(ManagementService::Hostapd)")
            .unwrap();
        let ready = helper
            .find("self.wait_for_hostapd(channel, AP_READY_WAIT)")
            .unwrap();
        assert!(start < ready);
    }

    #[test]
    fn service_argv_are_exact_golden_values() {
        assert_eq!(
            ManagementService::WpaSupplicant.argv(),
            &[
                "-i",
                "wlan0",
                "-D",
                "nl80211,wext",
                "-c",
                WPA_RUNTIME_CONFIG,
            ]
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
    fn dnsmasq_dynamic_pool_has_the_exact_inclusive_boundaries() {
        let range = DNSMASQ_CONFIG
            .lines()
            .find_map(|line| line.strip_prefix("dhcp-range="))
            .expect("fixed DHCP range");
        let mut fields = range.split(',');
        let start = fields
            .next()
            .expect("range start")
            .parse::<Ipv4Addr>()
            .expect("IPv4 range start");
        let end = fields
            .next()
            .expect("range end")
            .parse::<Ipv4Addr>()
            .expect("IPv4 range end");
        assert_eq!(fields.next(), Some("255.255.255.0"));
        assert_eq!(fields.next(), Some("10m"));
        assert_eq!(fields.next(), None);

        let in_pool = |address: &str| {
            let address = address.parse::<Ipv4Addr>().expect("test IPv4 address");
            u32::from(start) <= u32::from(address) && u32::from(address) <= u32::from(end)
        };
        assert!(in_pool("192.168.8.100"));
        assert!(in_pool("192.168.8.249"));
        assert!(!in_pool("192.168.8.99"));
        assert!(!in_pool("192.168.8.250"));
    }

    #[test]
    fn dnsmasq_config_keeps_the_fixed_gateway_dns_and_lease_path() {
        assert!(DNSMASQ_CONFIG.contains("dhcp-option=3,192.168.8.1\n"));
        assert!(DNSMASQ_CONFIG.contains("dhcp-option=6,192.168.8.1\n"));
        assert!(DNSMASQ_CONFIG.contains("dhcp-leasefile=/run/hyz-router/dnsmasq.leases\n"));
    }

    #[test]
    fn runtime_config_prepare_does_not_delete_the_dnsmasq_lease_file() {
        let source = include_str!("management.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production management source");
        let prepare = source
            .split_once("fn prepare_runtime_configs(")
            .expect("runtime config prepare function")
            .1
            .split_once("fn committed_network_config(")
            .expect("end of runtime config prepare function")
            .0;
        assert!(prepare.contains("atomic_write_private(DNSMASQ_RUNTIME_CONFIG"));
        assert!(!prepare.contains("remove_file"));
        assert!(!prepare.contains("dnsmasq.leases"));
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
