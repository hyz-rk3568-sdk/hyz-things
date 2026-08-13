use super::{
    paths::{MIHOMO_CONTROLLER_SECRET, MIHOMO_RUNTIME_CONFIG},
    process::{FixedOutput, LinuxMihomoFailOpenPlatform, LinuxRouterPlatform, Tool, NETWORK_LOCK},
    storage,
};
use crate::{
    application::ports::{
        ClockPort, CoreIdentity, CoreRecordState, DevicePolicyStorePort, FailOpenPlatformPort,
        FailOpenRetryKind, LifecycleLease, PlatformError, RouterPlatformPort, SystemProbePort,
    },
    domain::{
        device_policy::LanDeviceMac,
        network::{
            NetworkAction, NetworkObserved, OwnedResource, Probe, LAN_ADDRESS, LAN_BRIDGE,
            LAN_MEMBER, ROUTER_FILTER_CHAIN, ROUTER_NAT_CHAIN, WAN_INTERFACE,
        },
        proxy::{
            ProxyAction, ProxyMode, ProxyObserved, MIHOMO_FILTER_CHAIN, MIHOMO_MANGLE_CHAIN,
            MIHOMO_MARK, MIHOMO_ROUTE_TABLE, MIHOMO_RULE_PRIORITY, MIHOMO_TUN_INTERFACE,
        },
        tailscale::TAILSCALE_FORWARD_CHAIN,
    },
};
use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

impl RouterPlatformPort for LinuxRouterPlatform {
    fn acquire_lifecycle_lock(&self) -> Result<LifecycleLease, PlatformError> {
        storage::acquire_lock(NETWORK_LOCK)
    }

    fn release_lifecycle_lock(&self, lease: &LifecycleLease) -> Result<(), PlatformError> {
        storage::release_lock(lease)
    }

    fn apply_network(&self, action: &NetworkAction) -> Result<(), PlatformError> {
        self.apply_network_action(action)
    }

    fn apply_proxy(&self, action: &ProxyAction) -> Result<(), PlatformError> {
        self.apply_proxy_action(action)
    }
}

impl FailOpenPlatformPort for LinuxMihomoFailOpenPlatform {
    fn watcher_role_matches(&self, expected: CoreIdentity) -> Result<bool, PlatformError> {
        if effective_uid()? != 0 {
            return Ok(false);
        }
        let Some(watcher) = self.platform.read_watcher_record()? else {
            return Ok(false);
        };
        let core_record_matches = matches!(
            self.platform.core_record_state_exact(expected)?,
            CoreRecordState::ExpectedLive | CoreRecordState::ExpectedExited
        );
        Ok(core_record_matches
            && watcher.process.pid == std::process::id()
            && watcher.core == expected
            && watcher.matches_live_process()?)
    }

    fn core_record_state(&self, expected: CoreIdentity) -> Result<CoreRecordState, PlatformError> {
        self.platform.core_record_state_exact(expected)
    }

    fn acquire_fail_open_lock(&self) -> Result<LifecycleLease, PlatformError> {
        storage::acquire_lock(NETWORK_LOCK)
    }

    fn release_fail_open_lock(&self, lease: &LifecycleLease) -> Result<(), PlatformError> {
        storage::release_lock(lease)
    }

    fn observe_fail_open_proxy(&self) -> Result<ProxyObserved, PlatformError> {
        self.platform.observe_proxy()
    }

    fn apply_fail_open_proxy(&self, action: &ProxyAction) -> Result<(), PlatformError> {
        self.platform.apply_proxy_action(action)
    }

    fn remove_fail_open_stale_state(&self, expected: CoreIdentity) -> Result<(), PlatformError> {
        if self.platform.core_record_state_exact(expected)? != CoreRecordState::ExpectedExited {
            return Err(PlatformError::Conflict(
                "core identity changed before fail-open stale-state removal".to_owned(),
            ));
        }
        let watcher = self.platform.read_watcher_record()?.ok_or_else(|| {
            PlatformError::Conflict("watcher identity record disappeared".to_owned())
        })?;
        if watcher.process.pid != std::process::id()
            || watcher.core != expected
            || !watcher.matches_live_process()?
        {
            return Err(PlatformError::Conflict(
                "watcher identity changed before fail-open stale-state removal".to_owned(),
            ));
        }
        storage::remove_file_durable(super::process::MIHOMO_PID_RECORD)?;
        storage::remove_file_durable(super::process::MIHOMO_WATCHER_RECORD)?;
        storage::remove_file_durable(MIHOMO_CONTROLLER_SECRET)?;
        storage::remove_file_durable(MIHOMO_RUNTIME_CONFIG)
    }

    fn sleep_fail_open_retry(&self, duration: std::time::Duration) {
        std::thread::sleep(duration);
    }

    fn record_fail_open_retry(&self, kind: FailOpenRetryKind, error: &PlatformError) {
        let _ = super::process::append_fail_open_retry_log(&format!("{kind:?}: {error}"));
    }
}

