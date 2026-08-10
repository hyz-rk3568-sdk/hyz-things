use crate::{
    adapters::outbound::storage,
    domain::network_config::{
        ApConfig, DerivedPsk, NetworkConfigV1, PendingNetworkConfigV1, StaConfig, WifiCountry,
        WifiPassphrase, WifiSsid,
    },
};
use std::{error::Error, fmt};
use zeroize::Zeroizing;

pub const NETWORK_CONFIG_PATH: &str = "/userdata/hyz-router/network-config-v1.json";
pub const PENDING_NETWORK_CONFIG_PATH: &str = "/userdata/hyz-router/network-config-pending-v1.json";
pub const STA_ROLLBACK_CONFIG_PATH: &str =
    "/userdata/hyz-router/network-config-sta-rollback-v1.json";
pub const LEGACY_WPA_CONFIG_PATH: &str = "/userdata/hyz-router/wpa_supplicant.conf";
pub const LEGACY_HOSTAPD_CONFIG_PATH: &str = "/userdata/hyz-router/hostapd-sta-ap.conf";
const MAX_NETWORK_CONFIG_SIZE: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkConfigError {
    Invalid(String),
    Io(String),
    MigrationRequired(String),
}

impl fmt::Display for NetworkConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(detail) => write!(formatter, "invalid network config: {detail}"),
            Self::Io(detail) => write!(formatter, "network config I/O error: {detail}"),
            Self::MigrationRequired(detail) => write!(formatter, "migration required: {detail}"),
        }
    }
}

impl Error for NetworkConfigError {}

#[derive(Debug, Clone)]
pub struct NetworkConfigStore {
    config_path: &'static str,
    pending_path: &'static str,
    sta_rollback_path: &'static str,
    legacy_wpa_path: &'static str,
    legacy_hostapd_path: &'static str,
}

impl Default for NetworkConfigStore {
    fn default() -> Self {
        Self {
            config_path: NETWORK_CONFIG_PATH,
            pending_path: PENDING_NETWORK_CONFIG_PATH,
            sta_rollback_path: STA_ROLLBACK_CONFIG_PATH,
            legacy_wpa_path: LEGACY_WPA_CONFIG_PATH,
            legacy_hostapd_path: LEGACY_HOSTAPD_CONFIG_PATH,
        }
    }
}

impl NetworkConfigStore {
    pub fn read(&self) -> Result<Option<NetworkConfigV1>, NetworkConfigError> {
        read_json(self.config_path)
    }

    pub fn persist(&self, config: &NetworkConfigV1) -> Result<(), NetworkConfigError> {
        config
            .validate_version()
            .map_err(|error| NetworkConfigError::Invalid(error.to_string()))?;
        write_json(self.config_path, config)
    }

    pub fn read_pending(&self) -> Result<Option<PendingNetworkConfigV1>, NetworkConfigError> {
        read_json(self.pending_path)
    }

    pub fn persist_pending(
        &self,
        pending: &PendingNetworkConfigV1,
    ) -> Result<(), NetworkConfigError> {
        pending
            .validate_version()
            .map_err(|error| NetworkConfigError::Invalid(error.to_string()))?;
        write_json(self.pending_path, pending)
    }

    pub fn remove_pending(&self) -> Result<(), NetworkConfigError> {
        storage::remove_file_durable(self.pending_path)
            .map_err(|error| NetworkConfigError::Io(error.to_string()))
    }

    pub fn read_sta_rollback(&self) -> Result<Option<NetworkConfigV1>, NetworkConfigError> {
        read_json(self.sta_rollback_path)
    }

    pub fn persist_sta_rollback(
        &self,
        committed: &NetworkConfigV1,
    ) -> Result<(), NetworkConfigError> {
        committed
            .validate_version()
            .map_err(|error| NetworkConfigError::Invalid(error.to_string()))?;
        write_json(self.sta_rollback_path, committed)
    }

    pub fn remove_sta_rollback(&self) -> Result<(), NetworkConfigError> {
        storage::remove_file_durable(self.sta_rollback_path)
            .map_err(|error| NetworkConfigError::Io(error.to_string()))
    }

    /// Imports legacy files only when both files have one unambiguous SSID, credential, and
    /// country. Legacy files are never modified. Ambiguous or partial input requires an operator.
    pub fn read_or_migrate(&self) -> Result<Option<NetworkConfigV1>, NetworkConfigError> {
        if let Some(config) = self.read()? {
            return Ok(Some(config));
        }
        let wpa = read_legacy(self.legacy_wpa_path)?;
        let hostapd = read_legacy(self.legacy_hostapd_path)?;
        match (wpa, hostapd) {
            (None, None) => Ok(None),
            (Some(wpa), Some(hostapd)) => {
                let config = migrate_legacy_configs(&wpa, &hostapd)?;
                // The canonical path was confirmed absent above. atomic_write_private uses rename,
                // but this method never writes unless migration is uniquely parsed.
                self.persist(&config)?;
                Ok(Some(config))
            }
            _ => Err(NetworkConfigError::MigrationRequired(
                "both legacy WPA and hostapd files are required".to_owned(),
            )),
        }
    }
}

