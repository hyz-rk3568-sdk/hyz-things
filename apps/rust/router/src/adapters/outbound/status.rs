use async_trait::async_trait;
use std::{fs, net::Ipv4Addr};

use super::{
    paths::MIHOMO_SOURCE_CONFIG,
    process::{LinuxRouterPlatform, Tool},
};
use crate::{
    application::{
        ports::{DevicePolicyStorePort, SystemProbePort},
        status::{StatusRouterPlatformPort, StatusSystemProbePort},
    },
    domain::{
        network::{
            ForwardingDesired, NetworkDesired, NetworkObserved, OwnedResource, Probe, LAN_BRIDGE,
            LAN_MEMBER, WAN_INTERFACE,
        },
        proxy::{ProxyDesired, ProxyObserved},
        status::{
            Component, InterfaceStats, Issue, LanTunEffective, LanTunStatus, LinkState,
            MihomoCoreStatus, ProxyResourceState, ProxyStatus, RouterStatus, SystemStats,
        },
    },
};

const MAX_SAFE_SSID_BYTES: usize = 128;

#[async_trait]
impl StatusRouterPlatformPort for LinuxRouterPlatform {
    async fn read_router_status(&self) -> Component<RouterStatus> {
        let platform = self.clone();
        tokio::task::spawn_blocking(move || read_router_status(&platform))
            .await
            .unwrap_or_else(|_| unavailable("router_probe_failed", "Router status is unavailable"))
    }

    async fn read_proxy_status(&self) -> Component<ProxyStatus> {
        let platform = self.clone();
        tokio::task::spawn_blocking(move || read_proxy_status(&platform))
            .await
            .unwrap_or_else(|_| unavailable("proxy_probe_failed", "Proxy status is unavailable"))
    }
}

#[async_trait]
impl StatusSystemProbePort for LinuxRouterPlatform {
    async fn read_system_stats(&self) -> Component<SystemStats> {
        tokio::task::spawn_blocking(read_system_stats)
            .await
            .unwrap_or_else(|_| {
                unavailable("system_probe_failed", "System statistics are unavailable")
            })
    }
}

fn read_router_status(platform: &LinuxRouterPlatform) -> Component<RouterStatus> {
    let mut partial = false;

    let wpa = probe_output(platform, Tool::WpaCli, &["-i", WAN_INTERFACE, "status"]);
    let signal = probe_output(
        platform,
        Tool::WpaCli,
        &["-i", WAN_INTERFACE, "signal_poll"],
    );
    let route = probe_output(
        platform,
        Tool::Ip,
        &["-4", "route", "show", "default", "dev", WAN_INTERFACE],
    );
    let wan_address = probe_output(
        platform,
        Tool::Ip,
        &["-o", "-4", "address", "show", "dev", WAN_INTERFACE],
    );
    let hostapd = probe_output(platform, Tool::HostapdCli, &["-i", LAN_MEMBER, "status"]);
    let stations = probe_output(platform, Tool::HostapdCli, &["-i", LAN_MEMBER, "list_sta"]);

    partial |= wpa.is_none()
        || signal.is_none()
        || route.is_none()
        || wan_address.is_none()
        || hostapd.is_none()
        || stations.is_none();

    let observed = match platform.observe_network() {
        Ok(observed) => Some(observed),
        Err(_) => {
            partial = true;
            None
        }
    };

    let sta_state = wpa
        .as_deref()
        .and_then(|output| key_value(output, "wpa_state"))
        .map(parse_link_state);
    let sta_ssid = wpa
        .as_deref()
        .and_then(|output| key_value(output, "ssid"))
        .and_then(safe_text);
    let sta_signal_dbm = signal
        .as_deref()
        .and_then(|output| key_value(output, "RSSI"))
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| (-127..=0).contains(value));
    let sta_address = wan_address.as_deref().and_then(first_ipv4_cidr);
    let default_route_present = route.as_ref().map(|output| {
        output
            .lines()
            .any(|line| line.trim_start().starts_with("default "))
    });
    let default_route_metric = route.as_deref().and_then(route_metric);
    let ap_state = hostapd
        .as_deref()
        .and_then(|output| key_value(output, "state"))
        .map(parse_link_state);
    let ap_client_count = stations.as_deref().map(count_mac_lines);

    let status = RouterStatus {
        sta_state,
        sta_ssid,
        sta_address,
        sta_signal_dbm,
        default_route_present,
        default_route_metric,
        ap_state,
        ap_client_count,
        lan_present: observed.as_ref().and_then(|value| match &value.bridge {
            Probe::Known(OwnedResource::Absent) => Some(false),
            Probe::Known(_) => Some(true),
            Probe::Unknown(_) => None,
        }),
        lan_address: observed
            .as_ref()
            .and_then(|value| known_bool(&value.lan_address_present))
            .and_then(|present| present.then(|| "192.168.8.1/24".to_owned())),
        ap_attached_to_lan: observed
            .as_ref()
            .and_then(|value| known_bool(&value.ap_attached)),
        ipv4_forwarding: observed
            .as_ref()
            .and_then(|value| known_bool(&value.ipv4_forwarding)),
        masquerade_enabled: observed
            .as_ref()
            .and_then(|value| match &value.router_firewall {
                Probe::Known(OwnedResource::Owned { .. }) => Some(true),
                Probe::Known(OwnedResource::Absent) => Some(false),
                Probe::Known(OwnedResource::Foreign) | Probe::Unknown(_) => None,
            }),
    };

    if status.sta_state.is_none() || status.ap_state.is_none() {
        partial = true;
    }
    if observed.as_ref().is_some_and(router_status_is_degraded) {
        partial = true;
    }
    if partial {
        Component::degraded(
            status,
            Issue::new("router_status_partial", "Some router status is unavailable"),
        )
    } else {
        Component::available(status)
    }
}