impl SystemProbePort for LinuxRouterPlatform {
    fn observe_network(&self) -> Result<NetworkObserved, PlatformError> {
        let bridge = self.observe_bridge();
        let bridge_absent = bridge == Probe::Known(OwnedResource::Absent);
        let bridge_up = if bridge_absent {
            Probe::Known(false)
        } else {
            read_trimmed(format!("/sys/class/net/{LAN_BRIDGE}/operstate"))
                .map(|state| state == "up")
        };
        let lan_address_present = if bridge_absent {
            Probe::Known(false)
        } else {
            self.ip_output(&["-o", "-4", "address", "show", "dev", LAN_BRIDGE])
                .map(|output| output.split_whitespace().any(|field| field == LAN_ADDRESS))
        };
        let ap_attached = match fs::read_link(format!("/sys/class/net/{LAN_MEMBER}/master")) {
            Ok(master) => {
                Probe::Known(master.file_name().and_then(|name| name.to_str()) == Some(LAN_BRIDGE))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Probe::Known(false),
            Err(error) => Probe::Unknown(format!("read AP bridge master: {error}")),
        };
        let wan_default_route_present = match self.owned_sta_address_and_route_ready() {
            Ok(ready) => Probe::Known(ready),
            Err(error) => Probe::Unknown(error.to_string()),
        };
        let ipv4_forwarding =
            read_trimmed("/proc/sys/net/ipv4/ip_forward").and_then(|value| match value.as_str() {
                "0" => Probe::Known(false),
                "1" => Probe::Known(true),
                _ => Probe::Unknown("ip_forward value is malformed".to_owned()),
            });
        let previous_ipv4_forwarding =
            match storage::read_small_optional(storage::PREVIOUS_FORWARDING, 8) {
                Ok(None) => Probe::Known(None),
                Ok(Some(value)) => match value.trim() {
                    "0" => Probe::Known(Some(false)),
                    "1" => Probe::Known(Some(true)),
                    _ => Probe::Unknown("saved ip_forward value is malformed".to_owned()),
                },
                Err(error) => Probe::Unknown(error.to_string()),
            };
        Ok(NetworkObserved {
            bridge,
            bridge_up,
            lan_address_present,
            ap_attached,
            management_services_healthy: match self.management_services_ready() {
                Ok(ready) => Probe::Known(ready),
                Err(error) => Probe::Unknown(error.to_string()),
            },
            wan_default_route_present,
            ipv4_forwarding,
            previous_ipv4_forwarding,
            router_firewall: self.observe_router_firewall(),
        })
    }

    fn observe_proxy(&self) -> Result<ProxyObserved, PlatformError> {
        let process_identity_valid = match (self.mihomo_identity(), self.mihomo_process_count()) {
            (Ok(Some(_)), Ok(1)) => Probe::Known(true),
            (Ok(None), Ok(0)) => Probe::Known(false),
            (Ok(Some(_)), Ok(count)) => Probe::Unknown(format!(
                "managed Mihomo identity exists but {count} matching executables are running"
            )),
            (Ok(None), Ok(count)) => Probe::Unknown(format!(
                "{count} Mihomo executable(s) run without a matching PID/start/exe record"
            )),
            (Err(error), _) | (_, Err(error)) => Probe::Unknown(error.to_string()),
        };
        let watcher_identity_valid = match self.read_watcher_record() {
            Ok(None) => Probe::Known(false),
            Ok(Some(watcher)) => match (watcher.matches_live_process(), self.mihomo_identity()) {
                (Ok(true), Ok(Some(core))) if watcher.core == core.core => Probe::Known(true),
                (Ok(true), Ok(Some(_))) => Probe::Unknown(
                    "Mihomo watcher is bound to a different exact core identity".to_owned(),
                ),
                (Ok(true), Ok(None)) => Probe::Unknown(
                    "Mihomo watcher is live without its exact core identity".to_owned(),
                ),
                (Ok(false), _) => Probe::Unknown(
                    "Mihomo watcher record exists without exact live PID/start/exe/argv identity"
                        .to_owned(),
                ),
                (Err(error), _) | (_, Err(error)) => Probe::Unknown(error.to_string()),
            },
            Err(error) => Probe::Unknown(error.to_string()),
        };
        let runtime_config_valid =
            match storage::read_private_small_optional(MIHOMO_RUNTIME_CONFIG, 4 * 1024 * 1024) {
                Ok(Some(_)) => match self.validate_mihomo_config() {
                    Err(PlatformError::CommandFailed(_)) => Probe::Known(false),
                    Ok(()) => match self.mihomo_runtime_config_matches() {
                        Ok(matches) => Probe::Known(matches),
                        Err(error) => Probe::Unknown(error.to_string()),
                    },
                    Err(error) => Probe::Unknown(error.to_string()),
                },
                Ok(None) => Probe::Known(false),
                Err(error) => Probe::Unknown(error.to_string()),
            };
        let tun_interface_present = Probe::Known(
            fs::metadata(format!("/sys/class/net/{MIHOMO_TUN_INTERFACE}/tun_flags")).is_ok(),
        );
        let tun_firewall = self.observe_tun_firewall();
        let policy_rule_present = self
            .ip_output(&["-4", "rule", "show"])
            .and_then(|output| policy_rule_probe(&output));
        let policy_route_present = match self.run_probe(
            Tool::Ip,
            &strings(&[
                "-4",
                "route",
                "show",
                "table",
                &MIHOMO_ROUTE_TABLE.to_string(),
            ]),
        ) {
            Ok(output) => policy_route_command_probe(&output),
            Err(error) => Probe::Unknown(error.to_string()),
        };
        let interception_entry_present = match &tun_firewall {
            Probe::Known(OwnedResource::Owned { token }) => self
                .iptables_output(&["-w", "-t", "mangle", "-S"])
                .map(|output| {
                    let Some(rules) = normalized_chain_rules(&output, "PREROUTING") else {
                        return false;
                    };
                    let comment = token.to_owned();
                    let expected = expected_interception_rule(&comment);
                    let references = exact_chain_references(&output, MIHOMO_MANGLE_CHAIN);
                    let commented = rules
                        .iter()
                        .filter(|rule| rule.iter().any(|word| word == &comment))
                        .collect::<Vec<_>>();
                    references == Some(vec![expected.clone()])
                        && commented.len() == 1
                        && *commented[0] == expected
                        && rules.first() == Some(&expected)
                }),
            Probe::Known(OwnedResource::Absent) => self
                .iptables_output(&["-w", "-t", "mangle", "-S"])
                .map(|output| {
                    exact_chain_references(&output, MIHOMO_MANGLE_CHAIN)
                        .is_some_and(|references| !references.is_empty())
                }),
            Probe::Known(OwnedResource::Foreign) => Probe::Known(true),
            Probe::Unknown(reason) => Probe::Unknown(reason.clone()),
        };
        let mode_file = storage::read_private_small_optional(storage::MODE_FILE, 32);
        let disabled_marker = storage::read_private_small_optional(storage::DISABLED_MARKER, 32);
        let persisted_mode = match (mode_file, disabled_marker) {
            (_, Ok(Some(_))) => Probe::Known(Some(ProxyMode::Disabled)),
            (_, Err(error)) => Probe::Unknown(error.to_string()),
            (Err(error), Ok(None)) => Probe::Unknown(error.to_string()),
            (Ok(None), Ok(None)) => Probe::Known(None),
            (Ok(Some(mode)), Ok(None)) => match mode.trim() {
                "explicit" => Probe::Known(Some(ProxyMode::Explicit)),
                "tun" => Probe::Known(Some(ProxyMode::Tun)),
                _ => Probe::Unknown("persisted proxy mode is invalid".to_owned()),
            },
        };
        let router_firewall = self.observe_router_firewall();
        let ordinary_nat_confirmed = match (
            &router_firewall,
            &tun_firewall,
            &policy_rule_present,
            &interception_entry_present,
        ) {
            (
                Probe::Known(OwnedResource::Owned { .. }),
                Probe::Known(OwnedResource::Absent),
                Probe::Known(false),
                Probe::Known(false),
            ) => Probe::Known(true),
            (Probe::Unknown(reason), _, _, _)
            | (_, Probe::Unknown(reason), _, _)
            | (_, _, Probe::Unknown(reason), _)
            | (_, _, _, Probe::Unknown(reason)) => Probe::Unknown(reason.clone()),
            _ => Probe::Known(false),
        };
        let active_direct_macs = self.observe_active_direct_macs(&tun_firewall);
        Ok(ProxyObserved {
            persisted_mode,
            process_identity_valid,
            watcher_identity_valid,
            runtime_config_valid,
            tun_interface_present,
            tun_firewall,
            policy_rule_present,
            policy_route_present,
            interception_entry_present,
            ordinary_nat_confirmed,
            active_direct_macs,
        })
    }
}

impl ClockPort for LinuxRouterPlatform {
    fn unix_time_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }

