use super::{
    network_config::{
        render_hostapd_on_channel, render_wpa_supplicant, ApRadioChannel, NetworkConfigStore,
    },
    process::process_start_time,
    storage,
};
use crate::{
    application::{
        dhcp::{
            DhcpEvent, DhcpGeneration, DhcpPlatformPort, DhcpTransition, DhcpUplink,
            DHCP_GENERATION_ENV, DHCP_HOOK_ROLE_ENV, DHCP_UPLINK_ENV,
        },
        ethernet_dhcp::EthernetDhcpLifecyclePort,
        ports::{ClockPort, PlatformError},
        wifi::{WifiPlatformPort, WifiScanEntry},
    },
    domain::{
        network::{
            OwnedResource, Probe, UplinkObserved, ETHERNET_WAN_INTERFACE, LAN_BRIDGE, LAN_MEMBER,
            WAN_INTERFACE,
        },
        network_config::{
            ApConfig, NetworkConfigSummary, NetworkConfigV1, PendingNetworkConfigSummary,
            PendingNetworkConfigV1, StaConfig, WifiCountry, WifiSsid,
        },
        wifi_startup::{
            startup_channel_plan, LastGoodStaChannel, StaFingerprint, StartupChannelPlan,
            LAST_GOOD_MAX_AGE_MS,
        },
    },
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    net::Ipv4Addr,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

pub const WPA_RUNTIME_CONFIG: &str = "/run/hyz-router/wpa_supplicant.rust.conf";
pub const HOSTAPD_RUNTIME_CONFIG: &str = "/run/hyz-router/hostapd.rust.conf";
pub const DNSMASQ_RUNTIME_CONFIG: &str = "/run/hyz-router/dnsmasq.rust.conf";
pub const ROUTER_EXECUTABLE: &str = "/usr/bin/hyz-router";
const RESOLV_CONFIG: &str = "/etc/resolv.conf";
const WIFI_DHCP_ACTIVE_GENERATION_RECORD: &str = "/run/hyz-router/udhcpc.active-generation";
const ETHERNET_DHCP_ACTIVE_GENERATION_RECORD: &str =
    "/run/hyz-router/eth0-udhcpc.active-generation";
const WIFI_DHCP_OWNERSHIP_RECORD: &str = "/run/hyz-router/udhcpc.lease-generation.json";
const ETHERNET_DHCP_OWNERSHIP_RECORD: &str = "/run/hyz-router/eth0-udhcpc.lease-generation.json";
const WIFI_DHCP_RESOLVER_RECORD: &str = "/run/hyz-router/udhcpc.resolver-generation.json";
const ETHERNET_DHCP_RESOLVER_RECORD: &str = "/run/hyz-router/eth0-udhcpc.resolver-generation.json";
const AP_PENDING_APPLIED_RECORD: &str = "/run/hyz-router/ap-pending-applied-v1";
const LAST_GOOD_CHANNEL_PATH: &str = "/userdata/hyz-router/sta-last-good-channel.json";
const MAX_LAST_GOOD_BYTES: usize = 4096;
const PROCESS_WAIT: Duration = Duration::from_secs(5);
const STA_CHANNEL_WAIT: Duration = Duration::from_secs(45);
/// Short bounded window used on the last-good fast path before programming hostapd. It is not a
/// full shared-channel wait: it only lets the single radio finish its first scan/association
/// (stability), and it lets a live STA channel override the recorded last-good one when present.
/// Without an upstream, the AP still starts on the recorded channel after this window.
const FAST_START_CONFIRM_WAIT: Duration = Duration::from_secs(15);
/// Bound for the exact AP readiness probe (state=ENABLED plus the exact VHT80 geometry) after
/// hostapd starts. On a cold RTL8852BS start the radio settles into the exact profile tens of
/// seconds after the shared-channel gate passes; the window must stay small enough that a failing
/// attempt leaves room for S81 to relaunch within the startup deadline.
const AP_READY_WAIT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Bounded window for the single radio to become operationally ready for the target AP channel
/// before hostapd is programmed. On a cold RTL8852BS start the radio is not ready until the STA
/// is confirmed on the shared channel; starting hostapd earlier races the driver and fails the
/// strict VHT80 readiness probe, burning the whole AP-readiness window per launch attempt.
const RADIO_CHANNEL_READY_WAIT: Duration = Duration::from_secs(45);
const MAX_RESOLV_SIZE: usize = 64 * 1024;
static DHCP_GENERATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static RESOLVER_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
    #[serde(default)]
    uplink: DhcpUplink,
    #[serde(default)]
    generation: Option<DhcpGeneration>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DhcpResolverRecord {
    version: u8,
    uplink: DhcpUplink,
    generation: DhcpGeneration,
    entries: Vec<String>,
}

impl DhcpResolverRecord {
    fn from_lease(
        uplink: DhcpUplink,
        generation: DhcpGeneration,
        lease: &crate::application::dhcp::DhcpLease,
    ) -> Self {
        Self {
            version: 1,
            uplink,
            generation,
            entries: lease.resolver_lines(dhcp_uplink_interface(uplink)),
        }
    }

    fn validate(&self) -> Result<(), PlatformError> {
        if self.version != 1
            || self.entries.len() > 32
            || self.entries.iter().any(|entry| {
                entry.contains('\n')
                    || !entry.ends_with(&format!("# {}", dhcp_uplink_interface(self.uplink)))
                    || !(entry.starts_with("search ") || entry.starts_with("nameserver "))
            })
        {
            return Err(invalid_dhcp_ownership());
        }
        Ok(())
    }
}

impl DhcpOwnership {
    fn from_lease(
        uplink: DhcpUplink,
        generation: DhcpGeneration,
        lease: &crate::application::dhcp::DhcpLease,
    ) -> Self {
        let routes = if lease.static_routes.is_empty() {
            lease
                .routers
                .iter()
                .map(|gateway| OwnedRoute {
                    destination: "default".to_owned(),
                    gateway: *gateway,
                    metric: Some(dhcp_uplink_metric(uplink)),
                })
                .collect()
        } else {
            lease
                .static_routes
                .iter()
                .map(|(destination, gateway)| OwnedRoute {
                    metric: matches!(destination.as_str(), "default" | "0.0.0.0/0")
                        .then_some(dhcp_uplink_metric(uplink)),
                    destination: destination.clone(),
                    gateway: *gateway,
                })
                .collect()
        };
        Self {
            version: 3,
            uplink,
            generation: Some(generation),
            address: OwnedAddress {
                cidr: format!("{}/{}", lease.address, lease.prefix),
                broadcast: lease.broadcast,
            },
            routes,
            resolver_entries: lease.resolver_lines(dhcp_uplink_interface(uplink)),
        }
    }

    fn validate(&self) -> Result<(), PlatformError> {
        if !matches!(
            (self.version, &self.generation),
            (1, None) | (2, Some(_)) | (3, Some(_))
        ) || self.routes.len() > 64
            || self.resolver_entries.len() > 32
        {
            return Err(invalid_dhcp_ownership());
        }
        validate_ipv4_cidr(&self.address.cidr).map_err(|_| invalid_dhcp_ownership())?;
        let mut route_destinations = Vec::new();
        for route in &self.routes {
            if route.destination != "default" && route.destination != "0.0.0.0/0" {
                validate_ipv4_cidr(&route.destination).map_err(|_| invalid_dhcp_ownership())?;
            }
            let destination = canonical_route_destination(&route.destination);
            if route_destinations.contains(&destination) {
                return Err(invalid_dhcp_ownership());
            }
            route_destinations.push(destination);
            if route.metric.is_some() && route.metric != Some(dhcp_uplink_metric(self.uplink)) {
                return Err(invalid_dhcp_ownership());
            }
            if matches!(route.destination.as_str(), "default" | "0.0.0.0/0")
                != (route.metric == Some(dhcp_uplink_metric(self.uplink)))
            {
                return Err(invalid_dhcp_ownership());
            }
        }
        if self.resolver_entries.iter().any(|entry| {
            entry.contains('\n')
                || !entry.ends_with(&format!("# {}", dhcp_uplink_interface(self.uplink)))
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
    EthernetUdhcpc,
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
            Self::Udhcpc | Self::EthernetUdhcpc => "/sbin/udhcpc",
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
            Self::EthernetUdhcpc => &["-f", "-i", ETHERNET_WAN_INTERFACE, "-s", ROUTER_EXECUTABLE],
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
            Self::EthernetUdhcpc => "/run/hyz-router/eth0-udhcpc.identity",
            Self::Hostapd => "/run/hyz-router/hostapd.identity",
            Self::Dnsmasq => "/run/hyz-router/dnsmasq.identity",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::WpaSupplicant => "wpa_supplicant",
            Self::Udhcpc => "udhcpc",
            Self::EthernetUdhcpc => "eth0-udhcpc",
            Self::Hostapd => "hostapd",
            Self::Dnsmasq => "dnsmasq",
        }
    }

    const fn process_name(self) -> &'static str {
        match self {
            Self::EthernetUdhcpc => "udhcpc",
            _ => self.label(),
        }
    }

    const fn dhcp_uplink(self) -> Option<DhcpUplink> {
        match self {
            Self::Udhcpc => Some(DhcpUplink::Wifi),
            Self::EthernetUdhcpc => Some(DhcpUplink::Ethernet),
            Self::WpaSupplicant | Self::Hostapd | Self::Dnsmasq => None,
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

        // Last-good fast-start applies only to cold-start management startup (`attach_after_start
        // == false`). STA apply and rollback are explicit configuration/recovery paths where the
        // bounded shared-channel wait is kept for correctness regardless of any cache.
        let fingerprint = StaFingerprint::of(&config.sta);
        let plan = if attach_after_start {
            StartupChannelPlan::WaitForStaChannel
        } else {
            startup_channel_plan(
                read_last_good_channel()?.as_ref(),
                &fingerprint,
                self.unix_time_millis(),
                LAST_GOOD_MAX_AGE_MS,
            )
        };

        let mut started = Vec::new();
        let result = (|| {
            run_ip(&["link", "set", "dev", WAN_INTERFACE, "up"])?;
            self.start_service(ManagementService::WpaSupplicant)?;
            started.push(ManagementService::WpaSupplicant);

            // udhcpc keeps retrying while the STA is disconnected. LAN management must come up
            // even when no upstream AP or DHCP lease is currently available.
            self.start_service(ManagementService::Udhcpc)?;
            started.push(ManagementService::Udhcpc);

            // RTL8852BS concurrent mode shares one radio channel. With a fresh last-good record
            // the AP startup still keeps a short radio-settling window before hostapd is
            // programmed: starting hostapd while the single radio is still scanning/associating
            // races the driver and flakes the strict AP readiness probe (observed as repeated
            // cold-start relaunches on the board). So on the fast path we wait a bounded window
            // for the committed STA's channel and use the live channel when it appears, falling
            // back to the recorded last-good one only when no upstream channel shows up. The
            // standard path keeps the full 45s association window before AP startup, with LAN
            // fallback independent from DHCP/default-route readiness.
            let channel = match plan {
                StartupChannelPlan::FastStart { channel } => {
                    match self.wait_for_sta_channel(FAST_START_CONFIRM_WAIT)? {
                        Some(live) => live,
                        None => {
                            ApRadioChannel::from_domain(channel).unwrap_or(ApRadioChannel::DEFAULT)
                        }
                    }
                }
                StartupChannelPlan::WaitForStaChannel => self
                    .wait_for_sta_channel(STA_CHANNEL_WAIT)?
                    .unwrap_or(ApRadioChannel::DEFAULT),
            };
            // Whatever the channel source (live STA, recorded last-good fallback, or the fixed AP
            // fallback), only program hostapd once the single radio is operationally on that
            // channel; otherwise the strict AP readiness probe burns the full window on a radio
            // that is still settling.
            self.wait_for_radio_channel_ready(config, channel, RADIO_CHANNEL_READY_WAIT)?;
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
            // Persist a fresh last-good record whenever the AP is up on a channel the STA is
            // actually sharing, so a later cold start can fast-start on it. This is a
            // runtime-derived cache and never affects readiness; both the probe and the write
            // degrade to "no record" on any failure.
            if !attach_after_start {
                match self.current_sta_channel() {
                    Ok(Some(channel)) => {
                        let record = LastGoodStaChannel {
                            fingerprint,
                            channel: channel.to_domain(),
                            recorded_unix_ms: self.unix_time_millis(),
                        };
                        if let Err(error) = write_last_good_channel(&record) {
                            eprintln!("hyz-router: persist last-good STA channel: {error}");
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        eprintln!("hyz-router: skip last-good STA channel recording: {error}");
                    }
                }
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

    /// Refresh the persisted last-good STA channel record once the STA is confirmed on the shared
    /// channel (e.g. after forwarding reconciliation confirms the DHCP-owned default route). A
    /// not-yet-associated STA leaves the record untouched; a missing committed config no-ops.
    /// Fail-soft: this is a cache and must never affect management correctness.
    pub fn refresh_last_good_sta_channel(&self) -> Result<(), PlatformError> {
        let Some(channel) = self.current_sta_channel()? else {
            return Ok(());
        };
        let config = committed_network_config()?;
        let record = LastGoodStaChannel {
            fingerprint: StaFingerprint::of(&config.sta),
            channel: channel.to_domain(),
            recorded_unix_ms: self.unix_time_millis(),
        };
        write_last_good_channel(&record)
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

    fn observe_ethernet_carrier(&self) -> Result<bool, PlatformError> {
        match fs::read_to_string(format!("/sys/class/net/{ETHERNET_WAN_INTERFACE}/carrier")) {
            Ok(carrier) => match carrier.trim() {
                "1" => Ok(true),
                "0" => Ok(false),
                _ => Err(PlatformError::ProbeFailed(
                    "eth0 carrier state is malformed".to_owned(),
                )),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(PlatformError::ProbeFailed(format!(
                "read eth0 carrier state: {error}"
            ))),
        }
    }

    fn ensure_ethernet_dhcp_process(&self) -> Result<(), PlatformError> {
        let service = ManagementService::EthernetUdhcpc;
        if let Some(identity) = self.read_service_identity(service)? {
            if !identity.matches(service)? {
                return Err(PlatformError::Conflict(
                    "eth0 udhcpc identity does not match its live process".to_owned(),
                ));
            }
            if service_executable_process_count(service)? != 1
                || foreign_candidate_exists(service, Some(identity.pid))?
            {
                return Err(PlatformError::Conflict(
                    "eth0 udhcpc process ownership is not exact and unique".to_owned(),
                ));
            }
            return Ok(());
        }
        if service_executable_process_count(service)? != 0
            || foreign_candidate_exists(service, None)?
        {
            return Err(PlatformError::Conflict(
                "refusing to take over an unowned eth0 udhcpc process".to_owned(),
            ));
        }
        self.start_service(service)
    }

    fn stop_ethernet_dhcp_process(&self) -> Result<(), PlatformError> {
        self.stop_service(ManagementService::EthernetUdhcpc)?;
        if let Some(generation) = read_active_dhcp_generation(DhcpUplink::Ethernet)? {
            retire_active_dhcp_generation(DhcpUplink::Ethernet, &generation)?;
        }
        Ok(())
    }

    fn start_service(&self, service: ManagementService) -> Result<(), PlatformError> {
        if self.read_service_identity(service)?.is_some() {
            return Err(PlatformError::Conflict(format!(
                "{} identity already exists",
                service.label()
            )));
        }
        let dhcp = service
            .dhcp_uplink()
            .map(|uplink| {
                if read_active_dhcp_generation(uplink)?.is_some() {
                    return Err(PlatformError::Conflict(format!(
                        "{} active DHCP generation already exists",
                        service.label()
                    )));
                }
                let generation = new_dhcp_generation(self.unix_time_millis())?;
                storage::atomic_write_private(
                    dhcp_active_generation_record(uplink),
                    format!("{}\n", generation.as_str()).as_bytes(),
                )?;
                Ok((uplink, generation))
            })
            .transpose()?;
        let mut command = Command::new(service.executable());
        command.args(service.argv()).env_clear().env("LC_ALL", "C");
        if let Some((uplink, generation)) = &dhcp {
            command
                .env(DHCP_HOOK_ROLE_ENV, "v1")
                .env(DHCP_GENERATION_ENV, generation.as_str())
                .env(
                    DHCP_UPLINK_ENV,
                    match uplink {
                        DhcpUplink::Ethernet => "ethernet",
                        DhcpUplink::Wifi => "wifi",
                    },
                );
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                cleanup_unstarted_dhcp_generation(dhcp.as_ref());
                return Err(PlatformError::Io(format!(
                    "start {} directly: {error}",
                    service.label()
                )));
            }
        };
        let pid = child.id();
        let identity = match identify_spawned_service(service, pid, PROCESS_WAIT) {
            Ok(identity) => identity,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                cleanup_unstarted_dhcp_generation(dhcp.as_ref());
                return Err(error);
            }
        };
        if let Err(error) =
            storage::atomic_write_private(service.record(), identity.encode().as_bytes())
        {
            let _ = child.kill();
            let _ = child.wait();
            cleanup_unstarted_dhcp_generation(dhcp.as_ref());
            return Err(error);
        }
        super::process::reap_in_background(child, service.label());
        Ok(())
    }

    pub(crate) fn stop_owned_management_services(&self) -> Result<(), PlatformError> {
        for service in SERVICES.into_iter().rev() {
            self.stop_service(service)?;
        }
        let active = read_active_dhcp_generation(DhcpUplink::Wifi)?;
        let owned = read_dhcp_ownership(DhcpUplink::Wifi)?;
        if let Some(generation) = active {
            self.apply_dhcp_event_locked(&DhcpEvent::new(
                DhcpUplink::Wifi,
                generation.clone(),
                DhcpTransition::Deconfig,
            ))?;
            retire_active_dhcp_generation(DhcpUplink::Wifi, &generation)?;
        } else if let Some(owned) = owned {
            reconcile_owned_generation(DhcpUplink::Wifi, Some(&owned), None)?;
            storage::remove_file_durable(dhcp_ownership_record(DhcpUplink::Wifi))?;
        }
        Ok(())
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

    fn wait_for_radio_channel_ready(
        &self,
        config: &NetworkConfigV1,
        channel: ApRadioChannel,
        timeout: Duration,
    ) -> Result<(), PlatformError> {
        // The single radio only accepts the AP channel once it is operationally there: the STA
        // confirmed on the shared channel AND the driver's current supported-channel set listing
        // it. Both signals still precede the exact VHT80 readiness (the AP settles into it tens of
        // seconds later), which the longer AP_READY_WAIT absorbs. Without any upstream the STA can
        // never confirm the channel, so the gate degrades to the recorded-channel start.
        let mut ready = || {
            if !radio_currently_supports_channel(WAN_INTERFACE, channel)? {
                return Ok(false);
            }
            match self.sta_associated_with(&config.sta, channel) {
                Ok(ready) => Ok(ready),
                Err(error) if is_management_probe_timeout(&error) => Ok(false),
                Err(error) => Err(error),
            }
        };
        let mut degrade = || radio_currently_supports_channel(WAN_INTERFACE, channel);
        wait_for_radio_channel_ready_until(
            &mut ready,
            &mut degrade,
            Instant::now() + timeout,
            || thread::sleep(POLL_INTERVAL),
        )
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

    pub(crate) fn observe_dhcp_uplink(&self, uplink: DhcpUplink) -> UplinkObserved {
        let interface = dhcp_uplink_interface(uplink);
        let link = match uplink {
            DhcpUplink::Ethernet => self
                .observe_ethernet_carrier()
                .map(Probe::Known)
                .unwrap_or_else(|error| Probe::Unknown(error.to_string())),
            DhcpUplink::Wifi => {
                match fs::read_to_string(format!("/sys/class/net/{interface}/operstate")) {
                    Ok(state) => Probe::Known(matches!(state.trim(), "up" | "unknown")),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        Probe::Known(false)
                    }
                    Err(error) => Probe::Unknown(format!("read {interface} link state: {error}")),
                }
            }
        };
        let active = read_active_dhcp_generation(uplink);
        let ownership = read_dhcp_ownership(uplink);
        let session = match (&active, &ownership) {
            (Ok(Some(generation)), Ok(Some(ownership)))
                if ownership.generation.as_ref() == Some(generation) =>
            {
                match self.read_service_identity(match uplink {
                    DhcpUplink::Ethernet => ManagementService::EthernetUdhcpc,
                    DhcpUplink::Wifi => ManagementService::Udhcpc,
                }) {
                    Ok(Some(identity)) => match identity.matches(match uplink {
                        DhcpUplink::Ethernet => ManagementService::EthernetUdhcpc,
                        DhcpUplink::Wifi => ManagementService::Udhcpc,
                    }) {
                        Ok(true) => Probe::Known(true),
                        Ok(false) => Probe::Known(false),
                        Err(error) => Probe::Unknown(error.to_string()),
                    },
                    Ok(None) => Probe::Known(false),
                    Err(error) => Probe::Unknown(error.to_string()),
                }
            }
            (Ok(_), Ok(_)) => Probe::Known(false),
            (Err(error), _) | (_, Err(error)) => Probe::Unknown(error.to_string()),
        };
        let (address, default_route, gateway, resolver) = match ownership {
            Ok(Some(ownership)) => {
                let address = match owned_address_present(uplink, &ownership.address) {
                    Ok(true) => Probe::Known(OwnedResource::Owned {
                        token: ownership.generation.as_ref().map_or_else(
                            || "legacy-dhcp".to_owned(),
                            |generation| generation.as_str().to_owned(),
                        ),
                    }),
                    Ok(false) => Probe::Known(OwnedResource::Absent),
                    Err(error) => Probe::Unknown(error.to_string()),
                };
                let defaults = ownership
                    .routes
                    .iter()
                    .filter(|route| {
                        matches!(route.destination.as_str(), "default" | "0.0.0.0/0")
                            && route.metric == Some(dhcp_uplink_metric(uplink))
                    })
                    .collect::<Vec<_>>();
                let route = if defaults.len() == 1 {
                    let routes = self.run_management_probe(
                        "/usr/sbin/ip",
                        &[
                            "-4",
                            "route",
                            "show",
                            "default",
                            "dev",
                            dhcp_uplink_interface(uplink),
                        ],
                    );
                    routes
                        .ok()
                        .and_then(|routes| {
                            crate::adapters::outbound::system::exact_default_gateway(&routes)
                                .and_then(|gateway| gateway.parse().ok())
                        })
                        .filter(|gateway| *gateway == defaults[0].gateway)
                } else {
                    None
                };
                let default_route = Probe::Known(route.is_some());
                let gateway = Probe::Known(route);
                let resolver = match read_dhcp_resolver_record(uplink, Some(&ownership)) {
                    Ok(Some(record))
                        if ownership.generation.as_ref() == Some(&record.generation) =>
                    {
                        Probe::Known(true)
                    }
                    Ok(_) => Probe::Known(false),
                    Err(error) => Probe::Unknown(error.to_string()),
                };
                (address, default_route, gateway, resolver)
            }
            Ok(None) => (
                Probe::Known(OwnedResource::Absent),
                Probe::Known(false),
                Probe::Known(None),
                Probe::Known(false),
            ),
            Err(error) => {
                let reason = error.to_string();
                (
                    Probe::Unknown(reason.clone()),
                    Probe::Unknown(reason.clone()),
                    Probe::Unknown(reason.clone()),
                    Probe::Unknown(reason),
                )
            }
        };
        UplinkObserved {
            link,
            session,
            address,
            default_route,
            gateway,
            resolver,
        }
    }

    pub(crate) fn active_resolver_nameservers(
        &self,
    ) -> Result<Option<(DhcpUplink, Vec<Ipv4Addr>)>, PlatformError> {
        selected_resolver_record()?.map_or(Ok(None), |record| {
            let mut nameservers = Vec::new();
            for entry in record.entries {
                let Some(value) = entry.strip_prefix("nameserver ") else {
                    continue;
                };
                let address = value
                    .split_whitespace()
                    .next()
                    .ok_or_else(invalid_dhcp_ownership)?
                    .parse::<Ipv4Addr>()
                    .map_err(|_| invalid_dhcp_ownership())?;
                if !nameservers.contains(&address) {
                    nameservers.push(address);
                }
            }
            if nameservers.len() > 16 {
                return Err(invalid_dhcp_ownership());
            }
            Ok(Some((record.uplink, nameservers)))
        })
    }
    pub(crate) fn owned_sta_address_and_route_ready(&self) -> Result<bool, PlatformError> {
        self.owned_dhcp_uplink_ready(DhcpUplink::Wifi)
    }

    fn owned_dhcp_uplink_ready(&self, uplink: DhcpUplink) -> Result<bool, PlatformError> {
        let Some(ownership) = read_dhcp_ownership(uplink)? else {
            return Ok(false);
        };
        let interface = dhcp_uplink_interface(uplink);
        let addresses = self.run_management_probe(
            "/usr/sbin/ip",
            &["-o", "-4", "address", "show", "dev", interface],
        )?;
        if !address_output_contains(&addresses, &ownership.address.cidr) {
            return Ok(false);
        }

        let owned_defaults = ownership
            .routes
            .iter()
            .filter(|route| {
                matches!(route.destination.as_str(), "default" | "0.0.0.0/0")
                    && route.metric == Some(dhcp_uplink_metric(uplink))
            })
            .collect::<Vec<_>>();
        if owned_defaults.len() != 1 {
            return Ok(false);
        }
        let routes =
            self.run_management_probe("/usr/sbin/ip", &["-4", "route", "show", "default"])?;
        let live = routes
            .lines()
            .filter(|line| route_line_uses_interface(line, interface))
            .collect::<Vec<_>>();
        if live.len() != 1 || !exact_default_route_line(uplink, live[0], owned_defaults[0]) {
            return Ok(false);
        }
        owned_generation_matches(uplink, Some(&ownership))
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

    fn apply_dhcp_event_locked(&self, event: &DhcpEvent) -> Result<(), PlatformError> {
        match &event.transition {
            DhcpTransition::Deconfig => {
                run_ip(&[
                    "link",
                    "set",
                    "dev",
                    dhcp_uplink_interface(event.uplink),
                    "up",
                ])?;
                let Some(previous) = read_dhcp_ownership(event.uplink)? else {
                    return Ok(());
                };
                reconcile_owned_generation(event.uplink, Some(&previous), None)?;
                if let Err(error) =
                    storage::remove_file_durable(dhcp_ownership_record(event.uplink))
                {
                    if read_dhcp_ownership(event.uplink)?.is_some() {
                        let rollback =
                            reconcile_owned_generation(event.uplink, None, Some(&previous));
                        return match rollback {
                            Ok(()) => Err(error),
                            Err(rollback) => Err(PlatformError::InvalidState(format!(
                                "DHCP ownership removal failed: {error}; exact rollback also failed: {rollback}"
                            ))),
                        };
                    }
                }
                storage::remove_file_durable(dhcp_resolver_record(event.uplink))?;
                publish_selected_resolver_surface()?;
                Ok(())
            }
            DhcpTransition::Lease { lease } => {
                let next = DhcpOwnership::from_lease(event.uplink, event.generation.clone(), lease);
                next.validate()?;
                let previous = read_dhcp_ownership(event.uplink)?;
                reconcile_owned_generation(event.uplink, previous.as_ref(), Some(&next))?;
                if let Err(error) = persist_dhcp_ownership_verified(event.uplink, &next) {
                    let rollback =
                        reconcile_owned_generation(event.uplink, Some(&next), previous.as_ref());
                    return match rollback { Ok(()) => Err(error), Err(rollback) => Err(PlatformError::InvalidState(format!("DHCP ownership commit failed: {error}; exact rollback also failed: {rollback}"))) };
                }
                let resolver =
                    DhcpResolverRecord::from_lease(event.uplink, event.generation.clone(), lease);
                persist_dhcp_resolver_record(&resolver)?;
                publish_selected_resolver_surface()?;
                Ok(())
            }
            DhcpTransition::NoChange => Ok(()),
        }
    }
}

impl EthernetDhcpLifecyclePort for super::process::LinuxRouterPlatform {
    fn ethernet_carrier_up(&self) -> Result<bool, PlatformError> {
        self.observe_ethernet_carrier()
    }

    fn management_wifi_ready(&self) -> Result<bool, PlatformError> {
        self.management_services_ready()
    }

    fn start_ethernet_dhcp(&self) -> Result<(), PlatformError> {
        self.ensure_ethernet_dhcp_process()
    }

    fn stop_ethernet_dhcp(&self) -> Result<(), PlatformError> {
        self.stop_ethernet_dhcp_process()
    }
}

impl DhcpPlatformPort for super::process::LinuxRouterPlatform {
    fn active_dhcp_generation(
        &self,
        uplink: DhcpUplink,
    ) -> Result<Option<DhcpGeneration>, PlatformError> {
        read_active_dhcp_generation(uplink)
    }

    fn apply_dhcp_event(&self, event: &DhcpEvent) -> Result<(), PlatformError> {
        self.apply_dhcp_event_locked(event)
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

/// Read the persisted last-good STA channel record. This is a runtime-derived cache: a missing,
/// malformed, oversized or otherwise unusable record is treated as absent (returning `None`) so
/// that management startup never blocks on it and falls back to the bounded association wait.
fn read_last_good_channel() -> Result<Option<LastGoodStaChannel>, PlatformError> {
    read_last_good_channel_at(LAST_GOOD_CHANNEL_PATH)
}

fn read_last_good_channel_at(path: &str) -> Result<Option<LastGoodStaChannel>, PlatformError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            eprintln!("hyz-router: read last-good STA channel record: {error}");
            return Ok(None);
        }
    };
    if bytes.len() > MAX_LAST_GOOD_BYTES {
        eprintln!("hyz-router: last-good STA channel record exceeds size limit");
        return Ok(None);
    }
    match serde_json::from_slice::<LastGoodStaChannel>(&bytes) {
        Ok(record) => Ok(Some(record)),
        Err(error) => {
            eprintln!("hyz-router: ignore malformed last-good STA channel record: {error}");
            Ok(None)
        }
    }
}

fn write_last_good_channel(record: &LastGoodStaChannel) -> Result<(), PlatformError> {
    write_last_good_channel_at(LAST_GOOD_CHANNEL_PATH, record)
}

fn write_last_good_channel_at(
    path: &str,
    record: &LastGoodStaChannel,
) -> Result<(), PlatformError> {
    let json = serde_json::to_vec(record).map_err(|error| {
        PlatformError::InvalidState(format!("serialize last-good STA channel: {error}"))
    })?;
    storage::atomic_write_private(path, &json)
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
    decode_process_argv(&bytes)
}

fn decode_process_argv(bytes: &[u8]) -> Result<Vec<String>, PlatformError> {
    // A process may become a zombie between the executable and argv probes. Linux then exposes an
    // empty cmdline; treat that as a non-match so the caller can confirm exit by PID/start time.
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
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

/// Outcome of probing a freshly spawned management process for its identity.
///
/// On a cold single-radio start the fork/exec image settles asynchronously, so a visible process
/// may not yet carry the fixed identity. `Mismatch` is therefore transient: the caller retries
/// within its bounded window and only fails closed when the window elapses without ever observing
/// the fixed identity.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SpawnProbe {
    /// The process is not yet visible in /proc (or has already exited); keep waiting.
    NotReady,
    /// The process is visible and carries the fixed identity.
    Ready(ServiceIdentity),
    /// The process is visible but does not yet carry the fixed identity.
    Mismatch,
}

fn probe_spawned_service(
    service: ManagementService,
    pid: u32,
) -> Result<SpawnProbe, PlatformError> {
    let Some(start_time) = process_start_time(pid)? else {
        return Ok(SpawnProbe::NotReady);
    };
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
        Ok(SpawnProbe::Ready(identity))
    } else {
        Ok(SpawnProbe::Mismatch)
    }
}

fn identify_spawned_service(
    service: ManagementService,
    pid: u32,
    timeout: Duration,
) -> Result<ServiceIdentity, PlatformError> {
    let deadline = Instant::now() + timeout;
    let mut probe = || probe_spawned_service(service, pid);
    identify_spawned_service_with_probe(
        service,
        &mut probe,
        || Instant::now() < deadline,
        || thread::sleep(POLL_INTERVAL),
    )
}

fn identify_spawned_service_with_probe(
    service: ManagementService,
    probe: &mut dyn FnMut() -> Result<SpawnProbe, PlatformError>,
    mut keep_polling: impl FnMut() -> bool,
    mut wait: impl FnMut(),
) -> Result<ServiceIdentity, PlatformError> {
    let mut mismatch_seen = false;
    loop {
        match probe()? {
            SpawnProbe::Ready(identity) => return Ok(identity),
            SpawnProbe::Mismatch => mismatch_seen = true,
            SpawnProbe::NotReady => {}
        }
        if !keep_polling() {
            return Err(if mismatch_seen {
                PlatformError::Conflict(format!(
                    "new {} process did not have the fixed identity",
                    service.label()
                ))
            } else {
                PlatformError::ProbeFailed(format!(
                    "{} disappeared during startup",
                    service.label()
                ))
            });
        }
        wait();
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
    if service.dhcp_uplink().is_none() {
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
    service.dhcp_uplink().is_none() || comm == service.process_name()
}

fn executable_instance_matches_service(service: ManagementService, argv: &[String]) -> bool {
    // udhcpc is a BusyBox applet, so canonicalizing /sbin/udhcpc yields /bin/busybox. Counting
    // executable identities alone would classify every live BusyBox applet as another DHCP client.
    service.dhcp_uplink().is_none() || command_line_has_service_signature(service, argv)
}

pub(crate) fn command_line_has_service_signature(
    service: ManagementService,
    argv: &[String],
) -> bool {
    let Some(argv0) = argv.first() else {
        return false;
    };
    let argv0_matches = argv0 == service.executable()
        || Path::new(argv0).file_name().and_then(|name| name.to_str())
            == Some(service.process_name());
    if !argv0_matches {
        return false;
    }
    match service {
        ManagementService::WpaSupplicant => interface_argument_matches(argv, WAN_INTERFACE),
        ManagementService::Udhcpc => interface_argument_matches(argv, WAN_INTERFACE),
        ManagementService::EthernetUdhcpc => {
            interface_argument_matches(argv, ETHERNET_WAN_INTERFACE)
        }
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

/// Parses the RTL8852BS driver's current supported operating-class list (the per-interface
/// `cur_spt_op_class_ch` proc file, which is what hostapd's "current mode channel list" derives
/// from) and reports whether the channel is present. The list is transient on a cold start: the
/// upper-5GHz channels are missing until the radio finishes initializing/scans, then appear even
/// while the STA later rescans.
fn current_driver_channel_set_contains(content: &str, channel_number: u8) -> bool {
    content.lines().any(|line| {
        let mut fields = line.split_whitespace();
        let Some(class) = fields.next() else {
            return false;
        };
        if class.parse::<u8>().is_err() {
            // Header ("class band bw ch_list") and summary ("op_class number:N") lines.
            return false;
        }
        // Fields after the class id: band, bw, then the channel list.
        fields.next();
        fields.next();
        fields.any(|token| token.parse::<u8>() == Ok(channel_number))
    })
}

fn radio_currently_supports_channel(
    interface: &str,
    channel: ApRadioChannel,
) -> Result<bool, PlatformError> {
    let path = format!("/proc/net/rtl8852bs/{interface}/cur_spt_op_class_ch");
    let content = fs::read_to_string(&path).map_err(|error| {
        PlatformError::ProbeFailed(format!(
            "read {interface} current supported channels: {error}"
        ))
    })?;
    Ok(current_driver_channel_set_contains(
        &content,
        channel.number(),
    ))
}

fn wait_for_radio_channel_ready_until(
    ready: &mut dyn FnMut() -> Result<bool, PlatformError>,
    degrade: &mut dyn FnMut() -> Result<bool, PlatformError>,
    deadline: Instant,
    mut wait: impl FnMut(),
) -> Result<(), PlatformError> {
    loop {
        if ready()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            // No upstream may ever associate; degrade to the legacy recorded-channel start when
            // the driver still lists the channel as supported, and fail closed only when the
            // channel itself is unavailable.
            if degrade()? {
                return Ok(());
            }
            return Err(PlatformError::ProbeFailed(
                "single radio did not become ready for the target AP channel within its bounded wait"
                    .to_owned(),
            ));
        }
        wait();
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

fn exact_default_route_line(uplink: DhcpUplink, line: &str, route: &OwnedRoute) -> bool {
    let gateway = route.gateway.to_string();
    let metric = dhcp_uplink_metric(uplink).to_string();
    line.split_whitespace().collect::<Vec<_>>()
        == [
            "default",
            "via",
            gateway.as_str(),
            "dev",
            dhcp_uplink_interface(uplink),
            "metric",
            metric.as_str(),
        ]
}

fn new_dhcp_generation(unix_time_millis: u64) -> Result<DhcpGeneration, PlatformError> {
    let pid = std::process::id();
    let start_time = process_start_time(pid)?.ok_or_else(|| {
        PlatformError::InvalidState("cannot identify DHCP generation owner".to_owned())
    })?;
    let sequence = DHCP_GENERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    DhcpGeneration::new(format!(
        "dhcp-{pid}-{start_time}-{unix_time_millis}-{sequence}"
    ))
    .map_err(|message| PlatformError::InvalidState(message.to_owned()))
}

fn dhcp_uplink_interface(uplink: DhcpUplink) -> &'static str {
    match uplink {
        DhcpUplink::Ethernet => crate::domain::network::ETHERNET_WAN_INTERFACE,
        DhcpUplink::Wifi => WAN_INTERFACE,
    }
}

fn dhcp_uplink_metric(uplink: DhcpUplink) -> u32 {
    match uplink {
        DhcpUplink::Ethernet => 100,
        DhcpUplink::Wifi => 600,
    }
}

fn dhcp_active_generation_record(uplink: DhcpUplink) -> &'static str {
    match uplink {
        DhcpUplink::Ethernet => ETHERNET_DHCP_ACTIVE_GENERATION_RECORD,
        DhcpUplink::Wifi => WIFI_DHCP_ACTIVE_GENERATION_RECORD,
    }
}

fn dhcp_ownership_record(uplink: DhcpUplink) -> &'static str {
    match uplink {
        DhcpUplink::Ethernet => ETHERNET_DHCP_OWNERSHIP_RECORD,
        DhcpUplink::Wifi => WIFI_DHCP_OWNERSHIP_RECORD,
    }
}

fn dhcp_resolver_record(uplink: DhcpUplink) -> &'static str {
    match uplink {
        DhcpUplink::Ethernet => ETHERNET_DHCP_RESOLVER_RECORD,
        DhcpUplink::Wifi => WIFI_DHCP_RESOLVER_RECORD,
    }
}

fn read_dhcp_resolver_record(
    uplink: DhcpUplink,
    ownership: Option<&DhcpOwnership>,
) -> Result<Option<DhcpResolverRecord>, PlatformError> {
    if let Some(record) =
        storage::read_private_small_optional(dhcp_resolver_record(uplink), 16 * 1024)?
    {
        let record = serde_json::from_str::<DhcpResolverRecord>(&record)
            .map_err(|_| invalid_dhcp_ownership())?;
        record.validate()?;
        if record.uplink != uplink {
            return Err(invalid_dhcp_ownership());
        }
        return Ok(Some(record));
    }
    // Version 1-3 ownership records carried resolver lines. They are accepted only when their
    // generation is still exact; the next lease writes the split root-owned resolver record.
    Ok(ownership
        .and_then(|ownership| {
            ownership
                .generation
                .clone()
                .map(|generation| DhcpResolverRecord {
                    version: 1,
                    uplink,
                    generation,
                    entries: ownership.resolver_entries.clone(),
                })
        })
        .filter(|record| !record.entries.is_empty()))
}

fn persist_dhcp_resolver_record(record: &DhcpResolverRecord) -> Result<(), PlatformError> {
    record.validate()?;
    let encoded = serde_json::to_vec(record).map_err(|error| {
        PlatformError::InvalidState(format!("encode DHCP resolver record: {error}"))
    })?;
    storage::atomic_write_private(dhcp_resolver_record(record.uplink), &encoded)
}

fn read_active_dhcp_generation(
    uplink: DhcpUplink,
) -> Result<Option<DhcpGeneration>, PlatformError> {
    let Some(record) =
        storage::read_private_small_optional(dhcp_active_generation_record(uplink), 128)?
    else {
        return Ok(None);
    };
    DhcpGeneration::new(record.trim_end_matches('\n').to_owned())
        .map(Some)
        .map_err(|message| PlatformError::InvalidState(message.to_owned()))
}

fn retire_active_dhcp_generation(
    uplink: DhcpUplink,
    generation: &DhcpGeneration,
) -> Result<(), PlatformError> {
    match read_active_dhcp_generation(uplink)? {
        Some(active) if active == *generation => {
            storage::remove_file_durable(dhcp_active_generation_record(uplink))
        }
        Some(_) => Err(PlatformError::Conflict(
            "active DHCP generation changed during retirement".to_owned(),
        )),
        None => Ok(()),
    }
}

fn cleanup_unstarted_dhcp_generation(dhcp: Option<&(DhcpUplink, DhcpGeneration)>) {
    if let Some((uplink, generation)) = dhcp {
        let _ = retire_active_dhcp_generation(*uplink, generation);
    }
}

fn read_dhcp_ownership(uplink: DhcpUplink) -> Result<Option<DhcpOwnership>, PlatformError> {
    let Some(record) = storage::read_small_optional(dhcp_ownership_record(uplink), 16 * 1024)?
    else {
        return Ok(None);
    };
    let ownership =
        serde_json::from_str::<DhcpOwnership>(&record).map_err(|_| invalid_dhcp_ownership())?;
    ownership.validate()?;
    if ownership.uplink != uplink {
        return Err(invalid_dhcp_ownership());
    }
    Ok(Some(ownership))
}

fn persist_dhcp_ownership_verified(
    uplink: DhcpUplink,
    ownership: &DhcpOwnership,
) -> Result<(), PlatformError> {
    if ownership.uplink != uplink {
        return Err(invalid_dhcp_ownership());
    }
    let encoded = serde_json::to_vec(ownership).map_err(|_| {
        PlatformError::InvalidState("could not encode DHCP ownership record".to_owned())
    })?;
    let Err(write_error) = storage::atomic_write_private(dhcp_ownership_record(uplink), &encoded)
    else {
        return Ok(());
    };
    match read_dhcp_ownership(uplink) {
        Ok(Some(actual)) if actual == *ownership => Ok(()),
        Ok(_) => Err(write_error),
        Err(read_error) => Err(PlatformError::InvalidState(format!(
            "DHCP ownership commit was ambiguous: {write_error}; read-back also failed: {read_error}"
        ))),
    }
}

fn verify_owned_generation_prestate(
    uplink: DhcpUplink,
    previous: Option<&DhcpOwnership>,
    next: Option<&DhcpOwnership>,
) -> Result<(), PlatformError> {
    let previous_address = previous.map(|ownership| &ownership.address);
    let next_address = next.map(|ownership| &ownership.address);
    if previous_address != next_address {
        let output = run_bounded(
            "/usr/sbin/ip",
            &[
                "-o",
                "-4",
                "address",
                "show",
                "dev",
                dhcp_uplink_interface(uplink),
            ],
            Duration::from_secs(3),
        )?;
        let live = output
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let exact = previous_address
            .map(|address| live.len() == 1 && exact_owned_address_line(live[0], address))
            .unwrap_or_else(|| live.is_empty());
        if !exact {
            return Err(PlatformError::Conflict(
                "WAN IPv4 address state no longer exactly matches recorded DHCP ownership"
                    .to_owned(),
            ));
        }
    }

    let mut destinations = Vec::new();
    for route in previous
        .into_iter()
        .flat_map(|ownership| ownership.routes.iter())
        .chain(
            next.into_iter()
                .flat_map(|ownership| ownership.routes.iter()),
        )
    {
        let destination = canonical_route_destination(&route.destination);
        if !destinations.contains(&destination) {
            destinations.push(destination);
        }
    }
    for destination in destinations {
        let previous_routes = previous
            .into_iter()
            .flat_map(|ownership| ownership.routes.iter())
            .filter(|route| canonical_route_destination(&route.destination) == destination)
            .collect::<Vec<_>>();
        let next_routes = next
            .into_iter()
            .flat_map(|ownership| ownership.routes.iter())
            .filter(|route| canonical_route_destination(&route.destination) == destination)
            .collect::<Vec<_>>();
        if previous_routes == next_routes {
            continue;
        }
        let output = run_bounded(
            "/usr/sbin/ip",
            &["-4", "route", "show", destination.as_str()],
            Duration::from_secs(3),
        )?;
        let live = output
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let mut unmatched = previous_routes;
        for line in live {
            let Some(index) = unmatched
                .iter()
                .position(|route| exact_owned_route_line(uplink, line, route))
            else {
                return Err(PlatformError::Conflict(format!(
                    "route destination {destination} contains foreign or changed state"
                )));
            };
            unmatched.remove(index);
        }
        if !unmatched.is_empty() {
            return Err(PlatformError::Conflict(format!(
                "route destination {destination} is missing recorded DHCP-owned state"
            )));
        }
    }
    Ok(())
}

#[derive(Debug)]
enum DhcpCompensation {
    EnsureAddress(OwnedAddress),
    RemoveAddress(OwnedAddress),
    EnsureRoute(OwnedRoute),
    RemoveRoute(OwnedRoute),
}

fn reconcile_owned_generation(
    uplink: DhcpUplink,
    previous: Option<&DhcpOwnership>,
    next: Option<&DhcpOwnership>,
) -> Result<(), PlatformError> {
    if previous.is_some_and(|ownership| ownership.uplink != uplink)
        || next.is_some_and(|ownership| ownership.uplink != uplink)
    {
        return Err(invalid_dhcp_ownership());
    }
    verify_owned_generation_prestate(uplink, previous, next)?;
    let mut journal = Vec::new();
    let result = (|| {
        if let Some(previous) = previous {
            for route in previous.routes.iter().rev() {
                if !next.is_some_and(|next| next.routes.contains(route)) {
                    remove_owned_route(uplink, route)?;
                    journal.push(DhcpCompensation::EnsureRoute(route.clone()));
                }
            }
            if !next.is_some_and(|next| next.address == previous.address) {
                remove_owned_address(uplink, &previous.address)?;
                journal.push(DhcpCompensation::EnsureAddress(previous.address.clone()));
            }
        }
        if let Some(next) = next {
            if !previous.is_some_and(|previous| previous.address == next.address) {
                ensure_owned_address(uplink, &next.address)?;
                journal.push(DhcpCompensation::RemoveAddress(next.address.clone()));
            }
            for route in &next.routes {
                if !previous.is_some_and(|previous| previous.routes.contains(route)) {
                    ensure_owned_route(uplink, route)?;
                    journal.push(DhcpCompensation::RemoveRoute(route.clone()));
                }
            }
        }
        if owned_generation_matches(uplink, next)? {
            Ok(())
        } else {
            Err(PlatformError::UnsafeToCutOver(
                "DHCP address, route, or resolver transaction failed strict read-back".to_owned(),
            ))
        }
    })();
    if let Err(primary) = result {
        let mut rollback_errors = Vec::new();
        for compensation in journal.into_iter().rev() {
            if let Err(error) = apply_dhcp_compensation(uplink, compensation) {
                rollback_errors.push(error.to_string());
            }
        }
        return if rollback_errors.is_empty() {
            Err(primary)
        } else {
            Err(PlatformError::InvalidState(format!(
                "DHCP transaction failed: {primary}; exact action rollback also failed: {}",
                rollback_errors.join("; ")
            )))
        };
    }
    Ok(())
}

fn apply_dhcp_compensation(
    uplink: DhcpUplink,
    compensation: DhcpCompensation,
) -> Result<(), PlatformError> {
    match compensation {
        DhcpCompensation::EnsureAddress(address) => {
            require_wan_address_state(uplink, None)?;
            ensure_owned_address(uplink, &address)
        }
        DhcpCompensation::RemoveAddress(address) => {
            require_wan_address_state(uplink, Some(&address))?;
            remove_owned_address(uplink, &address)
        }
        DhcpCompensation::EnsureRoute(route) => {
            require_route_state(uplink, &route.destination, None)?;
            ensure_owned_route(uplink, &route)
        }
        DhcpCompensation::RemoveRoute(route) => {
            require_route_state(uplink, &route.destination, Some(&route))?;
            remove_owned_route(uplink, &route)
        }
    }
}

fn require_wan_address_state(
    uplink: DhcpUplink,
    expected: Option<&OwnedAddress>,
) -> Result<(), PlatformError> {
    let output = run_bounded(
        "/usr/sbin/ip",
        &[
            "-o",
            "-4",
            "address",
            "show",
            "dev",
            dhcp_uplink_interface(uplink),
        ],
        Duration::from_secs(3),
    )?;
    let live = output
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let matches = expected
        .map(|address| live.len() == 1 && exact_owned_address_line(live[0], address))
        .unwrap_or_else(|| live.is_empty());
    if matches {
        Ok(())
    } else {
        Err(PlatformError::Conflict(
            "WAN IPv4 address changed before DHCP rollback".to_owned(),
        ))
    }
}

fn require_route_state(
    uplink: DhcpUplink,
    destination: &str,
    expected: Option<&OwnedRoute>,
) -> Result<(), PlatformError> {
    let destination = canonical_route_destination(destination);
    let output = run_bounded(
        "/usr/sbin/ip",
        &["-4", "route", "show", destination.as_str()],
        Duration::from_secs(3),
    )?;
    let live = output
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let matches = expected
        .map(|route| live.len() == 1 && exact_owned_route_line(uplink, live[0], route))
        .unwrap_or_else(|| live.is_empty());
    if matches {
        Ok(())
    } else {
        Err(PlatformError::Conflict(format!(
            "route destination {destination} changed before DHCP rollback"
        )))
    }
}

fn ensure_owned_address(uplink: DhcpUplink, address: &OwnedAddress) -> Result<(), PlatformError> {
    run_ip_rechecked(&address_args(uplink, "replace", address), || {
        owned_address_present(uplink, address)
    })
}

fn remove_owned_address(uplink: DhcpUplink, address: &OwnedAddress) -> Result<(), PlatformError> {
    run_ip_rechecked(&address_args(uplink, "del", address), || {
        owned_address_present(uplink, address).map(|present| !present)
    })
}

fn ensure_owned_route(uplink: DhcpUplink, route: &OwnedRoute) -> Result<(), PlatformError> {
    run_ip_rechecked(&route_args(uplink, "replace", route), || {
        owned_route_present(uplink, route)
    })
}

fn remove_owned_route(uplink: DhcpUplink, route: &OwnedRoute) -> Result<(), PlatformError> {
    run_ip_rechecked(&route_args(uplink, "del", route), || {
        owned_route_present(uplink, route).map(|present| !present)
    })
}

fn run_ip_rechecked(
    args: &[String],
    verify: impl FnOnce() -> Result<bool, PlatformError>,
) -> Result<(), PlatformError> {
    let operation = run_ip_owned(args);
    match verify() {
        Ok(true) => Ok(()),
        Ok(false) => match operation {
            Err(error) => Err(error),
            Ok(()) => Err(PlatformError::ProbeFailed(
                "fixed ip operation did not reach its exact requested state".to_owned(),
            )),
        },
        Err(probe) => match operation {
            Err(operation) => Err(PlatformError::InvalidState(format!(
                "fixed ip operation failed: {operation}; exact read-back also failed: {probe}"
            ))),
            Ok(()) => Err(probe),
        },
    }
}

fn owned_address_present(
    uplink: DhcpUplink,
    address: &OwnedAddress,
) -> Result<bool, PlatformError> {
    let output = run_bounded(
        "/usr/sbin/ip",
        &[
            "-o",
            "-4",
            "address",
            "show",
            "dev",
            dhcp_uplink_interface(uplink),
        ],
        Duration::from_secs(3),
    )?;
    Ok(output
        .lines()
        .any(|line| exact_owned_address_line(line, address)))
}

fn exact_owned_address_line(line: &str, address: &OwnedAddress) -> bool {
    let fields = line.split_whitespace().collect::<Vec<_>>();
    let cidr = fields
        .windows(2)
        .any(|pair| pair == ["inet", address.cidr.as_str()]);
    let broadcast = address.broadcast.is_none_or(|broadcast| {
        let broadcast = broadcast.to_string();
        fields
            .windows(2)
            .any(|pair| pair == ["brd", broadcast.as_str()])
    });
    cidr && broadcast
}

fn owned_route_present(uplink: DhcpUplink, route: &OwnedRoute) -> Result<bool, PlatformError> {
    let output = run_bounded(
        "/usr/sbin/ip",
        &["-4", "route", "show", route.destination.as_str()],
        Duration::from_secs(3),
    )?;
    Ok(output
        .lines()
        .any(|line| exact_owned_route_line(uplink, line, route)))
}

fn exact_owned_route_line(uplink: DhcpUplink, line: &str, route: &OwnedRoute) -> bool {
    let destination = canonical_route_destination(&route.destination);
    let mut expected = vec![
        destination,
        "via".to_owned(),
        route.gateway.to_string(),
        "dev".to_owned(),
        dhcp_uplink_interface(uplink).to_owned(),
    ];
    if let Some(metric) = route.metric {
        expected.extend(["metric".to_owned(), metric.to_string()]);
    }
    line.split_whitespace()
        .eq(expected.iter().map(String::as_str))
}

fn canonical_route_destination(destination: &str) -> String {
    if matches!(destination, "default" | "0.0.0.0/0") {
        return "default".to_owned();
    }
    let Some((address, prefix)) = destination.split_once('/') else {
        return destination.to_owned();
    };
    let (Ok(address), Ok(prefix)) = (address.parse::<Ipv4Addr>(), prefix.parse::<u8>()) else {
        return destination.to_owned();
    };
    if prefix > 32 {
        return destination.to_owned();
    }
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    format!("{}/{}", Ipv4Addr::from(u32::from(address) & mask), prefix)
}

fn owned_generation_matches(
    uplink: DhcpUplink,
    ownership: Option<&DhcpOwnership>,
) -> Result<bool, PlatformError> {
    if let Some(ownership) = ownership {
        if !owned_address_present(uplink, &ownership.address)? {
            return Ok(false);
        }
        for route in &ownership.routes {
            if !owned_route_present(uplink, route)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn address_args(uplink: DhcpUplink, operation: &str, address: &OwnedAddress) -> Vec<String> {
    let mut args = vec![
        "-4".to_owned(),
        "address".to_owned(),
        operation.to_owned(),
        address.cidr.clone(),
    ];
    if operation == "replace" {
        if let Some(broadcast) = address.broadcast {
            args.extend(["broadcast".to_owned(), broadcast.to_string()]);
        }
    }
    args.extend(["dev".to_owned(), dhcp_uplink_interface(uplink).to_owned()]);
    args
}

fn route_args(uplink: DhcpUplink, operation: &str, route: &OwnedRoute) -> Vec<String> {
    let mut args = vec![
        "-4".to_owned(),
        "route".to_owned(),
        operation.to_owned(),
        route.destination.clone(),
        "via".to_owned(),
        route.gateway.to_string(),
        "dev".to_owned(),
        dhcp_uplink_interface(uplink).to_owned(),
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

fn selected_resolver_record() -> Result<Option<DhcpResolverRecord>, PlatformError> {
    let ethernet = read_dhcp_ownership(DhcpUplink::Ethernet)?;
    let wifi = read_dhcp_ownership(DhcpUplink::Wifi)?;
    for (uplink, ownership) in [
        (DhcpUplink::Ethernet, ethernet.as_ref()),
        (DhcpUplink::Wifi, wifi.as_ref()),
    ] {
        let Some(ownership) = ownership else {
            continue;
        };
        let Some(record) = read_dhcp_resolver_record(uplink, Some(ownership))? else {
            continue;
        };
        if ownership.generation.as_ref() == Some(&record.generation)
            && owned_generation_matches(uplink, Some(ownership))?
        {
            let defaults = ownership
                .routes
                .iter()
                .filter(|route| {
                    matches!(route.destination.as_str(), "default" | "0.0.0.0/0")
                        && route.metric == Some(dhcp_uplink_metric(uplink))
                })
                .count();
            if defaults == 1 {
                return Ok(Some(record));
            }
        }
    }
    Ok(None)
}

fn managed_resolver_line(line: &str) -> bool {
    [DhcpUplink::Ethernet, DhcpUplink::Wifi]
        .into_iter()
        .any(|uplink| line.ends_with(&format!("# {}", dhcp_uplink_interface(uplink))))
}

fn selected_resolver_surface(existing: &str, selected: &[String]) -> String {
    let managed = existing
        .lines()
        .filter(|line| managed_resolver_line(line))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    replace_resolver_text(existing, &managed, selected)
}

fn publish_selected_resolver_surface() -> Result<(), PlatformError> {
    let selected = selected_resolver_record()?;
    let path = resolver_target_path()?;
    let previous = read_resolver_bytes(&path)?;
    let existing = String::from_utf8(previous.clone())
        .map_err(|_| PlatformError::InvalidState("resolver config is not UTF-8".to_owned()))?;
    let selected_entries = selected
        .as_ref()
        .map_or_else(Vec::new, |record| record.entries.clone());
    let output = selected_resolver_surface(&existing, &selected_entries);
    if output.as_bytes() == previous {
        return Ok(());
    }
    match atomic_write_resolver(&path, &previous, output.as_bytes()) {
        Ok(()) => Ok(()),
        Err(write_error) => match read_resolver_bytes(&path) {
            Ok(actual) if actual == output.as_bytes() => Ok(()),
            Ok(actual) => {
                let restore = atomic_write_resolver(&path, &actual, &previous).and_then(|()| {
                    if read_resolver_bytes(&path)? == previous { Ok(()) } else { Err(PlatformError::UnsafeToCutOver("resolver rollback read-back differed from the prior confirmed surface".to_owned())) }
                });
                match restore { Ok(()) => Err(write_error), Err(restore) => Err(PlatformError::UnsafeToCutOver(format!("resolver selection publication failed: {write_error}; prior surface restoration failed: {restore}"))) }
            }
            Err(read_error) => Err(PlatformError::InvalidState(format!("resolver selection publication failed: {write_error}; read-back failed: {read_error}"))),
        },
    }
}

fn resolve_resolver_symlink_target(entry: &Path) -> Result<PathBuf, PlatformError> {
    let link = fs::read_link(entry)
        .map_err(|error| PlatformError::Io(format!("read resolver symlink: {error}")))?;
    let linked = if link.is_absolute() {
        link
    } else {
        entry
            .parent()
            .ok_or_else(|| PlatformError::InvalidState("resolver path has no parent".to_owned()))?
            .join(link)
    };
    let parent = linked.parent().ok_or_else(|| {
        PlatformError::InvalidState("resolver symlink target has no parent".to_owned())
    })?;
    let file_name = linked.file_name().ok_or_else(|| {
        PlatformError::InvalidState("resolver symlink target has no file name".to_owned())
    })?;
    let parent = fs::canonicalize(parent)
        .map_err(|error| PlatformError::Io(format!("resolve resolver target parent: {error}")))?;
    Ok(parent.join(file_name))
}

fn resolver_target_path() -> Result<PathBuf, PlatformError> {
    let entry = Path::new(RESOLV_CONFIG);
    let target = match fs::symlink_metadata(entry) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if metadata.uid() != 0 {
                return Err(PlatformError::InvalidState(
                    "resolver symlink must be root-owned".to_owned(),
                ));
            }
            resolve_resolver_symlink_target(entry)?
        }
        Ok(_) => entry.to_path_buf(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => entry.to_path_buf(),
        Err(error) => {
            return Err(PlatformError::Io(format!(
                "inspect resolver config: {error}"
            )))
        }
    };
    if !matches!(
        target.to_str(),
        Some(
            "/etc/resolv.conf"
                | "/run/resolv.conf"
                | "/run/hyz-router/resolv.conf"
                | "/tmp/resolv.conf"
        )
    ) {
        return Err(PlatformError::Conflict(format!(
            "resolver target {} is outside the fixed allowlist",
            target.display()
        )));
    }
    validate_resolver_target(&target)?;
    Ok(target)
}

fn validate_resolver_target(path: &Path) -> Result<(), PlatformError> {
    let parent = path
        .parent()
        .ok_or_else(|| PlatformError::InvalidState("resolver path has no parent".to_owned()))?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|error| PlatformError::Io(format!("inspect resolver parent: {error}")))?;
    let private_parent = parent_metadata.mode() & 0o022 == 0;
    let sticky_tmp = path == Path::new("/tmp/resolv.conf")
        && parent == Path::new("/tmp")
        && parent_metadata.mode() & 0o1000 != 0;
    if !parent_metadata.file_type().is_dir()
        || parent_metadata.uid() != 0
        || (!private_parent && !sticky_tmp)
    {
        return Err(PlatformError::InvalidState(
            "resolver parent must be root-owned and either non-writable or the sticky /tmp directory"
                .to_owned(),
        ));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_file()
                && metadata.uid() == 0
                && metadata.mode() & 0o022 == 0
                && metadata.nlink() == 1 =>
        {
            Ok(())
        }
        Ok(_) => Err(PlatformError::InvalidState(
            "resolver target must be a singly-linked root-owned regular file that is not group/world writable"
                .to_owned(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PlatformError::Io(format!(
            "inspect resolver target: {error}"
        ))),
    }
}

fn read_resolver_bytes(path: &Path) -> Result<Vec<u8>, PlatformError> {
    let bytes = match fs::read(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(PlatformError::Io(format!("read resolver config: {error}"))),
    };
    if bytes.len() > MAX_RESOLV_SIZE {
        return Err(PlatformError::InvalidState(
            "resolver config exceeds size limit".to_owned(),
        ));
    }
    Ok(bytes)
}

fn atomic_write_resolver(path: &Path, expected: &[u8], bytes: &[u8]) -> Result<(), PlatformError> {
    let parent = path
        .parent()
        .ok_or_else(|| PlatformError::InvalidState("resolver path has no parent".to_owned()))?;
    let mode = match fs::metadata(path) {
        Ok(metadata) => metadata.permissions().mode() & 0o777,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0o644,
        Err(error) => return Err(PlatformError::Io(format!("inspect resolver mode: {error}"))),
    };
    let base = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("resolv.conf");
    let (temporary, mut file) = loop {
        let sequence = RESOLVER_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{base}.hyz-router.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&candidate)
        {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(PlatformError::Io(format!(
                    "create resolver temporary {}: {error}",
                    candidate.display()
                )))
            }
        }
    };
    let result = (|| {
        file.write_all(bytes)
            .map_err(|error| PlatformError::Io(format!("write resolver temporary: {error}")))?;
        file.sync_all()
            .map_err(|error| PlatformError::Io(format!("sync resolver temporary: {error}")))?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(mode)).map_err(|error| {
            PlatformError::Io(format!("set resolver temporary permissions: {error}"))
        })?;
        file.sync_all().map_err(|error| {
            PlatformError::Io(format!("sync resolver temporary metadata: {error}"))
        })?;
        let current_target = resolver_target_path()?;
        if current_target != path || read_resolver_bytes(path)? != expected {
            return Err(PlatformError::Conflict(
                "resolver entry or contents changed before commit".to_owned(),
            ));
        }
        fs::rename(&temporary, path).map_err(|error| {
            PlatformError::Io(format!("commit resolver {}: {error}", path.display()))
        })?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| PlatformError::Io(format!("sync resolver directory: {error}")))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn replace_resolver_text(existing: &str, previous: &[String], next: &[String]) -> String {
    let mut removals = std::collections::HashMap::<&str, usize>::new();
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
    use crate::domain::network_config::WifiPassphrase;
    use crate::domain::wifi_startup::{ApBand, ApChannel};

    #[test]
    fn dangling_relative_resolver_symlink_resolves_through_its_existing_parent() {
        let root = std::env::temp_dir().join(format!(
            "hyz-router-resolver-test-{}-{}",
            std::process::id(),
            RESOLVER_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let etc = root.join("etc");
        let tmp = root.join("tmp");
        fs::create_dir_all(&etc).unwrap();
        fs::create_dir_all(&tmp).unwrap();
        let entry = etc.join("resolv.conf");
        std::os::unix::fs::symlink("../tmp/resolv.conf", &entry).unwrap();

        assert_eq!(
            resolve_resolver_symlink_target(&entry).unwrap(),
            tmp.join("resolv.conf")
        );
        assert!(!tmp.join("resolv.conf").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn last_good_channel_store_read_is_fail_soft_and_roundtrips() {
        let root = std::env::temp_dir().join(format!(
            "hyz-router-last-good-test-{}-{}",
            std::process::id(),
            RESOLVER_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("sta-last-good-channel.json");
        let path = path.to_string_lossy().into_owned();

        // Missing record reads as absent: the cache must never block management startup.
        assert_eq!(read_last_good_channel_at(&path).unwrap(), None);

        let config = StaConfig::from_passphrase(
            WifiSsid::new("uplink").unwrap(),
            &WifiPassphrase::new("passphrase-123").unwrap(),
        );
        let record = LastGoodStaChannel {
            fingerprint: StaFingerprint::of(&config),
            channel: ApChannel::new(ApBand::Ghz5, 161).unwrap(),
            recorded_unix_ms: 42,
        };
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert_eq!(
            read_last_good_channel_at(&path).unwrap(),
            Some(record.clone())
        );

        // Malformed JSON degrades to absent.
        fs::write(&path, b"{not json").unwrap();
        assert_eq!(read_last_good_channel_at(&path).unwrap(), None);

        // Unknown fields are rejected by deny_unknown_fields and degrade to absent.
        fs::write(
            &path,
            br#"{"fingerprint":"0000000000000000000000000000000000000000","channel":{"band":"Ghz2","number":6},"recorded_unix_ms":1,"extra":1}"#,
        )
        .unwrap();
        assert_eq!(read_last_good_channel_at(&path).unwrap(), None);

        // Oversized records degrade to absent.
        fs::write(&path, vec![b' '; MAX_LAST_GOOD_BYTES + 1]).unwrap();
        assert_eq!(read_last_good_channel_at(&path).unwrap(), None);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn empty_zombie_cmdline_is_a_non_matching_argv_not_invalid_framing() {
        assert_eq!(decode_process_argv(&[]).unwrap(), Vec::<String>::new());
        assert!(decode_process_argv(b"/usr/sbin/dnsmasq").is_err());
        assert_eq!(
            decode_process_argv(b"/usr/sbin/dnsmasq\0--no-daemon\0").unwrap(),
            ["/usr/sbin/dnsmasq", "--no-daemon"]
        );
    }

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
    fn management_restart_preserves_the_board_validated_sta_first_vht80_sequence() {
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
        let refuse_foreign = body
            .find("self.refuse_foreign_management_processes()?")
            .unwrap();
        let prepare = body.find("prepare_runtime_configs").unwrap();
        let fingerprint = body
            .find("let fingerprint = StaFingerprint::of(&config.sta)")
            .unwrap();
        let plan = body.find("startup_channel_plan(").unwrap();
        let wan_up = body
            .find("run_ip(&[\"link\", \"set\", \"dev\", WAN_INTERFACE, \"up\"])?")
            .unwrap();
        let wpa = body
            .find("self.start_service(ManagementService::WpaSupplicant)?")
            .unwrap();
        let udhcpc = body
            .find("self.start_service(ManagementService::Udhcpc)?")
            .unwrap();
        let fast_channel = body
            .find("StartupChannelPlan::FastStart { channel }")
            .unwrap();
        let sta_channel = body
            .find(".wait_for_sta_channel(STA_CHANNEL_WAIT)?")
            .unwrap();
        // The radio channel readiness gate must sit between the final channel choice (which covers
        // the live STA channel, the recorded last-good fallback, and the fixed AP fallback) and the
        // hostapd programming: it uses the same `channel` value in every branch.
        let gate = body
            .find("wait_for_radio_channel_ready(config, channel, RADIO_CHANNEL_READY_WAIT)?")
            .expect("single-radio channel readiness gate before hostapd");
        let hostapd = body.find("self.start_hostapd_on_channel").unwrap();
        let dnsmasq = body
            .find("self.start_service(ManagementService::Dnsmasq)?")
            .unwrap();
        let attach = body.find("if restore_attachment").unwrap();
        let management_ready = body.find("self.management_services_ready()?").unwrap();
        let record = body.find("write_last_good_channel(&record)").unwrap();
        assert!(
            preserve < detach
                && detach < stop
                && stop < refuse_foreign
                && refuse_foreign < prepare
                && prepare < fingerprint
                && fingerprint < plan
                && plan < wan_up
                && wan_up < wpa
                && wpa < udhcpc
                && udhcpc < fast_channel
                && fast_channel < sta_channel
                && sta_channel < gate
                && gate < hostapd
                && hostapd < dnsmasq
                && dnsmasq < attach
                && attach < management_ready
                && management_ready < record
        );
        // The full STA-channel wait exists only in the slow path, exactly once. The last-good
        // fast path must still keep a short radio-settling window (FAST_START_CONFIRM_WAIT) before
        // programming hostapd; it uses a live STA channel when present and only falls back to the
        // recorded channel when no upstream channel appears.
        assert_eq!(
            body.matches(".wait_for_sta_channel(STA_CHANNEL_WAIT)?")
                .count(),
            1
        );
        assert!(body.contains("wait_for_sta_channel(FAST_START_CONFIRM_WAIT)?"));
        assert!(body.contains("Some(live) => live"));
        assert!(body.contains("if attach_after_start"));
        assert!(body.contains("StartupChannelPlan::WaitForStaChannel"));
        assert!(body.contains("read_last_good_channel()?.as_ref()"));
        let cleanup = body.split_once("Err(primary) =>").unwrap().1;
        assert!(cleanup.find("self.detach_ap()").unwrap() < cleanup.find("for service").unwrap());

        let helper = source
            .split_once("fn start_hostapd_on_channel")
            .unwrap()
            .1
            .split_once("fn wait_for_sta_channel")
            .unwrap()
            .0;
        let country = helper.find("apply_wifi_country(config.country)?").unwrap();
        let stop_hostapd = helper
            .find("self.stop_service(ManagementService::Hostapd)?")
            .unwrap();
        let start = helper
            .find(".start_service(ManagementService::Hostapd)")
            .unwrap();
        let ready = helper
            .find("self.wait_for_hostapd(channel, AP_READY_WAIT)")
            .unwrap();
        assert!(country < stop_hostapd && stop_hostapd < start && start < ready);
        assert!(helper.contains("for attempt in 0..2"));
        assert!(source.contains("const STA_CHANNEL_WAIT: Duration = Duration::from_secs(45)"));
        assert!(source.contains("const AP_READY_WAIT: Duration = Duration::from_secs(30)"));
        assert!(!body.contains("rmmod"));
        assert!(!body.contains("insmod"));
        assert!(!body.contains("modprobe"));
        assert!(!helper.contains("rmmod"));
        assert!(!helper.contains("insmod"));
        assert!(!helper.contains("modprobe"));
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
        assert_eq!(
            ManagementService::EthernetUdhcpc.argv(),
            &["-f", "-i", "eth0", "-s", "/usr/bin/hyz-router"]
        );
        assert_eq!(ManagementService::Hostapd.argv(), &[HOSTAPD_RUNTIME_CONFIG]);
        assert_eq!(
            ManagementService::Dnsmasq.argv(),
            &[
                "--no-daemon",
                "--conf-file=/run/hyz-router/dnsmasq.rust.conf"
            ]
        );
        let source = include_str!("management.rs");
        let restart = source
            .split_once("fn restart_management_services")
            .unwrap()
            .1
            .split_once("pub(crate) fn management_services_ready")
            .unwrap()
            .0;
        assert!(!restart.contains("DhcpUplink::Ethernet"));
        assert!(!restart.contains("ETHERNET_WAN_INTERFACE"));
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
    fn spawned_service_transient_identity_mismatch_is_retried_within_the_window() {
        let service = ManagementService::Hostapd;
        let identity = ServiceIdentity {
            pid: 4242,
            start_time: 9001,
            executable: PathBuf::from(service.executable()),
            argv: expected_command_line(service).unwrap(),
        };
        // On a cold single-radio start the fork/exec image settles asynchronously, so the first
        // probes observe a pre-exec identity. The retry loop must keep probing within its bounded
        // window instead of failing on the first mismatch, and must return the settled identity.
        let outcomes = [
            SpawnProbe::Mismatch,
            SpawnProbe::Mismatch,
            SpawnProbe::Ready(identity.clone()),
        ];
        let calls = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let probe_calls = calls.clone();
        let mut probe = move || -> Result<SpawnProbe, PlatformError> {
            let call = probe_calls.get();
            probe_calls.set(call + 1);
            Ok(outcomes.get(call).cloned().unwrap_or(SpawnProbe::Mismatch))
        };
        let resolved = identify_spawned_service_with_probe(service, &mut probe, || true, || {})
            .expect("transient mismatch must be retried to the settled identity");
        assert_eq!(resolved, identity);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn spawned_service_persistent_identity_mismatch_fails_closed() {
        let service = ManagementService::WpaSupplicant;
        let mut probe = || Ok(SpawnProbe::Mismatch);
        let mut attempts = 0;
        let keep_polling = move || {
            attempts += 1;
            attempts < 3
        };
        let error = identify_spawned_service_with_probe(service, &mut probe, keep_polling, || {})
            .expect_err("persistent identity mismatch must fail closed");
        assert!(matches!(error, PlatformError::Conflict(detail)
            if detail.contains("did not have the fixed identity")));
    }

    #[test]
    fn spawned_service_that_never_appears_fails_closed() {
        let service = ManagementService::Dnsmasq;
        let mut probe = || Ok(SpawnProbe::NotReady);
        let error = identify_spawned_service_with_probe(service, &mut probe, || false, || {})
            .expect_err("a service that never appears must fail closed");
        assert!(matches!(error, PlatformError::ProbeFailed(detail)
            if detail.contains("disappeared during startup")));
    }

    #[test]
    fn current_driver_channel_set_contains_reflects_the_board_observed_cold_start_state() {
        // Settled state observed on the board (radio on ch161, STA COMPLETED): upper-5GHz classes
        // carry 161, while the current 20M class only carries 36/40/48 (44 and 165 are in the
        // static capability but not the current set).
        let settled = "class band bw      ch_list\n\
            81 2.4G    20M  1 2 3 4 5 6 7 8 9 10 11\n\
            83 2.4G    40M+ 1 2 3 4 5 6 7\n\
            115   5G    20M  36 40 48\n\
            124   5G    20M  149 153 157 161\n\
            125   5G    20M  149 153 157 161\n\
            128   5G    80M  149 153 157 161\n\
            op_class number:11\n";
        assert!(current_driver_channel_set_contains(settled, 161));
        assert!(current_driver_channel_set_contains(settled, 149));
        assert!(current_driver_channel_set_contains(settled, 6));
        assert!(current_driver_channel_set_contains(settled, 36));
        assert!(!current_driver_channel_set_contains(settled, 44));
        assert!(!current_driver_channel_set_contains(settled, 165));

        // Early cold-start scanning state observed on the board: only 2.4G and lower-5GHz classes
        // are current, so the recorded upper-5GHz channel is not yet usable by hostapd.
        let scanning = "class band bw      ch_list\n\
            81 2.4G    20M  1 2 3 4 5 6 7 8 9 10 11\n\
            115   5G    20M  36 40 48\n\
            op_class number:3\n";
        assert!(!current_driver_channel_set_contains(scanning, 161));
        assert!(current_driver_channel_set_contains(scanning, 6));
    }

    #[test]
    fn radio_ready_gate_retries_transient_unavailability_then_proceeds() {
        let mut ready = {
            let outcomes = [false, false, true];
            let mut index = 0;
            move || -> Result<bool, PlatformError> {
                let outcome = outcomes.get(index).copied().unwrap_or(true);
                index += 1;
                Ok(outcome)
            }
        };
        let mut degrade = || Ok(true);
        wait_for_radio_channel_ready_until(
            &mut ready,
            &mut degrade,
            Instant::now() + Duration::from_secs(60),
            || {},
        )
        .expect(
            "a transiently unavailable radio must be retried until the STA confirms the channel",
        );
    }

    #[test]
    fn radio_ready_gate_fails_closed_when_the_channel_stays_unavailable() {
        let mut ready = || Ok(false);
        let mut degrade = || Ok(false);
        let error =
            wait_for_radio_channel_ready_until(&mut ready, &mut degrade, Instant::now(), || {})
                .expect_err("a channel that never becomes available must fail closed");
        assert!(matches!(error, PlatformError::ProbeFailed(detail)
            if detail.contains("did not become ready")));
    }

    #[test]
    fn radio_ready_gate_degrades_to_the_recorded_channel_when_no_upstream_associates() {
        // With no upstream the STA can never confirm the shared channel; the gate must degrade to
        // the recorded-channel start (legacy LAN-only behavior) when the driver still lists the
        // channel as supported, instead of failing the whole launch.
        let mut ready = || Ok(false);
        let mut degrade = || Ok(true);
        wait_for_radio_channel_ready_until(&mut ready, &mut degrade, Instant::now(), || {})
            .expect("no-upstream must degrade to the recorded channel, not fail");
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
            ManagementService::EthernetUdhcpc,
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
            ManagementService::EthernetUdhcpc,
            &[
                "/sbin/udhcpc".to_owned(),
                "-f".to_owned(),
                "-i".to_owned(),
                "eth0".to_owned(),
            ]
        ));
        assert!(!executable_instance_matches_service(
            ManagementService::EthernetUdhcpc,
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
        let generation = DhcpGeneration::new("dhcp-test-generation".to_owned()).unwrap();
        let ownership = DhcpOwnership::from_lease(DhcpUplink::Wifi, generation.clone(), &lease);
        let encoded = serde_json::to_string(&ownership).unwrap();
        let decoded: DhcpOwnership = serde_json::from_str(&encoded).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded, ownership);
        assert_eq!(decoded.generation, Some(generation));
        let legacy = encoded
            .replace("\"version\":3,", "\"version\":1,")
            .replace("\"generation\":\"dhcp-test-generation\",", "");
        let legacy: DhcpOwnership = serde_json::from_str(&legacy).unwrap();
        legacy.validate().unwrap();
        assert_eq!(legacy.generation, None);
        assert_eq!(
            address_args(DhcpUplink::Wifi, "replace", &ownership.address),
            [
                "-4",
                "address",
                "replace",
                "192.0.2.5/24",
                "broadcast",
                "192.0.2.255",
                "dev",
                "wlan0",
            ]
            .map(str::to_owned)
        );
        assert_eq!(
            address_args(DhcpUplink::Wifi, "del", &ownership.address),
            ["-4", "address", "del", "192.0.2.5/24", "dev", "wlan0"].map(str::to_owned)
        );
        assert_eq!(
            route_args(DhcpUplink::Wifi, "del", &ownership.routes[0]),
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
    fn ethernet_dhcp_transaction_helpers_select_eth0_metric_100_and_eth0_resolver_tags() {
        let lease = crate::application::dhcp::DhcpLease {
            address: "192.0.2.5".parse().unwrap(),
            prefix: 24,
            broadcast: Some("192.0.2.255".parse().unwrap()),
            routers: vec!["192.0.2.1".parse().unwrap()],
            static_routes: Vec::new(),
            dns: vec!["1.1.1.1".parse().unwrap()],
            search: vec!["example.test".to_owned()],
        };
        let generation = DhcpGeneration::new("ethernet-dhcp-test-generation".to_owned()).unwrap();
        let ownership = DhcpOwnership::from_lease(DhcpUplink::Ethernet, generation, &lease);

        assert_eq!(dhcp_uplink_interface(DhcpUplink::Ethernet), "eth0");
        assert_eq!(dhcp_uplink_metric(DhcpUplink::Ethernet), 100);
        assert_eq!(
            ownership.resolver_entries,
            ["search example.test # eth0", "nameserver 1.1.1.1 # eth0"].map(str::to_owned)
        );
        assert_eq!(ownership.routes[0].metric, Some(100));
        assert_eq!(
            address_args(DhcpUplink::Ethernet, "replace", &ownership.address),
            [
                "-4",
                "address",
                "replace",
                "192.0.2.5/24",
                "broadcast",
                "192.0.2.255",
                "dev",
                "eth0",
            ]
            .map(str::to_owned)
        );
        assert_eq!(
            route_args(DhcpUplink::Ethernet, "replace", &ownership.routes[0]),
            [
                "-4",
                "route",
                "replace",
                "default",
                "via",
                "192.0.2.1",
                "dev",
                "eth0",
                "metric",
                "100",
            ]
            .map(str::to_owned)
        );
        assert!(exact_owned_route_line(
            DhcpUplink::Ethernet,
            "default via 192.0.2.1 dev eth0 metric 100",
            &ownership.routes[0]
        ));
        assert!(exact_default_route_line(
            DhcpUplink::Ethernet,
            "default via 192.0.2.1 dev eth0 metric 100",
            &ownership.routes[0]
        ));
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
    fn resolver_reconciliation_is_idempotent_and_preserves_foreign_lines() {
        let owned = vec!["nameserver 1.1.1.1 # wlan0".to_owned()];
        let existing = "nameserver 9.9.9.9\nnameserver 1.1.1.1 # wlan0\n";
        let once = replace_resolver_text(existing, &owned, &owned);
        let twice = replace_resolver_text(&once, &owned, &owned);
        assert_eq!(once, existing);
        assert_eq!(twice, once);
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
        let address = OwnedAddress {
            cidr: "192.0.2.5/24".to_owned(),
            broadcast: Some("192.0.2.255".parse().unwrap()),
        };
        assert!(exact_owned_address_line(
            "3: wlan0 inet 192.0.2.5/24 brd 192.0.2.255 scope global wlan0",
            &address
        ));
        assert!(!exact_owned_address_line(
            "3: wlan0 inet 192.0.2.5/24 brd 192.0.2.127 scope global wlan0",
            &address
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
            DhcpUplink::Wifi,
            "default via 192.0.2.1 dev wlan0 metric 600",
            &route
        ));
        let classless = OwnedRoute {
            destination: "198.51.100.7/24".to_owned(),
            gateway: "192.0.2.1".parse().unwrap(),
            metric: None,
        };
        assert!(exact_owned_route_line(
            DhcpUplink::Wifi,
            "198.51.100.0/24 via 192.0.2.1 dev wlan0",
            &classless
        ));
        assert!(!exact_default_route_line(
            DhcpUplink::Wifi,
            "default via 192.0.2.1 dev wlan0 proto dhcp metric 600",
            &route
        ));
    }
}