fn router_status_is_degraded(observed: &NetworkObserved) -> bool {
    let desired = match &observed.router_firewall {
        Probe::Known(OwnedResource::Owned { .. }) => NetworkDesired {
            forwarding: ForwardingDesired::Enabled,
        },
        Probe::Known(OwnedResource::Absent) => NetworkDesired {
            forwarding: ForwardingDesired::Disabled,
        },
        Probe::Known(OwnedResource::Foreign) | Probe::Unknown(_) => return true,
    };

    matches!(
        &observed.bridge,
        Probe::Known(OwnedResource::Foreign) | Probe::Unknown(_)
    ) || !observed.ready_for(&desired)
}

fn read_proxy_status(platform: &LinuxRouterPlatform) -> Component<ProxyStatus> {
    let observed = match platform.observe_proxy() {
        Ok(observed) => observed,
        Err(_) => return unavailable("proxy_probe_failed", "Proxy status is unavailable"),
    };

    let desired_direct_macs = match platform.load_device_policy() {
        Ok(config) => Probe::Known(config.direct_macs()),
        Err(error) => Probe::Unknown(error.to_string()),
    };
    proxy_status_from_observed(
        &observed,
        fs::metadata(MIHOMO_SOURCE_CONFIG).is_ok(),
        desired_direct_macs,
    )
}

