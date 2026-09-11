use super::*;

pub(crate) const PROXY_LAN_TUN_ENDPOINT: &str = "/api/v1/control/proxy/lan-tun";

pub(crate) const PROXY_LOCAL_SYSTEM_ENDPOINT: &str = "/api/v1/control/proxy/local-system";

pub(crate) const PROXY_SELECTION_ENDPOINT: &str = "/api/v1/control/proxy/selection";

pub(crate) const PROXY_DELAYS_ENDPOINT: &str = "/api/v1/control/proxy/delays";

pub(crate) const SUBSCRIPTION_ENDPOINT: &str = "/api/v1/proxy/subscription";

pub(crate) const SUBSCRIPTION_SOURCE_ENDPOINT: &str = "/api/v1/control/proxy/subscription/source";

pub(crate) const SUBSCRIPTION_REFRESH_ENDPOINT: &str = "/api/v1/control/proxy/subscription/refresh";

pub(crate) const DEVICE_POLICIES_ENDPOINT: &str = "/api/v1/proxy/device-policies";

pub(crate) const DEVICE_POLICIES_UPDATE_ENDPOINT: &str = "/api/v1/control/proxy/device-policies";

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SubscriptionStateDto {
    Idle,
    Fetching,
    Active,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DevicePolicyDto {
    Direct,
    Proxy,
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DevicePolicyEntryDto {
    pub(crate) mac: String,
    pub(crate) label: String,
    pub(crate) policy: DevicePolicyDto,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DevicePolicyConfigDto {
    pub(crate) version: u8,
    pub(crate) generation: u64,
    pub(crate) entries: Vec<DevicePolicyEntryDto>,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LanClientDto {
    pub(crate) mac: String,
    pub(crate) lease_address: Option<String>,
    pub(crate) hostname: Option<String>,
    pub(crate) associated: bool,
    pub(crate) policy: DevicePolicyDto,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActivityNetworkDto {
    Tcp,
    Udp,
    Other,
}

impl ActivityNetworkDto {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
            Self::Other => "OTHER",
        }
    }
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActivityRecordDto {
    pub(crate) connection_id: String,
    pub(crate) mac: String,
    pub(crate) device_name: String,
    pub(crate) source_address: String,
    pub(crate) target: String,
    pub(crate) destination_port: u16,
    pub(crate) network: ActivityNetworkDto,
    pub(crate) rule: String,
    pub(crate) chains: Vec<String>,
    pub(crate) upload_bytes: u64,
    pub(crate) download_bytes: u64,
    pub(crate) first_seen_unix_ms: u64,
    pub(crate) last_seen_unix_ms: u64,
    pub(crate) active: bool,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActivitySnapshotDto {
    pub(crate) observed_at_unix_ms: u64,
    pub(crate) records: Vec<ActivityRecordDto>,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DevicePolicySnapshotDto {
    pub(crate) config: DevicePolicyConfigDto,
    pub(crate) clients: Vec<LanClientDto>,
    pub(crate) effective: bool,
    #[serde(default)]
    pub(crate) activity: Option<ActivitySnapshotDto>,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DevicePolicyUpdateDto {
    pub(crate) expected_generation: u64,
    pub(crate) entries: Vec<DevicePolicyEntryDto>,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubscriptionDto {
    pub(crate) configured: bool,
    pub(crate) state: SubscriptionStateDto,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubscriptionResponseDto {
    pub(crate) subscription: SubscriptionDto,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubscriptionSourceRequest {
    pub(crate) url: String,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProxyFeatureRequestDto {
    pub(crate) enabled: bool,
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum DelayRefreshControlResponse {
    ProxyDelays { groups: Vec<ProxyGroup> },
}
