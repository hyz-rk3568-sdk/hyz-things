use super::*;

pub(crate) const NETWORK_CONFIG_ENDPOINT: &str = "/api/v1/network/config";

pub(crate) const NETWORK_PENDING_ENDPOINT: &str = "/api/v1/network/pending";

pub(crate) const STA_SCAN_ENDPOINT: &str = "/api/v1/control/network/sta/scan";

pub(crate) const STA_APPLY_ENDPOINT: &str = "/api/v1/control/network/sta/apply";

pub(crate) const AP_PREPARE_ENDPOINT: &str = "/api/v1/control/network/ap/prepare";

pub(crate) const AP_APPLY_ENDPOINT: &str = "/api/v1/control/network/ap/apply";

pub(crate) const AP_CONFIRM_ENDPOINT: &str = "/api/v1/control/network/ap/confirm";

pub(crate) const AP_CANCEL_ENDPOINT: &str = "/api/v1/control/network/ap/cancel";

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub(crate) enum WifiCountryDto {
    #[serde(rename = "AU")]
    Au,
    #[serde(rename = "BR")]
    Br,
    #[serde(rename = "CA")]
    Ca,
    #[serde(rename = "CN")]
    Cn,
    #[serde(rename = "DE")]
    De,
    #[serde(rename = "FR")]
    Fr,
    #[serde(rename = "GB")]
    Gb,
    #[serde(rename = "IN")]
    In,
    #[serde(rename = "JP")]
    Jp,
    #[serde(rename = "KR")]
    Kr,
    #[serde(rename = "NZ")]
    Nz,
    #[serde(rename = "SG")]
    Sg,
    #[serde(rename = "TW")]
    Tw,
    #[serde(rename = "US")]
    Us,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NetworkConfigDto {
    pub(crate) version: u8,
    pub(crate) ap_ssid: String,
    pub(crate) sta_ssid: String,
    pub(crate) country: WifiCountryDto,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PendingConfigDto {
    pub(crate) version: u8,
    #[serde(rename = "staged_at_unix_ms")]
    pub(crate) _staged_at_unix_ms: u64,
    pub(crate) config: NetworkConfigDto,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NetworkPendingDto {
    pub(crate) pending: Option<PendingConfigDto>,
    pub(crate) applied: bool,
    pub(crate) remaining_seconds: Option<u64>,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WifiScanDto {
    pub(crate) ssid: String,
    pub(crate) bssid: String,
    pub(crate) frequency_mhz: u16,
    pub(crate) signal_dbm: i16,
    pub(crate) secured: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NetworkConfigResponseDto {
    pub(crate) config: NetworkConfigDto,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NetworkScanResponseDto {
    pub(crate) entries: Vec<WifiScanDto>,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StaRequest {
    pub(crate) ssid: String,
    pub(crate) passphrase: String,
}

pub(crate) enum NetworkApplyIntent {
    Sta(StaRequest),
    Ap,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApRequest {
    pub(crate) ssid: String,
    pub(crate) passphrase: String,
    pub(crate) country: String,
}