fn proxy_status_from_observed(
    observed: &ProxyObserved,
    configured: bool,
    desired_direct_macs: Probe<
        std::collections::BTreeSet<crate::domain::device_policy::LanDeviceMac>,
    >,
) -> Component<ProxyStatus> {
    let features = match &observed.persisted_features {
        Probe::Known(features) if features.supported() => Some(*features),
        _ => None,
    };
    let desired_macs = match &desired_direct_macs {
        Probe::Known(macs) => Some(macs),
        Probe::Unknown(_) => None,
    };
    let core_required = features.map(|features| features.mihomo_required());
    let lan_ready = features
        .zip(desired_macs)
        .is_some_and(|(features, macs)| features.lan_tun_enabled && observed.lan_tun_ready(macs));
    let lan_effective = if lan_ready {
        LanTunEffective::Ready
    } else if observed.ordinary_nat_confirmed == Probe::Known(true)
        && features.is_some_and(|features| !features.lan_tun_enabled)
    {
        LanTunEffective::OrdinaryNat
    } else {
        LanTunEffective::NotConfirmed
    };
    let status = ProxyStatus {
        configured,
        mihomo: MihomoCoreStatus {
            configured_required: core_required,
            process: resource_state(&observed.process_identity_valid, core_required),
            runtime_config: resource_state(&observed.runtime_config_valid, core_required),
            mixed_port: resource_state(&observed.mixed_port_ready, core_required),
        },
        lan_tun: LanTunStatus {
            desired: features.map(|features| features.lan_tun_enabled),
            effective: lan_effective,
            ordinary_nat_fallback: known_bool(&observed.ordinary_nat_confirmed),
        },
    };
    let ready = match (features, desired_macs) {
        (Some(features), Some(macs)) => observed.ready_for(&ProxyDesired {
            lan_tun_enabled: features.lan_tun_enabled,
            tailscale_explicit_proxy_enabled: features.tailscale_explicit_proxy_enabled,
            direct_macs: macs.clone(),
        }),
        _ => false,
    };
    if ready {
        Component::available(status)
    } else {
        Component::degraded(
            status,
            Issue::new(
                "proxy_not_ready",
                "Proxy features do not satisfy strict independent readiness",
            ),
        )
    }
}

fn resource_state(probe: &Probe<bool>, required: Option<bool>) -> ProxyResourceState {
    match (probe, required) {
        (Probe::Unknown(_), _) | (_, None) => ProxyResourceState::Unknown,
        (Probe::Known(true), Some(true)) => ProxyResourceState::Ready,
        (Probe::Known(false), Some(false)) => ProxyResourceState::Absent,
        _ => ProxyResourceState::NotReady,
    }
}

fn read_system_stats() -> Component<SystemStats> {
    let uptime_seconds = fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|value| value.split_whitespace().next()?.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value as u64);
    let cpu_temperature_millidegrees = fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok());
    let interfaces = [LAN_BRIDGE, WAN_INTERFACE, LAN_MEMBER]
        .into_iter()
        .filter_map(interface_stats)
        .collect::<Vec<_>>();
    let complete =
        uptime_seconds.is_some() && cpu_temperature_millidegrees.is_some() && interfaces.len() == 3;
    let stats = SystemStats {
        uptime_seconds,
        cpu_temperature_millidegrees,
        interfaces,
    };
    if complete {
        Component::available(stats)
    } else if stats.uptime_seconds.is_none()
        && stats.cpu_temperature_millidegrees.is_none()
        && stats.interfaces.is_empty()
    {
        unavailable("system_probe_failed", "System statistics are unavailable")
    } else {
        Component::degraded(
            stats,
            Issue::new(
                "system_status_partial",
                "Some system statistics are unavailable",
            ),
        )
    }
}

fn probe_output(platform: &LinuxRouterPlatform, tool: Tool, args: &[&str]) -> Option<String> {
    let args = args
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    platform
        .run_probe(tool, &args)
        .ok()
        .filter(|output| output.success)
        .map(|output| output.stdout)
}

fn key_value<'a>(output: &'a str, key: &str) -> Option<&'a str> {
    output
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
}

fn safe_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= MAX_SAFE_SSID_BYTES
        && !value.chars().any(char::is_control))
    .then(|| value.to_owned())
}

fn parse_link_state(value: &str) -> LinkState {
    match value.trim() {
        "COMPLETED" | "CONNECTED" | "ENABLED" => LinkState::Up,
        "DISCONNECTED" | "DISABLED" | "INACTIVE" => LinkState::Down,
        "SCANNING" | "ASSOCIATING" | "ASSOCIATED" | "4WAY_HANDSHAKE" | "GROUP_HANDSHAKE" => {
            LinkState::Connecting
        }
        _ => LinkState::Unknown,
    }
}

fn first_ipv4_cidr(output: &str) -> Option<String> {
    let fields = output.split_whitespace().collect::<Vec<_>>();
    let index = fields.iter().position(|field| *field == "inet")?;
    let value = fields.get(index + 1)?;
    let (address, prefix) = value.split_once('/')?;
    address.parse::<Ipv4Addr>().ok()?;
    prefix.parse::<u8>().ok().filter(|prefix| *prefix <= 32)?;
    Some((*value).to_owned())
}

