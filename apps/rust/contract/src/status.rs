//! Status snapshot DTOs served over the router control protocol.

use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

use super::tailscale::{TailscaleBackendState, TailscaleEnvironment, TailscaleMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotState {
    Ok,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentState {
    Available,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component<T> {
    pub state: ComponentState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<Issue>,
}

impl<T> Component<T> {
    pub fn available(data: T) -> Self {
        Self {
            state: ComponentState::Available,
            data: Some(data),
            issue: None,
        }
    }

    pub fn degraded(data: T, issue: Issue) -> Self {
        Self {
            state: ComponentState::Degraded,
            data: Some(data),
            issue: Some(issue),
        }
    }

    pub fn unavailable(issue: Issue) -> Self {
        Self {
            state: ComponentState::Unavailable,
            data: None,
            issue: Some(issue),
        }
    }

    pub fn is_available(&self) -> bool {
        self.state == ComponentState::Available
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Issue {
    pub code: String,
    pub message: String,
}

impl Issue {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusSnapshot {
    pub state: SnapshotState,
    pub observed_at_unix_ms: u64,
    pub router: Component<RouterStatus>,
    pub proxy: Component<ProxyStatus>,
    pub tailscale: Component<TailscaleStatus>,
    pub system: Component<SystemStats>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TailscaleRouteApproval {
    Approved,
    UnknownExternalApprovalRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TailscaleConnectionType {
    Direct,
    PeerRelay,
    Derp,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TailscaleConnectionStatus {
    pub kind: TailscaleConnectionType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derp_region: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TailscaleErrorCategory {
    ProbeFailed,
    Conflict,
    NotReady,
    OperationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TailscaleStatus {
    pub desired_mode: Option<TailscaleMode>,
    pub effective_mode: Option<TailscaleMode>,
    pub backend_state: TailscaleBackendState,
    pub authenticated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<Ipv4Addr>,
    pub route_advertised: Option<bool>,
    pub local_firewall_ready: Option<bool>,
    pub route_approval: TailscaleRouteApproval,
    pub connection: TailscaleConnectionStatus,
    pub explicit_proxy_desired: Option<bool>,
    pub environment: Option<TailscaleEnvironment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_category: Option<TailscaleErrorCategory>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UplinkId {
    Ethernet,
    Wifi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UplinkStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_up: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_up: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_route_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_route_metric: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<Ipv4Addr>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolver_present: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveResolverStatus {
    pub uplink: UplinkId,
    pub nameservers: Vec<Ipv4Addr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RouterStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sta_state: Option<LinkState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sta_ssid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sta_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sta_signal_dbm: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_route_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_route_metric: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ap_state: Option<LinkState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ap_client_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lan_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lan_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ap_attached_to_lan: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4_forwarding: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masquerade_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ethernet: Option<UplinkStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wifi: Option<UplinkStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_uplink: Option<UplinkId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_resolver: Option<ActiveResolverStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    Up,
    Down,
    Connecting,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    Explicit,
    Tun,
    Disabled,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyStatus {
    pub configured: bool,
    pub mihomo: MihomoCoreStatus,
    pub lan_tun: LanTunStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MihomoCoreStatus {
    pub configured_required: Option<bool>,
    pub process: ProxyResourceState,
    pub runtime_config: ProxyResourceState,
    pub mixed_port: ProxyResourceState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanTunStatus {
    pub desired: Option<bool>,
    pub effective: LanTunEffective,
    pub ordinary_nat_fallback: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyResourceState {
    Ready,
    Absent,
    NotReady,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanTunEffective {
    Ready,
    OrdinaryNat,
    NotConfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SystemStats {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_temperature_millidegrees: Option<i64>,
    pub interfaces: Vec<InterfaceStats>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterfaceStats {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_snapshot_round_trips_without_secrets() {
        let snapshot = StatusSnapshot {
            state: SnapshotState::Degraded,
            observed_at_unix_ms: 42,
            router: Component::available(RouterStatus {
                sta_state: Some(LinkState::Up),
                sta_ssid: Some("Example Wi-Fi".to_owned()),
                sta_address: Some("10.0.0.23/24".to_owned()),
                sta_signal_dbm: Some(-48),
                default_route_present: Some(true),
                default_route_metric: Some(600),
                ap_state: Some(LinkState::Up),
                ap_client_count: Some(2),
                lan_present: Some(true),
                lan_address: Some("192.168.8.1/24".to_owned()),
                ap_attached_to_lan: Some(true),
                ipv4_forwarding: Some(true),
                masquerade_enabled: Some(true),
                ethernet: Some(UplinkStatus {
                    link_up: Some(true),
                    session_up: Some(true),
                    address_present: Some(true),
                    address: Some("192.0.2.10/24".to_owned()),
                    default_route_present: Some(true),
                    default_route_metric: Some(100),
                    gateway: Some("192.0.2.1".parse().unwrap()),
                    resolver_present: Some(true),
                }),
                wifi: Some(UplinkStatus {
                    link_up: Some(true),
                    session_up: Some(true),
                    address_present: Some(true),
                    address: Some("198.51.100.10/24".to_owned()),
                    default_route_present: Some(true),
                    default_route_metric: Some(600),
                    gateway: Some("198.51.100.1".parse().unwrap()),
                    resolver_present: Some(true),
                }),
                active_uplink: Some(UplinkId::Ethernet),
                active_resolver: Some(ActiveResolverStatus {
                    uplink: UplinkId::Ethernet,
                    nameservers: vec!["192.0.2.53".parse().unwrap()],
                }),
            }),
            proxy: Component::unavailable(Issue::new(
                "proxy_unavailable",
                "Proxy status is unavailable",
            )),
            tailscale: Component::available(TailscaleStatus {
                desired_mode: Some(TailscaleMode::LanSubnetAccess),
                effective_mode: Some(TailscaleMode::RouterOnly),
                backend_state: TailscaleBackendState::Running,
                authenticated: Some(true),
                ipv4: Some("100.64.0.10".parse().unwrap()),
                route_advertised: Some(true),
                local_firewall_ready: Some(true),
                route_approval: TailscaleRouteApproval::UnknownExternalApprovalRequired,
                connection: TailscaleConnectionStatus {
                    kind: TailscaleConnectionType::Derp,
                    derp_region: Some("sfo".to_owned()),
                },
                explicit_proxy_desired: Some(true),
                environment: Some(TailscaleEnvironment::MihomoExplicit),
                error_category: None,
            }),
            system: Component::available(SystemStats {
                uptime_seconds: Some(120),
                cpu_temperature_millidegrees: Some(45_000),
                interfaces: vec![InterfaceStats {
                    name: "wlan0".to_owned(),
                    rx_bytes: 10,
                    tx_bytes: 20,
                }],
            }),
        };

        let json = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(!json.contains("password"));
        assert!(!json.contains("subscription"));
        assert!(!json.contains("login_url"));
        assert!(!json.contains("auth_key"));
        assert!(!json.contains("node_key"));
        let decoded: StatusSnapshot = serde_json::from_str(&json).expect("deserialize snapshot");
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn legacy_router_status_deserializes_without_uplink_fields() {
        let json = r#"{
            "state":"degraded",
            "observed_at_unix_ms":7,
            "router":{
                "state":"available",
                "data":{
                    "sta_state":"up",
                    "sta_ssid":"Example Wi-Fi",
                    "sta_address":"10.0.0.23/24",
                    "sta_signal_dbm":-48,
                    "default_route_present":true,
                    "default_route_metric":600,
                    "ap_state":"up",
                    "ap_client_count":0,
                    "lan_present":true,
                    "lan_address":"192.168.8.1/24",
                    "ap_attached_to_lan":true,
                    "ipv4_forwarding":true,
                    "masquerade_enabled":true
                }
            },
            "proxy":{"state":"unavailable"},
            "tailscale":{"state":"unavailable"},
            "system":{"state":"available","data":{"interfaces":[]}}
        }"#;

        let snapshot: StatusSnapshot =
            serde_json::from_str(json).expect("legacy status must decode");
        let router = snapshot.router.data.expect("router data");
        assert_eq!(router.active_uplink, None);
        assert_eq!(router.active_resolver, None);
        assert_eq!(router.ethernet, None);
        assert_eq!(router.wifi, None);
    }
}