fn read_json<T>(path: &str) -> Result<Option<T>, NetworkConfigError>
where
    T: serde::de::DeserializeOwned + VersionedRecord,
{
    let Some(json) = storage::read_private_small_optional(path, MAX_NETWORK_CONFIG_SIZE)
        .map_err(|error| NetworkConfigError::Io(error.to_string()))?
    else {
        return Ok(None);
    };
    let value: T = serde_json::from_str(&json)
        .map_err(|error| NetworkConfigError::Invalid(format!("decode {path}: {error}")))?;
    value.validate_record()?;
    Ok(Some(value))
}

fn write_json<T: serde::Serialize>(path: &str, value: &T) -> Result<(), NetworkConfigError> {
    let mut json = serde_json::to_vec(value)
        .map_err(|error| NetworkConfigError::Invalid(format!("encode {path}: {error}")))?;
    json.push(b'\n');
    storage::atomic_write_private(path, &json)
        .map_err(|error| NetworkConfigError::Io(error.to_string()))
}

trait VersionedRecord {
    fn validate_record(&self) -> Result<(), NetworkConfigError>;
}

impl VersionedRecord for NetworkConfigV1 {
    fn validate_record(&self) -> Result<(), NetworkConfigError> {
        self.validate_version()
            .map_err(|error| NetworkConfigError::Invalid(error.to_string()))
    }
}

impl VersionedRecord for PendingNetworkConfigV1 {
    fn validate_record(&self) -> Result<(), NetworkConfigError> {
        self.validate_version()
            .map_err(|error| NetworkConfigError::Invalid(error.to_string()))
    }
}

fn read_legacy(path: &str) -> Result<Option<String>, NetworkConfigError> {
    storage::read_private_small_optional(path, MAX_NETWORK_CONFIG_SIZE)
        .map_err(|error| NetworkConfigError::Io(error.to_string()))
}

pub fn render_wpa_supplicant(config: &StaConfig) -> Zeroizing<String> {
    let ssid = encode_hex(config.ssid.as_bytes());
    let psk = config.psk.to_hex();
    Zeroizing::new(format!(
        "ctrl_interface=/run/wpa_supplicant\n\
         update_config=0\n\
         network={{\n\
         \tssid={ssid}\n\
         \tpsk={}\n\
         \tkey_mgmt=WPA-PSK\n\
         \tproto=RSN\n\
         \tpairwise=CCMP\n\
         \tgroup=CCMP\n\
         }}\n",
        psk.as_str()
    ))
}

pub fn render_hostapd(config: &ApConfig) -> Zeroizing<String> {
    render_hostapd_on_channel(config, 6)
}

pub fn render_hostapd_on_channel(config: &ApConfig, channel: u8) -> Zeroizing<String> {
    let ssid = encode_hex(config.ssid.as_bytes());
    let psk = config.psk.to_hex();
    Zeroizing::new(format!(
        "interface=p2p0\n\
         driver=nl80211\n\
         ctrl_interface=/var/run/hostapd\n\
         ssid2={ssid}\n\
         country_code={}\n\
         ieee80211d=1\n\
         hw_mode=g\n\
         channel={channel}\n\
         auth_algs=1\n\
         wpa=2\n\
         wpa_key_mgmt=WPA-PSK\n\
         rsn_pairwise=CCMP\n\
         wpa_psk={}\n",
        config.country.as_str(),
        psk.as_str()
    ))
}