    fn ownership_token(&self, prefix: &str) -> Result<String, PlatformError> {
        let uuid = fs::read_to_string("/proc/sys/kernel/random/uuid")
            .map_err(|error| PlatformError::Io(format!("read kernel UUID: {error}")))?;
        let uuid = uuid.trim();
        if uuid.is_empty()
            || !uuid
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
        {
            return Err(PlatformError::InvalidState(
                "kernel UUID has invalid characters".to_owned(),
            ));
        }
        let token = format!("{prefix}-{uuid}");
        storage::validate_token(&token)?;
        Ok(token)
    }
}

impl LinuxRouterPlatform {
    fn observe_bridge(&self) -> Probe<OwnedResource> {
        let interface = Path::new("/sys/class/net").join(LAN_BRIDGE);
        let marker = match storage::read_small_optional(storage::BRIDGE_OWNER, 256) {
            Ok(marker) => marker,
            Err(error) => return Probe::Unknown(error.to_string()),
        };
        if !interface.exists() {
            return if marker.is_none() {
                Probe::Known(OwnedResource::Absent)
            } else {
                Probe::Unknown("bridge marker exists but interface is absent".to_owned())
            };
        }
        if !interface.join("bridge").exists() {
            return Probe::Known(OwnedResource::Foreign);
        }
        let Some(marker) = marker else {
            return Probe::Known(OwnedResource::Foreign);
        };
        let mut lines = marker.lines();
        let Some(token) = lines.next() else {
            return Probe::Unknown("bridge ownership marker is empty".to_owned());
        };
        if storage::validate_token(token).is_err() {
            return Probe::Unknown("bridge ownership token is invalid".to_owned());
        }
        let expected = lines.next().and_then(|value| value.parse::<u32>().ok());
        let actual = read_trimmed(interface.join("ifindex"))
            .known()
            .and_then(|value| value.parse::<u32>().ok());
        if expected.is_none() || actual.is_none() {
            Probe::Unknown("bridge ifindex ownership cannot be verified".to_owned())
        } else if expected != actual {
            Probe::Known(OwnedResource::Foreign)
        } else {
            Probe::Known(OwnedResource::Owned {
                token: token.to_owned(),
            })
        }
    }

    pub(crate) fn allowed_direct_mac_sets(
        &self,
    ) -> Result<Vec<BTreeSet<LanDeviceMac>>, PlatformError> {
        let committed = self.load_device_policy()?;
        let mut allowed = vec![committed.direct_macs()];
        if let Some((previous, candidate)) = self.load_pending_device_policy()? {
            allowed.push(previous.direct_macs());
            allowed.push(candidate.direct_macs());
        }
        allowed.sort();
        allowed.dedup();
        Ok(allowed)
    }

    fn observe_active_direct_macs(
        &self,
        ownership: &Probe<OwnedResource>,
    ) -> Probe<BTreeSet<LanDeviceMac>> {
        match ownership {
            Probe::Known(OwnedResource::Absent) => Probe::Known(BTreeSet::new()),
            Probe::Known(OwnedResource::Owned { .. }) => self
                .iptables_output(&["-w", "-t", "mangle", "-S", MIHOMO_MANGLE_CHAIN])
                .and_then(|output| {
                    parse_direct_mac_rules(&output, MIHOMO_MANGLE_CHAIN).map_or_else(
                        || {
                            Probe::Unknown(
                                "direct MAC rules are malformed or noncanonical".to_owned(),
                            )
                        },
                        Probe::Known,
                    )
                }),
            Probe::Known(OwnedResource::Foreign) => {
                Probe::Unknown("direct MAC rules belong to foreign TUN chains".to_owned())
            }
            Probe::Unknown(reason) => Probe::Unknown(reason.clone()),
        }
    }

    fn observe_tun_firewall(&self) -> Probe<OwnedResource> {
        let filter = self.observe_owned_chain(
            "filter",
            MIHOMO_FILTER_CHAIN,
            storage::TUN_FIREWALL_OWNER,
            "hyz-mihomo-owner:",
        );
        let mangle = self.observe_owned_chain(
            "mangle",
            MIHOMO_MANGLE_CHAIN,
            storage::TUN_FIREWALL_OWNER,
            "hyz-mihomo-owner:",
        );
        match (filter, mangle) {
            (Probe::Known(OwnedResource::Absent), Probe::Known(OwnedResource::Absent)) => {
                Probe::Known(OwnedResource::Absent)
            }
            (
                Probe::Known(OwnedResource::Owned { token: left }),
                Probe::Known(OwnedResource::Owned { token: right }),
            ) if left == right => match (
                self.observe_hook("filter", "FORWARD", left.as_str(), MIHOMO_FILTER_CHAIN),
                self.observe_shared_forward_hook_order(),
            ) {
                (Probe::Known(true), Probe::Known(true)) => {
                    Probe::Known(OwnedResource::Owned { token: left })
                }
                (Probe::Unknown(reason), _) | (_, Probe::Unknown(reason)) => Probe::Unknown(reason),
                _ => Probe::Known(OwnedResource::Foreign),
            },
            (Probe::Unknown(reason), _) | (_, Probe::Unknown(reason)) => Probe::Unknown(reason),
            _ => Probe::Known(OwnedResource::Foreign),
        }
    }

