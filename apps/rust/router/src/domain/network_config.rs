use serde::{Deserialize, Serialize};

pub use hyz_contract::network_config::{
    DerivedPsk, NetworkConfigSummary, PendingNetworkConfigSummary, WifiConfigError, WifiCountry,
    WifiPassphrase, WifiSsid,
};

pub const NETWORK_CONFIG_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApConfig {
    pub ssid: WifiSsid,
    pub psk: DerivedPsk,
    pub country: WifiCountry,
}

impl ApConfig {
    pub fn from_passphrase(
        ssid: WifiSsid,
        passphrase: &WifiPassphrase,
        country: WifiCountry,
    ) -> Self {
        let psk = passphrase.derive_psk(&ssid);
        Self { ssid, psk, country }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaConfig {
    pub ssid: WifiSsid,
    pub psk: DerivedPsk,
}

impl StaConfig {
    pub fn from_passphrase(ssid: WifiSsid, passphrase: &WifiPassphrase) -> Self {
        let psk = passphrase.derive_psk(&ssid);
        Self { ssid, psk }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkConfigV1 {
    pub version: u8,
    pub ap: ApConfig,
    pub sta: StaConfig,
}

impl NetworkConfigV1 {
    pub const fn new(ap: ApConfig, sta: StaConfig) -> Self {
        Self {
            version: NETWORK_CONFIG_VERSION,
            ap,
            sta,
        }
    }

    pub fn validate_version(&self) -> Result<(), WifiConfigError> {
        if self.version != NETWORK_CONFIG_VERSION {
            return Err(WifiConfigError::new("unsupported network config version"));
        }
        Ok(())
    }

    pub fn summary(&self) -> NetworkConfigSummary {
        NetworkConfigSummary {
            version: self.version,
            ap_ssid: self.ap.ssid.clone(),
            sta_ssid: self.sta.ssid.clone(),
            country: self.ap.country,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingNetworkConfigV1 {
    pub version: u8,
    pub staged_at_unix_ms: u64,
    pub config: NetworkConfigV1,
}

impl PendingNetworkConfigV1 {
    pub const fn new(staged_at_unix_ms: u64, config: NetworkConfigV1) -> Self {
        Self {
            version: NETWORK_CONFIG_VERSION,
            staged_at_unix_ms,
            config,
        }
    }

    pub fn validate_version(&self) -> Result<(), WifiConfigError> {
        if self.version != NETWORK_CONFIG_VERSION {
            return Err(WifiConfigError::new("unsupported pending record version"));
        }
        self.config.validate_version()
    }

    pub fn summary(&self) -> PendingNetworkConfigSummary {
        PendingNetworkConfigSummary {
            version: self.version,
            staged_at_unix_ms: self.staged_at_unix_ms,
            config: self.config.summary(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_never_serialize_psks() {
        let passphrase = WifiPassphrase::new("placeholder-secret").unwrap();
        let ap_ssid = WifiSsid::new("safe-ap").unwrap();
        let sta_ssid = WifiSsid::new("safe-sta").unwrap();
        let config = NetworkConfigV1::new(
            ApConfig::from_passphrase(ap_ssid, &passphrase, WifiCountry::Us),
            StaConfig::from_passphrase(sta_ssid, &passphrase),
        );
        let json = serde_json::to_string(&config.summary()).unwrap();
        assert!(!json.contains("psk"));
        assert!(!json.contains("placeholder-secret"));
        assert_eq!(
            serde_json::from_str::<NetworkConfigSummary>(&json).unwrap(),
            config.summary()
        );
    }

    #[test]
    fn persisted_versions_are_validated_on_read() {
        let passphrase = WifiPassphrase::new("placeholder-secret").unwrap();
        let config = NetworkConfigV1::new(
            ApConfig::from_passphrase(WifiSsid::new("ap").unwrap(), &passphrase, WifiCountry::Us),
            StaConfig::from_passphrase(WifiSsid::new("sta").unwrap(), &passphrase),
        );
        let mut value = serde_json::to_value(config).unwrap();
        value["version"] = 2.into();
        let decoded: NetworkConfigV1 = serde_json::from_value(value).unwrap();
        assert!(decoded.validate_version().is_err());
    }
}