fn encode_hex(bytes: &[u8]) -> String {
    use fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

pub fn migrate_legacy_configs(
    wpa: &str,
    hostapd: &str,
) -> Result<NetworkConfigV1, NetworkConfigError> {
    let (_, sta_ssid) = parse_unique(wpa, &["ssid"])?;
    let (sta_secret_kind, sta_secret) = parse_unique(wpa, &["psk"])?;
    let (ap_ssid_kind, ap_ssid) = parse_unique(hostapd, &["ssid", "ssid2"])?;
    let (ap_secret_kind, ap_secret) = parse_unique(hostapd, &["wpa_passphrase", "wpa_psk"])?;
    let (_, country) = parse_unique(hostapd, &["country_code"])?;

    let sta_ssid = parse_wpa_ssid(&sta_ssid, "STA SSID")?;
    let ap_ssid = parse_hostapd_ssid(&ap_ssid_kind, &ap_ssid)?;
    let sta_psk = parse_wpa_secret(&sta_secret_kind, &sta_secret, &sta_ssid)?;
    let ap_psk = parse_hostapd_secret(&ap_secret_kind, &ap_secret, &ap_ssid)?;
    let country = parse_country(unquote_plain(&country, "country")?)?;

    Ok(NetworkConfigV1::new(
        ApConfig {
            ssid: ap_ssid,
            psk: ap_psk,
            country,
        },
        StaConfig {
            ssid: sta_ssid,
            psk: sta_psk,
        },
    ))
}

fn parse_unique(source: &str, keys: &[&str]) -> Result<(String, String), NetworkConfigError> {
    if source.as_bytes().contains(&0) {
        return migration_required("legacy config contains NUL");
    }
    let mut values = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if keys.contains(&key) {
            values.push((key.to_owned(), value.trim().to_owned()));
        }
    }
    if values.len() != 1 {
        return migration_required(&format!(
            "expected exactly one legacy {} directive",
            keys.join("/")
        ));
    }
    Ok(values.remove(0))
}

fn parse_wpa_ssid(value: &str, label: &str) -> Result<WifiSsid, NetworkConfigError> {
    if value.starts_with('"') || value.ends_with('"') {
        return parse_plain_ssid(unquote_plain(value, label)?, label);
    }
    parse_hex_ssid(value, label)
}

fn parse_hostapd_ssid(key: &str, value: &str) -> Result<WifiSsid, NetworkConfigError> {
    match key {
        "ssid" => parse_plain_ssid(value, "AP SSID"),
        "ssid2" => parse_hex_ssid(value, "AP SSID"),
        _ => unreachable!("hostapd SSID key came from a fixed allowlist"),
    }
}

fn parse_plain_ssid(value: &str, label: &str) -> Result<WifiSsid, NetworkConfigError> {
    WifiSsid::new(value.to_owned())
        .map_err(|error| NetworkConfigError::MigrationRequired(format!("{label}: {error}")))
}

fn parse_hex_ssid(value: &str, label: &str) -> Result<WifiSsid, NetworkConfigError> {
    if value.len() % 2 != 0 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return migration_required(&format!("{label} is not hexadecimal"));
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        bytes.push((hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]));
    }
    let decoded = String::from_utf8(bytes)
        .map_err(|_| NetworkConfigError::MigrationRequired(format!("{label} hex is not UTF-8")))?;
    parse_plain_ssid(&decoded, label)
}

fn parse_wpa_secret(
    key: &str,
    value: &str,
    ssid: &WifiSsid,
) -> Result<DerivedPsk, NetworkConfigError> {
    debug_assert_eq!(key, "psk");
    if value.starts_with('"') || value.ends_with('"') {
        derive_legacy_passphrase(
            unquote_plain(value, "STA credential")?,
            ssid,
            "STA credential",
        )
    } else {
        parse_legacy_psk(value, "STA credential")
    }
}

fn parse_hostapd_secret(
    key: &str,
    value: &str,
    ssid: &WifiSsid,
) -> Result<DerivedPsk, NetworkConfigError> {
    match key {
        "wpa_passphrase" => derive_legacy_passphrase(value, ssid, "AP credential"),
        "wpa_psk" => parse_legacy_psk(value, "AP credential"),
        _ => unreachable!("hostapd credential key came from a fixed allowlist"),
    }
}

fn derive_legacy_passphrase(
    value: &str,
    ssid: &WifiSsid,
    label: &str,
) -> Result<DerivedPsk, NetworkConfigError> {
    let passphrase = WifiPassphrase::new(value.to_owned())
        .map_err(|error| NetworkConfigError::MigrationRequired(format!("{label}: {error}")))?;
    Ok(passphrase.derive_psk(ssid))
}

fn parse_legacy_psk(value: &str, label: &str) -> Result<DerivedPsk, NetworkConfigError> {
    DerivedPsk::from_hex(value)
        .map_err(|error| NetworkConfigError::MigrationRequired(format!("{label}: {error}")))
}

fn unquote_plain<'a>(value: &'a str, label: &str) -> Result<&'a str, NetworkConfigError> {
    if value.starts_with('"') || value.ends_with('"') {
        if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
            return migration_required(&format!("{label} has mismatched quotes"));
        }
        let inner = &value[1..value.len() - 1];
        if inner.contains(['\\', '"']) {
            return migration_required(&format!("{label} uses legacy escaping"));
        }
        Ok(inner)
    } else {
        Ok(value)
    }
}