    fn observe_router_firewall(&self) -> Probe<OwnedResource> {
        let filter = self.observe_owned_chain(
            "filter",
            ROUTER_FILTER_CHAIN,
            storage::ROUTER_FIREWALL_OWNER,
            "hyz-router-owner:",
        );
        let nat = self.observe_owned_chain(
            "nat",
            ROUTER_NAT_CHAIN,
            storage::ROUTER_FIREWALL_OWNER,
            "hyz-router-owner:",
        );
        match (filter, nat) {
            (Probe::Known(OwnedResource::Absent), Probe::Known(OwnedResource::Absent)) => {
                Probe::Known(OwnedResource::Absent)
            }
            (
                Probe::Known(OwnedResource::Owned { token: left }),
                Probe::Known(OwnedResource::Owned { token: right }),
            ) if left == right => {
                let filter_hook =
                    self.observe_hook("filter", "FORWARD", left.as_str(), ROUTER_FILTER_CHAIN);
                let nat_hook =
                    self.observe_hook("nat", "POSTROUTING", left.as_str(), ROUTER_NAT_CHAIN);
                match (filter_hook, nat_hook) {
                    (Probe::Known(true), Probe::Known(true)) => {
                        match self.observe_router_hook_order() {
                            Probe::Known(true) => {
                                Probe::Known(OwnedResource::Owned { token: left })
                            }
                            Probe::Known(false) => Probe::Known(OwnedResource::Foreign),
                            Probe::Unknown(reason) => Probe::Unknown(reason),
                        }
                    }
                    (Probe::Unknown(reason), _) | (_, Probe::Unknown(reason)) => {
                        Probe::Unknown(reason)
                    }
                    _ => Probe::Known(OwnedResource::Foreign),
                }
            }
            (Probe::Unknown(reason), _) | (_, Probe::Unknown(reason)) => Probe::Unknown(reason),
            _ => Probe::Known(OwnedResource::Foreign),
        }
    }

    fn observe_router_hook_order(&self) -> Probe<bool> {
        let forward = self.observe_shared_forward_hook_order();
        let nat = self.iptables_output(&["-w", "-t", "nat", "-S", "POSTROUTING"]);
        match (forward, nat) {
            (Probe::Known(forward), Probe::Known(nat)) => {
                Probe::Known(forward && owned_jump_is_first(&nat, "POSTROUTING", ROUTER_NAT_CHAIN))
            }
            (Probe::Unknown(reason), _) | (_, Probe::Unknown(reason)) => Probe::Unknown(reason),
        }
    }

    fn observe_shared_forward_hook_order(&self) -> Probe<bool> {
        let output = match self.iptables_output(&["-w", "-t", "filter", "-S", "FORWARD"]) {
            Probe::Known(output) => output,
            Probe::Unknown(reason) => return Probe::Unknown(reason),
        };
        let mihomo =
            owned_forward_hook_is_exact(&output, storage::TUN_FIREWALL_OWNER, MIHOMO_FILTER_CHAIN);
        let tailscale = owned_forward_hook_is_exact(
            &output,
            storage::TAILSCALE_FIREWALL_OWNER,
            TAILSCALE_FORWARD_CHAIN,
        );
        let router = owned_forward_hook_is_exact(
            &output,
            storage::ROUTER_FIREWALL_OWNER,
            ROUTER_FILTER_CHAIN,
        );
        match (mihomo, tailscale, router) {
            (Ok(mihomo), Ok(tailscale), Ok(router)) => Probe::Known(forward_hook_order_is_exact(
                &output, mihomo, tailscale, router,
            )),
            (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
                Probe::Unknown(error.to_string())
            }
        }
    }

    fn observe_hook(&self, table: &str, parent: &str, comment: &str, jump: &str) -> Probe<bool> {
        self.iptables_output(&["-w", "-t", table, "-S"])
            .map(|output| {
                let Some(rules) = normalized_chain_rules(&output, parent) else {
                    return false;
                };
                let expected = words(&[
                    "-A",
                    parent,
                    "-m",
                    "comment",
                    "--comment",
                    comment,
                    "-j",
                    jump,
                ]);
                let references = exact_chain_references(&output, jump);
                let commented = rules
                    .iter()
                    .filter(|rule| rule.iter().any(|word| word == comment))
                    .collect::<Vec<_>>();
                references == Some(vec![expected.clone()])
                    && commented.len() == 1
                    && *commented[0] == expected
            })
    }

    fn observe_owned_chain(
        &self,
        table: &str,
        chain: &str,
        marker_path: &str,
        _comment_prefix: &str,
    ) -> Probe<OwnedResource> {
        let output =
            match self.run_probe(Tool::Iptables, &strings(&["-w", "-t", table, "-S", chain])) {
                Ok(output) => output,
                Err(error) => return Probe::Unknown(error.to_string()),
            };
        let marker = match storage::read_private_small_optional(marker_path, 128) {
            Ok(marker) => marker,
            Err(error) => return Probe::Unknown(error.to_string()),
        };
        if !output.success {
            if output.stderr.contains("No chain/target/match by that name") && marker.is_none() {
                return Probe::Known(OwnedResource::Absent);
            }
            return Probe::Unknown(format!(
                "cannot classify {table}/{chain}: {}",
                output.stderr.trim()
            ));
        }
        let Some(token) = marker else {
            return Probe::Known(OwnedResource::Foreign);
        };
        let token = token.trim();
        if storage::validate_token(token).is_err() {
            return Probe::Known(OwnedResource::Foreign);
        }
        let gateway = if chain == MIHOMO_MANGLE_CHAIN {
            match self.ip_output(&["-4", "route", "show", "default", "dev", WAN_INTERFACE]) {
                Probe::Known(output) => match exact_default_gateway(&output) {
                    Some(gateway) => Some(gateway.to_owned()),
                    None => {
                        return Probe::Unknown(
                            "wlan0 default gateway is not exactly and uniquely identifiable"
                                .to_owned(),
                        )
                    }
                },
                Probe::Unknown(reason) => return Probe::Unknown(reason),
            }
        } else {
            None
        };
        let direct_macs = if chain == MIHOMO_MANGLE_CHAIN {
            let parsed = match parse_direct_mac_rules(&output.stdout, chain) {
                Some(macs) => macs,
                None => return Probe::Known(OwnedResource::Foreign),
            };
            let allowed = match self.allowed_direct_mac_sets() {
                Ok(allowed) => allowed,
                Err(error) => return Probe::Unknown(error.to_string()),
            };
            if !direct_mac_set_is_allowed(&parsed, &allowed) {
                return Probe::Known(OwnedResource::Foreign);
            }
            parsed
        } else {
            BTreeSet::new()
        };
        let expected =
            expected_chain_rules_with_direct(chain, token, gateway.as_deref(), &direct_macs);
        if !chain_output_is_exact(&output.stdout, chain, &expected) {
            Probe::Known(OwnedResource::Foreign)
        } else {
            Probe::Known(OwnedResource::Owned {
                token: token.to_owned(),
            })
        }
    }