fn route_metric(output: &str) -> Option<u32> {
    let fields = output.split_whitespace().collect::<Vec<_>>();
    let index = fields.iter().position(|field| *field == "metric")?;
    fields.get(index + 1)?.parse::<u32>().ok()
}

fn count_mac_lines(output: &str) -> u32 {
    output
        .lines()
        .filter(|line| {
            let octets = line.trim().split(':').collect::<Vec<_>>();
            octets.len() == 6
                && octets.iter().all(|octet| {
                    octet.len() == 2 && octet.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
        })
        .count()
        .try_into()
        .unwrap_or(u32::MAX)
}

fn interface_stats(name: &str) -> Option<InterfaceStats> {
    let base = format!("/sys/class/net/{name}/statistics");
    Some(InterfaceStats {
        name: name.to_owned(),
        rx_bytes: read_u64(format!("{base}/rx_bytes"))?,
        tx_bytes: read_u64(format!("{base}/tx_bytes"))?,
    })
}

fn read_u64(path: String) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn known_bool(probe: &Probe<bool>) -> Option<bool> {
    match probe {
        Probe::Known(value) => Some(*value),
        Probe::Unknown(_) => None,
    }
}

fn unavailable<T>(code: &str, message: &str) -> Component<T> {
    Component::unavailable(Issue::new(code, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::proxy::ProxyFeaturesV1;

    #[test]
    fn parses_only_safe_status_values() {
        assert_eq!(
            first_ipv4_cidr("3: wlan0 inet 10.0.0.3/24 scope global"),
            Some("10.0.0.3/24".to_owned())
        );
        assert_eq!(
            route_metric("default via 10.0.0.1 dev wlan0 metric 600"),
            Some(600)
        );
        assert_eq!(count_mac_lines("00:11:22:33:44:55\nFAIL\n"), 1);
        assert!(safe_text("ssid\nwith-control").is_none());
    }

    fn proxy_observed(features: ProxyFeaturesV1) -> ProxyObserved {
        let tun = features.lan_tun_enabled;
        let core = features.mihomo_required();
        ProxyObserved {
            persisted_features: Probe::Known(features),
            process_identity_valid: Probe::Known(core),
            watcher_identity_valid: Probe::Known(tun),
            runtime_config_valid: Probe::Known(core),
            mixed_port_ready: Probe::Known(core),
            tun_interface: Probe::Known(if tun {
                OwnedResource::Owned {
                    token: "proxy".to_owned(),
                }
            } else {
                OwnedResource::Absent
            }),
            tun_firewall: Probe::Known(if tun {
                OwnedResource::Owned {
                    token: "owned".to_owned(),
                }
            } else {
                OwnedResource::Absent
            }),
            policy_rule_present: Probe::Known(tun),
            policy_route_present: Probe::Known(tun),
            interception_entry_present: Probe::Known(tun),
            ordinary_nat_confirmed: Probe::Known(!tun),
            active_direct_macs: Probe::Known(Default::default()),
        }
    }

    #[test]
    fn layered_status_distinguishes_shared_core_and_lan_tun() {
        let tailscale_only = proxy_status_from_observed(
            &proxy_observed(ProxyFeaturesV1::new(false, true)),
            true,
            Probe::Known(Default::default()),
        );
        let data = tailscale_only.data.unwrap();
        assert_eq!(data.mihomo.process, ProxyResourceState::Ready);
        assert_eq!(data.lan_tun.effective, LanTunEffective::OrdinaryNat);

        let mut incomplete = proxy_observed(ProxyFeaturesV1::new(true, true));
        incomplete.policy_route_present = Probe::Known(false);
        let status =
            proxy_status_from_observed(&incomplete, true, Probe::Known(Default::default()));
        assert_eq!(
            status.state,
            crate::domain::status::ComponentState::Degraded
        );
        assert_eq!(
            status.data.unwrap().lan_tun.effective,
            LanTunEffective::NotConfirmed
        );
    }
}