fn parse_country(value: &str) -> Result<WifiCountry, NetworkConfigError> {
    match value {
        "AU" => Ok(WifiCountry::Au),
        "BR" => Ok(WifiCountry::Br),
        "CA" => Ok(WifiCountry::Ca),
        "CN" => Ok(WifiCountry::Cn),
        "DE" => Ok(WifiCountry::De),
        "FR" => Ok(WifiCountry::Fr),
        "GB" => Ok(WifiCountry::Gb),
        "IN" => Ok(WifiCountry::In),
        "JP" => Ok(WifiCountry::Jp),
        "KR" => Ok(WifiCountry::Kr),
        "NZ" => Ok(WifiCountry::Nz),
        "SG" => Ok(WifiCountry::Sg),
        "TW" => Ok(WifiCountry::Tw),
        "US" => Ok(WifiCountry::Us),
        _ => migration_required("legacy country is not in the allowlist"),
    }
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => unreachable!("hex was validated before decoding"),
    }
}

fn migration_required<T>(detail: &str) -> Result<T, NetworkConfigError> {
    Err(NetworkConfigError::MigrationRequired(detail.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> NetworkConfigV1 {
        let placeholder = WifiPassphrase::new("unit-test-placeholder").unwrap();
        NetworkConfigV1::new(
            ApConfig::from_passphrase(
                WifiSsid::new("AP 界").unwrap(),
                &placeholder,
                WifiCountry::Us,
            ),
            StaConfig::from_passphrase(WifiSsid::new("STA").unwrap(), &placeholder),
        )
    }

    #[test]
    fn renderers_use_hex_ssids_and_derived_psks_only() {
        let config = config();
        let wpa = render_wpa_supplicant(&config.sta);
        let hostapd = render_hostapd(&config.ap);

        assert!(wpa.contains("ssid=535441\n"));
        assert!(hostapd.contains("ctrl_interface=/var/run/hostapd\n"));
        assert!(hostapd.contains("ssid2=415020e7958c\n"));
        assert!(hostapd.contains("country_code=US\n"));
        assert!(!wpa.contains('"'));
        assert!(!hostapd.contains("wpa_passphrase"));
        for output in [&*wpa, &*hostapd] {
            let psk = output
                .lines()
                .find_map(|line| {
                    line.trim()
                        .strip_prefix("psk=")
                        .or_else(|| line.strip_prefix("wpa_psk="))
                })
                .unwrap();
            assert_eq!(psk.len(), 64);
            assert!(psk.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn canonical_json_contains_psks_but_summary_does_not() {
        let config = config();
        let canonical = serde_json::to_string(&config).unwrap();
        let summary = serde_json::to_string(&config.summary()).unwrap();
        assert!(canonical.contains("\"psk\""));
        assert!(!summary.contains("psk"));
        assert!(!canonical.contains("unit-test-placeholder"));
    }

    #[test]
    fn migrates_only_unique_plain_legacy_values() {
        let migrated = migrate_legacy_configs(
            r#"
                ctrl_interface=/run/wpa_supplicant
                network={
                    ssid="upstream"
                    psk="migration-placeholder"
                    key_mgmt=WPA-PSK
                }
            "#,
            r#"
                interface=p2p0
                ssid=downstream
                ssid2=646f776e73747265616d
                country_code=US
                wpa=2
                wpa_passphrase=migration-placeholder
            "#,
        );
        // Both ssid and ssid2 are present, so selecting one would be ambiguous.
        assert!(matches!(
            migrated,
            Err(NetworkConfigError::MigrationRequired(_))
        ));

        let migrated = migrate_legacy_configs(
            "network={\nssid=\"upstream\"\npsk=\"migration-placeholder\"\n}\n",
            "ssid=downstream\ncountry_code=US\nwpa_passphrase=migration-placeholder\n",
        )
        .unwrap();
        assert_eq!(migrated.sta.ssid.as_str(), "upstream");
        assert_eq!(migrated.ap.ssid.as_str(), "downstream");
    }

    #[test]
    fn migration_rejects_escapes_duplicates_and_unknown_country() {
        let valid_hostapd = "ssid2=6170\ncountry_code=US\nwpa_psk=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n";
        let duplicate = "ssid=\"one\"\nssid=\"two\"\npsk=\"migration-placeholder\"\n";
        assert!(matches!(
            migrate_legacy_configs(duplicate, valid_hostapd),
            Err(NetworkConfigError::MigrationRequired(_))
        ));

        let escaped = "ssid=\"up\\stream\"\npsk=\"migration-placeholder\"\n";
        assert!(matches!(
            migrate_legacy_configs(escaped, valid_hostapd),
            Err(NetworkConfigError::MigrationRequired(_))
        ));

        let unknown_country = valid_hostapd.replace("country_code=US", "country_code=ZZ");
        let valid_wpa =
            "ssid=7570\npsk=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n";
        assert!(matches!(
            migrate_legacy_configs(valid_wpa, &unknown_country),
            Err(NetworkConfigError::MigrationRequired(_))
        ));
    }
}