    fn ip_output(&self, args: &[&str]) -> Probe<String> {
        match self.run_probe(Tool::Ip, &strings(args)) {
            Ok(output) if output.success => Probe::Known(output.stdout),
            Ok(output) => Probe::Unknown(format!("ip probe failed: {}", output.stderr.trim())),
            Err(error) => Probe::Unknown(error.to_string()),
        }
    }

    fn iptables_output(&self, args: &[&str]) -> Probe<String> {
        match self.run_probe(Tool::Iptables, &strings(args)) {
            Ok(output) if output.success => Probe::Known(output.stdout),
            Ok(output) => {
                Probe::Unknown(format!("iptables probe failed: {}", output.stderr.trim()))
            }
            Err(error) => Probe::Unknown(error.to_string()),
        }
    }
}

pub(crate) fn expected_interception_rule(token: &str) -> Vec<String> {
    words(&[
        "-A",
        "PREROUTING",
        "-s",
        crate::domain::network::LAN_SUBNET,
        "-i",
        LAN_BRIDGE,
        "-m",
        "comment",
        "--comment",
        token,
        "-j",
        MIHOMO_MANGLE_CHAIN,
    ])
}

pub(crate) fn expected_chain_rules(
    chain: &str,
    token: &str,
    gateway: Option<&str>,
) -> Vec<Vec<String>> {
    expected_chain_rules_with_direct(chain, token, gateway, &BTreeSet::new())
}

pub(crate) fn expected_chain_rules_with_direct(
    chain: &str,
    token: &str,
    gateway: Option<&str>,
    direct_macs: &BTreeSet<LanDeviceMac>,
) -> Vec<Vec<String>> {
    let mut rules = Vec::new();
    let mut push = |body: &[&str]| {
        let mut rule = words(&["-A", chain]);
        rule.extend(words(body));
        rules.push(rule);
    };
    match chain {
        ROUTER_FILTER_CHAIN => {
            push(&["-m", "comment", "--comment", token]);
            push(&[
                "-s",
                crate::domain::network::LAN_SUBNET,
                "-i",
                LAN_BRIDGE,
                "-o",
                WAN_INTERFACE,
                "-m",
                "conntrack",
                "--ctstate",
                "NEW,RELATED,ESTABLISHED",
                "-j",
                "ACCEPT",
            ]);
            push(&[
                "-d",
                crate::domain::network::LAN_SUBNET,
                "-i",
                WAN_INTERFACE,
                "-o",
                LAN_BRIDGE,
                "-m",
                "conntrack",
                "--ctstate",
                "RELATED,ESTABLISHED",
                "-j",
                "ACCEPT",
            ]);
            push(&["-i", WAN_INTERFACE, "-o", LAN_BRIDGE, "-j", "DROP"]);
            push(&["-i", LAN_BRIDGE, "-j", "DROP"]);
        }
        ROUTER_NAT_CHAIN => {
            push(&["-m", "comment", "--comment", token]);
            push(&[
                "-s",
                crate::domain::network::LAN_SUBNET,
                "-o",
                WAN_INTERFACE,
                "-j",
                "MASQUERADE",
            ]);
        }
        MIHOMO_MANGLE_CHAIN => {
            push(&["-m", "comment", "--comment", token]);
            for subnet in [
                crate::domain::network::LAN_SUBNET,
                "0.0.0.0/8",
                "10.0.0.0/8",
                "100.64.0.0/10",
                "127.0.0.0/8",
                "169.254.0.0/16",
                "172.16.0.0/12",
                "192.0.0.0/24",
                "192.0.2.0/24",
                "192.168.0.0/16",
                "198.18.0.0/15",
                "198.51.100.0/24",
                "203.0.113.0/24",
                "224.0.0.0/4",
                "240.0.0.0/4",
            ] {
                push(&["-d", subnet, "-j", "RETURN"]);
            }
            if let Some(gateway) = gateway {
                push(&["-d", &format!("{gateway}/32"), "-j", "RETURN"]);
            }
            push(&["-p", "udp", "-m", "udp", "--dport", "67:68", "-j", "RETURN"]);
            for protocol in ["tcp", "udp"] {
                push(&[
                    "-p", protocol, "-m", protocol, "--dport", "53", "-j", "RETURN",
                ]);
            }
            for mac in direct_macs {
                let mac = mac.to_string();
                push(&["-m", "mac", "--mac-source", &mac, "-j", "RETURN"]);
            }
            for protocol in ["tcp", "udp"] {
                push(&["-p", protocol, "-j", "MARK", "--set-xmark", MIHOMO_MARK]);
            }
        }
        MIHOMO_FILTER_CHAIN => {
            push(&["-m", "comment", "--comment", token]);
            for protocol in ["tcp", "udp"] {
                push(&[
                    "-s",
                    crate::domain::network::LAN_SUBNET,
                    "-i",
                    LAN_BRIDGE,
                    "-o",
                    MIHOMO_TUN_INTERFACE,
                    "-p",
                    protocol,
                    "-j",
                    "ACCEPT",
                ]);
            }
            push(&[
                "-d",
                crate::domain::network::LAN_SUBNET,
                "-i",
                MIHOMO_TUN_INTERFACE,
                "-o",
                LAN_BRIDGE,
                "-m",
                "conntrack",
                "--ctstate",
                "RELATED,ESTABLISHED",
                "-j",
                "ACCEPT",
            ]);
            push(&["-j", "RETURN"]);
        }
        _ => {}
    }
    rules
}

fn direct_mac_set_is_allowed(
    parsed: &BTreeSet<LanDeviceMac>,
    allowed: &[BTreeSet<LanDeviceMac>],
) -> bool {
    allowed.iter().any(|macs| macs == parsed)
}

