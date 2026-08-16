//! Wi-Fi control request wire DTOs.

use serde::{Deserialize, Serialize};

use super::network_config::{WifiCountry, WifiPassphrase, WifiSsid};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaCandidateRequest {
    pub ssid: WifiSsid,
    pub passphrase: WifiPassphrase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApPrepareRequest {
    pub ssid: WifiSsid,
    pub passphrase: WifiPassphrase,
    pub country: WifiCountry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WifiScanEntry {
    pub ssid: WifiSsid,
    pub bssid: String,
    pub frequency_mhz: u16,
    pub signal_dbm: i16,
    pub secured: bool,
}
