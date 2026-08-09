use serde::{Deserialize, Serialize};

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
    pub system: Component<SystemStats>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkState {
    Up,
    Down,
    Connecting,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyStatus {
    pub state: ProxyState,
    pub mode: ProxyMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configured: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordinary_nat_fallback: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyState {
    Running,
    Stopped,
    Disabled,
    Error,
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
            }),
            proxy: Component::unavailable(Issue::new(
                "proxy_unavailable",
                "Proxy status is unavailable",
            )),
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
        let decoded: StatusSnapshot = serde_json::from_str(&json).expect("deserialize snapshot");
        assert_eq!(decoded, snapshot);
    }
}