pub(crate) fn parse_direct_mac_rules(output: &str, chain: &str) -> Option<BTreeSet<LanDeviceMac>> {
    let rules = normalized_chain_rules(output, chain)?;
    let mut macs = BTreeSet::new();
    for rule in rules {
        if rule.len() == 8
            && rule[0] == "-A"
            && rule[1] == chain
            && rule[2..5] == ["-m", "mac", "--mac-source"]
            && rule[6..] == ["-j", "RETURN"]
        {
            let mac = rule[5].parse().ok()?;
            if !macs.insert(mac) {
                return None;
            }
        }
    }
    Some(macs)
}

pub(crate) fn normalized_chain_rules(output: &str, chain: &str) -> Option<Vec<Vec<String>>> {
    output
        .lines()
        .filter(|line| line.starts_with("-A "))
        .map(normalized_rule)
        .collect::<Option<Vec<_>>>()
        .map(|rules| {
            rules
                .into_iter()
                .filter(|rule| rule.get(1).map(String::as_str) == Some(chain))
                .collect()
        })
}

fn normalized_rule(line: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in line.chars() {
        if escaped {
            word.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if quote == Some(character) {
            quote = None;
        } else if quote.is_none() && matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if quote.is_none() && character.is_whitespace() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else {
            word.push(character);
        }
    }
    if escaped || quote.is_some() {
        return None;
    }
    if !word.is_empty() {
        words.push(word);
    }
    Some(words)
}

pub(crate) fn exact_default_gateway(routes: &str) -> Option<&str> {
    let lines = routes
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.len() != 1 {
        return None;
    }
    let fields = lines[0].split_whitespace().collect::<Vec<_>>();
    if fields.first() != Some(&"default") {
        return None;
    }
    let gateways = fields
        .windows(2)
        .filter(|pair| pair[0] == "via")
        .map(|pair| pair[1])
        .collect::<Vec<_>>();
    if gateways.len() == 1 {
        Some(gateways[0])
    } else {
        None
    }
}

fn policy_rule_probe(output: &str) -> Probe<bool> {
    let priority = format!("{MIHOMO_RULE_PRIORITY}:");
    let count = output
        .lines()
        .filter(|line| line.split_whitespace().next() == Some(priority.as_str()))
        .count();
    if count == 0 {
        Probe::Known(false)
    } else if policy_rule_state_is_exact(output) {
        Probe::Known(true)
    } else {
        Probe::Unknown("managed policy-rule priority is duplicate or conflicting".to_owned())
    }
}

fn policy_route_command_probe(output: &FixedOutput) -> Probe<bool> {
    if output.success {
        return policy_route_probe(&output.stdout);
    }
    // iproute2 5.14 reports an absent numeric IPv4 table as rc=2 with this exact stderr and no
    // stdout on the target kernel. An empty table is the desired clean state, not an unknown one.
    if output.stdout.is_empty() && output.stderr.trim() == "Dump terminated" {
        Probe::Known(false)
    } else {
        Probe::Unknown(format!(
            "policy-route probe failed: {}",
            output.stderr.trim()
        ))
    }
}

fn policy_route_probe(output: &str) -> Probe<bool> {
    if output.lines().all(|line| line.trim().is_empty()) {
        Probe::Known(false)
    } else if policy_route_state_is_exact(output) {
        Probe::Known(true)
    } else {
        Probe::Unknown("managed policy table is ambiguous or contains extra routes".to_owned())
    }
}

pub(crate) fn policy_rule_state_is_exact(output: &str) -> bool {
    let priority = format!("{MIHOMO_RULE_PRIORITY}:");
    let table = MIHOMO_ROUTE_TABLE.to_string();
    let expected = [
        priority.as_str(),
        "from",
        "all",
        "fwmark",
        MIHOMO_MARK,
        "lookup",
        table.as_str(),
    ];
    let at_priority = output
        .lines()
        .filter(|line| line.split_whitespace().next() == Some(priority.as_str()))
        .collect::<Vec<_>>();
    at_priority.len() == 1
        && at_priority[0]
            .split_whitespace()
            .eq(expected.iter().copied())
}

pub(crate) fn policy_route_state_is_exact(output: &str) -> bool {
    let lines = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    lines.len() == 1
        && lines[0]
            .split_whitespace()
            .eq(["default", "dev", MIHOMO_TUN_INTERFACE, "scope", "link"])
}

pub(crate) fn chain_output_is_exact(output: &str, chain: &str, expected: &[Vec<String>]) -> bool {
    let declarations = output
        .lines()
        .filter(|line| line.starts_with("-N "))
        .map(normalized_rule)
        .collect::<Option<Vec<_>>>();
    declarations == Some(vec![words(&["-N", chain])])
        && normalized_chain_rules(output, chain).as_deref() == Some(expected)
}

pub(crate) fn exact_chain_references(output: &str, jump: &str) -> Option<Vec<Vec<String>>> {
    output
        .lines()
        .filter(|line| line.starts_with("-A "))
        .map(normalized_rule)
        .collect::<Option<Vec<_>>>()
        .map(|rules| {
            rules
                .into_iter()
                .filter(|rule| rule.windows(2).any(|pair| pair == ["-j", jump]))
                .collect()
        })
}

pub(crate) fn owned_forward_hook_is_exact(
    output: &str,
    marker_path: &str,
    chain: &str,
) -> Result<bool, PlatformError> {
    let marker = storage::read_private_small_optional(marker_path, 128)?;
    let references = exact_chain_references(output, chain).ok_or_else(|| {
        PlatformError::ProbeFailed(format!("cannot parse {chain} FORWARD references"))
    })?;
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
                    "{chain} FORWARD hook is not exact and unique"
                )));
            }
            Ok(true)
        }
        None if references.is_empty() => Ok(false),
        None => Err(PlatformError::Conflict(format!(
            "unowned {chain} FORWARD references are present"
        ))),
    }
}

pub fn forward_hook_order_is_exact(
    output: &str,
    mihomo_present: bool,
    tailscale_present: bool,
    router_present: bool,
) -> bool {
    let Some(rules) = normalized_chain_rules(output, "FORWARD") else {
        return false;
    };
    let expected = [
        (mihomo_present, MIHOMO_FILTER_CHAIN),
        (tailscale_present, TAILSCALE_FORWARD_CHAIN),
        (router_present, ROUTER_FILTER_CHAIN),
    ]
    .into_iter()
    .filter_map(|(present, chain)| present.then_some(chain))
    .collect::<Vec<_>>();
    for (_, chain) in [
        (mihomo_present, MIHOMO_FILTER_CHAIN),
        (tailscale_present, TAILSCALE_FORWARD_CHAIN),
        (router_present, ROUTER_FILTER_CHAIN),
    ] {
        let positions = rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.windows(2).any(|pair| pair == ["-j", chain]))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let wanted = expected.iter().position(|expected| *expected == chain);
        match wanted {
            Some(position) if positions == [position] => {}
            None if positions.is_empty() => {}
            _ => return false,
        }
    }
    true
}

fn owned_jump_is_first(output: &str, parent: &str, jump: &str) -> bool {
    let Some(rules) = normalized_chain_rules(output, parent) else {
        return false;
    };
    let positions = rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.windows(2).any(|pair| pair == ["-j", jump]))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    positions == [0]
}

fn words(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn effective_uid() -> Result<u32, PlatformError> {
    let status = fs::read_to_string("/proc/self/status")
        .map_err(|error| PlatformError::ProbeFailed(format!("read self status: {error}")))?;
    let uid = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|values| values.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| PlatformError::ProbeFailed("effective UID is unavailable".to_owned()))?;
    Ok(uid)
}

fn read_trimmed(path: impl AsRef<Path>) -> Probe<String> {
    match fs::read_to_string(path) {
        Ok(value) => Probe::Known(value.trim().to_owned()),
        Err(error) => Probe::Unknown(error.to_string()),
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_mac_rules_are_sorted_before_mark_and_strictly_parsed() {
        let direct_macs = [
            "02:00:00:00:00:02".parse().unwrap(),
            "02:00:00:00:00:01".parse().unwrap(),
        ]
        .into_iter()
        .collect();
        let rules = expected_chain_rules_with_direct(
            MIHOMO_MANGLE_CHAIN,
            "owned",
            Some("192.0.2.1"),
            &direct_macs,
        );
        let first_mac = rules
            .iter()
            .position(|rule| rule.iter().any(|word| word == "--mac-source"))
            .unwrap();
        let first_mark = rules
            .iter()
            .position(|rule| rule.iter().any(|word| word == "MARK"))
            .unwrap();
        assert!(first_mac < first_mark);
        let mut output = format!("-N {MIHOMO_MANGLE_CHAIN}\n");
        for rule in &rules {
            output.push_str(&format!("{}\n", rule.join(" ")));
        }
        assert_eq!(
            parse_direct_mac_rules(&output, MIHOMO_MANGLE_CHAIN),
            Some(direct_macs.clone())
        );
        assert!(direct_mac_set_is_allowed(
            &direct_macs,
            std::slice::from_ref(&direct_macs)
        ));
        let edited = ["02:00:00:00:00:03".parse().unwrap()].into_iter().collect();
        assert!(!direct_mac_set_is_allowed(
            &edited,
            std::slice::from_ref(&direct_macs)
        ));
        assert!(parse_direct_mac_rules(
            &format!("{output}-A {MIHOMO_MANGLE_CHAIN} -m mac --mac-source 02:00:00:00:00:01 -j RETURN\n"),
            MIHOMO_MANGLE_CHAIN,
        )
        .is_none());
    }

    #[test]
    fn absent_kernel_policy_table_is_known_false_but_other_probe_failures_are_unknown() {
        assert_eq!(
            policy_route_command_probe(&FixedOutput {
                success: false,
                stdout: String::new(),
                stderr: "Dump terminated\n".to_owned(),
            }),
            Probe::Known(false)
        );
        assert!(matches!(
            policy_route_command_probe(&FixedOutput {
                success: false,
                stdout: String::new(),
                stderr: "RTNETLINK answers: permission denied\n".to_owned(),
            }),
            Probe::Unknown(_)
        ));
    }

    #[test]
    fn normalized_rules_require_exact_tokens_and_shared_hook_order() {
        let unrelated = "-A FORWARD -j UNRELATED\n";
        for (mihomo, tailscale, router, managed) in [
            (false, false, false, ""),
            (
                false,
                false,
                true,
                "-A FORWARD -m comment --comment r -j HYZ_ROUTER_FWD\n",
            ),
            (
                true,
                false,
                true,
                "-A FORWARD -m comment --comment m -j HYZ_MIHOMO_FWD\n-A FORWARD -m comment --comment r -j HYZ_ROUTER_FWD\n",
            ),
            (
                false,
                true,
                true,
                "-A FORWARD -m comment --comment t -j HYZ_TS_FWD\n-A FORWARD -m comment --comment r -j HYZ_ROUTER_FWD\n",
            ),
            (
                true,
                true,
                true,
                "-A FORWARD -m comment --comment m -j HYZ_MIHOMO_FWD\n-A FORWARD -m comment --comment t -j HYZ_TS_FWD\n-A FORWARD -m comment --comment r -j HYZ_ROUTER_FWD\n",
            ),
        ] {
            let output = format!("-P FORWARD ACCEPT\n{managed}{unrelated}");
            assert!(
                forward_hook_order_is_exact(&output, mihomo, tailscale, router),
                "{output}"
            );
        }
        assert_ne!(
            normalized_rule(
                "-A FORWARD -m comment --comment \"hyz-mihomo-forward:t\" -s 1.2.3.4 -j HYZ_MIHOMO_FWD",
            ),
            Some(words(&[
                "-A", "FORWARD", "-m", "comment", "--comment", "hyz-mihomo-forward:t",
                "-j", MIHOMO_FILTER_CHAIN,
            ])),
        );
        for invalid in [
            "-A FORWARD -j HYZ_ROUTER_FWD\n-A FORWARD -j HYZ_TS_FWD\n",
            "-A FORWARD -j HYZ_TS_FWD\n-A FORWARD -j HYZ_MIHOMO_FWD\n-A FORWARD -j HYZ_ROUTER_FWD\n",
            "-A FORWARD -j HYZ_MIHOMO_FWD\n-A FORWARD -j HYZ_TS_FWD\n-A FORWARD -j HYZ_TS_FWD\n-A FORWARD -j HYZ_ROUTER_FWD\n",
        ] {
            assert!(!forward_hook_order_is_exact(invalid, true, true, true));
        }
    }

    #[test]
    fn router_chain_golden_matches_validated_sentinel_and_rule_order() {
        let token = "hyz-router-00000000-0000-0000-0000-000000000000";
        let filter = expected_chain_rules(ROUTER_FILTER_CHAIN, token, None);
        assert_eq!(
            filter.first(),
            Some(&words(&[
                "-A",
                ROUTER_FILTER_CHAIN,
                "-m",
                "comment",
                "--comment",
                token
            ]))
        );
        assert!(filter[1]
            .iter()
            .any(|word| word == "NEW,RELATED,ESTABLISHED"));
        assert!(filter[2].iter().any(|word| word == "RELATED,ESTABLISHED"));
        let board_output = format!(
            "-N {ROUTER_FILTER_CHAIN}\n\
-A {ROUTER_FILTER_CHAIN} -m comment --comment {token}\n\
-A {ROUTER_FILTER_CHAIN} -s 192.168.8.0/24 -i br-lan -o wlan0 -m conntrack --ctstate NEW,RELATED,ESTABLISHED -j ACCEPT\n\
-A {ROUTER_FILTER_CHAIN} -d 192.168.8.0/24 -i wlan0 -o br-lan -m conntrack --ctstate RELATED,ESTABLISHED -j ACCEPT\n\
-A {ROUTER_FILTER_CHAIN} -i wlan0 -o br-lan -j DROP\n\
-A {ROUTER_FILTER_CHAIN} -i br-lan -j DROP\n"
        );
        assert!(chain_output_is_exact(
            &board_output,
            ROUTER_FILTER_CHAIN,
            &filter
        ));
        assert_eq!(
            &filter[filter.len() - 2..],
            &[
                words(&[
                    "-A",
                    ROUTER_FILTER_CHAIN,
                    "-i",
                    WAN_INTERFACE,
                    "-o",
                    LAN_BRIDGE,
                    "-j",
                    "DROP"
                ]),
                words(&["-A", ROUTER_FILTER_CHAIN, "-i", LAN_BRIDGE, "-j", "DROP"]),
            ]
        );
        let nat = expected_chain_rules(ROUTER_NAT_CHAIN, token, None);
        assert_eq!(nat.len(), 2);
        assert_eq!(
            nat.first(),
            Some(&words(&[
                "-A",
                ROUTER_NAT_CHAIN,
                "-m",
                "comment",
                "--comment",
                token
            ]))
        );
    }

    #[test]
    fn policy_readiness_golden_requires_unique_exact_rule_and_route_table() {
        assert!(policy_rule_state_is_exact(
            "0: from all lookup local\n11000: from all fwmark 0x1000000/0x1000000 lookup 110\n32766: from all lookup main\n"
        ));
        assert!(policy_route_state_is_exact(
            "default dev hyz-mihomo scope link\n"
        ));
    }

    #[test]
    fn policy_readiness_refuses_duplicate_ambiguous_and_extra_state() {
        for rules in [
            "11000: from all fwmark 0x1000000/0x1000000 lookup 110\n11000: from all fwmark 0x1000000/0x1000000 lookup 110\n",
            "11000: from all lookup 110\n",
            "11000: from all fwmark 0x1000000/0x1000000 lookup 111\n",
            "11000: not from all fwmark 0x1000000/0x1000000 lookup 110\n",
        ] {
            assert!(!policy_rule_state_is_exact(rules), "{rules:?}");
            assert!(matches!(policy_rule_probe(rules), Probe::Unknown(_)));
        }
        for routes in [
            "default dev hyz-mihomo scope link\ndefault dev other scope link\n",
            "default dev hyz-mihomo scope link\n192.0.2.0/24 dev hyz-mihomo\n",
            "default dev other scope link\n",
            "default dev hyz-mihomo\n",
        ] {
            assert!(!policy_route_state_is_exact(routes), "{routes:?}");
            assert!(matches!(policy_route_probe(routes), Probe::Unknown(_)));
        }
        assert_eq!(
            policy_rule_probe("0: from all lookup local\n"),
            Probe::Known(false)
        );
        assert_eq!(policy_route_probe("\n"), Probe::Known(false));
    }

    #[test]
    fn ownership_faults_reject_extra_body_and_references() {
        let token = "hyz-router-00000000-0000-0000-0000-000000000000";
        let expected = expected_chain_rules(ROUTER_NAT_CHAIN, token, None);
        let mut golden = format!("-N {ROUTER_NAT_CHAIN}\n");
        for rule in &expected {
            golden.push_str(&format!("{}\n", rule.join(" ")));
        }
        assert!(chain_output_is_exact(&golden, ROUTER_NAT_CHAIN, &expected));
        let modified = format!("{golden}-A {ROUTER_NAT_CHAIN} -j ACCEPT\n");
        assert!(!chain_output_is_exact(
            &modified,
            ROUTER_NAT_CHAIN,
            &expected
        ));

        let hook = format!("-A POSTROUTING -m comment --comment {token} -j {ROUTER_NAT_CHAIN}\n");
        assert_eq!(
            exact_chain_references(&hook, ROUTER_NAT_CHAIN),
            Some(vec![words(&[
                "-A",
                "POSTROUTING",
                "-m",
                "comment",
                "--comment",
                token,
                "-j",
                ROUTER_NAT_CHAIN,
            ])])
        );
        assert_eq!(
            exact_chain_references(
                &format!("{hook}-A OUTPUT -j {ROUTER_NAT_CHAIN}\n"),
                ROUTER_NAT_CHAIN
            )
            .map(|rules| rules.len()),
            Some(2)
        );
    }

    #[test]
    fn fail_open_stale_cleanup_removes_controller_credentials() {
        let source = include_str!("system.rs");
        let cleanup = source
            .split("fn remove_fail_open_stale_state")
            .nth(1)
            .unwrap()
            .split("fn sleep_fail_open_retry")
            .next()
            .unwrap();
        let watcher = cleanup.find("MIHOMO_WATCHER_RECORD").unwrap();
        let secret = cleanup.find("MIHOMO_CONTROLLER_SECRET").unwrap();
        let runtime = cleanup.find("MIHOMO_RUNTIME_CONFIG").unwrap();
        assert!(watcher < secret && secret < runtime);
    }

    #[test]
    fn normalized_parser_rejects_unterminated_quotes() {
        assert!(normalized_rule("-A FORWARD --comment \"unterminated").is_none());
    }
}
