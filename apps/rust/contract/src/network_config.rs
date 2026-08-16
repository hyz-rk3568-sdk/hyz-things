//! Wi-Fi credential and network-config summary wire DTOs.
//!
//! Full persisted configs (`ApConfig`, `StaConfig`, `NetworkConfigV1`) stay in
//! the router; only the typed credential values they embed and the wire
//! summaries are shared here.

use pbkdf2::pbkdf2_hmac;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use sha1::Sha1;
use std::{error::Error, fmt};
use zeroize::Zeroizing;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiConfigError(&'static str);

impl WifiConfigError {
    pub const fn new(message: &'static str) -> Self {
        Self(message)
    }
}

impl fmt::Display for WifiConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for WifiConfigError {}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct WifiSsid(String);

impl WifiSsid {
    pub fn new(value: impl Into<String>) -> Result<Self, WifiConfigError> {
        let value = value.into();
        if value.is_empty() || value.len() > 32 {
            return Err(WifiConfigError::new("SSID must contain 1-32 UTF-8 bytes"));
        }
        if value.chars().any(char::is_control) {
            return Err(WifiConfigError::new(
                "SSID must not contain control characters",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Debug for WifiSsid {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("WifiSsid").field(&self.0).finish()
    }
}

impl Serialize for WifiSsid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for WifiSsid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

pub struct WifiPassphrase(Zeroizing<String>);

impl Clone for WifiPassphrase {
    fn clone(&self) -> Self {
        Self(Zeroizing::new(self.0.to_string()))
    }
}

impl Serialize for WifiPassphrase {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for WifiPassphrase {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

impl WifiPassphrase {
    pub fn new(value: impl Into<String>) -> Result<Self, WifiConfigError> {
        let value = Zeroizing::new(value.into());
        if !(8..=63).contains(&value.len()) {
            return Err(WifiConfigError::new(
                "WPA2 passphrase must contain 8-63 ASCII characters",
            ));
        }
        if !value
            .bytes()
            .all(|byte| byte == b' ' || byte.is_ascii_graphic())
        {
            return Err(WifiConfigError::new(
                "WPA2 passphrase must contain printable ASCII only",
            ));
        }
        Ok(Self(value))
    }

    pub fn derive_psk(&self, ssid: &WifiSsid) -> DerivedPsk {
        let mut bytes = Zeroizing::new([0_u8; 32]);
        pbkdf2_hmac::<Sha1>(self.0.as_bytes(), ssid.as_bytes(), 4096, bytes.as_mut());
        DerivedPsk(bytes)
    }
}

impl fmt::Debug for WifiPassphrase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WifiPassphrase([REDACTED])")
    }
}

impl PartialEq for WifiPassphrase {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_bytes() == other.0.as_bytes()
    }
}

impl Eq for WifiPassphrase {}

pub struct DerivedPsk(Zeroizing<[u8; 32]>);

impl DerivedPsk {
    pub fn from_hex(value: &str) -> Result<Self, WifiConfigError> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(WifiConfigError::new(
                "WPA2 PSK must contain exactly 64 hexadecimal characters",
            ));
        }
        let mut bytes = Zeroizing::new([0_u8; 32]);
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            bytes[index] = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
        }
        Ok(Self(bytes))
    }

    pub fn to_hex(&self) -> Zeroizing<String> {
        let mut output = Zeroizing::new(String::with_capacity(64));
        for byte in self.0.iter() {
            use fmt::Write as _;
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
        }
        output
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

impl Clone for DerivedPsk {
    fn clone(&self) -> Self {
        Self(Zeroizing::new(*self.0))
    }
}

impl PartialEq for DerivedPsk {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_slice() == other.0.as_slice()
    }
}

impl Eq for DerivedPsk {}

impl fmt::Debug for DerivedPsk {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DerivedPsk([REDACTED])")
    }
}

impl Serialize for DerivedPsk {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for DerivedPsk {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Zeroizing::new(String::deserialize(deserializer)?);
        Self::from_hex(&value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WifiCountry {
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

impl WifiCountry {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Au => "AU",
            Self::Br => "BR",
            Self::Ca => "CA",
            Self::Cn => "CN",
            Self::De => "DE",
            Self::Fr => "FR",
            Self::Gb => "GB",
            Self::In => "IN",
            Self::Jp => "JP",
            Self::Kr => "KR",
            Self::Nz => "NZ",
            Self::Sg => "SG",
            Self::Tw => "TW",
            Self::Us => "US",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkConfigSummary {
    pub version: u8,
    pub ap_ssid: WifiSsid,
    pub sta_ssid: WifiSsid,
    pub country: WifiCountry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingNetworkConfigSummary {
    pub version: u8,
    pub staged_at_unix_ms: u64,
    pub config: NetworkConfigSummary,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssid_enforces_utf8_byte_limit_and_control_rejection() {
        assert!(WifiSsid::new("").is_err());
        assert!(WifiSsid::new("a".repeat(32)).is_ok());
        assert!(WifiSsid::new("界".repeat(10)).is_ok());
        assert!(WifiSsid::new("界".repeat(11)).is_err());
        assert!(WifiSsid::new("bad\nssid").is_err());
        assert!(WifiSsid::new("bad\0ssid").is_err());
    }

    #[test]
    fn credentials_are_strictly_typed_and_redacted() {
        assert!(WifiPassphrase::new("1234567").is_err());
        assert!(WifiPassphrase::new("x".repeat(63)).is_ok());
        assert!(WifiPassphrase::new("x".repeat(64)).is_err());
        assert!(WifiPassphrase::new("密码密码密码密码").is_err());
        assert!(DerivedPsk::from_hex(&"a".repeat(64)).is_ok());
        assert!(DerivedPsk::from_hex(&"g".repeat(64)).is_err());
        assert_eq!(
            format!("{:?}", WifiPassphrase::new("not-a-real-secret").unwrap()),
            "WifiPassphrase([REDACTED])"
        );
    }

    #[test]
    fn derives_standard_wpa2_psk_vector() {
        let ssid = WifiSsid::new("IEEE").unwrap();
        let passphrase = WifiPassphrase::new("password").unwrap();
        assert_eq!(
            passphrase.derive_psk(&ssid).to_hex().as_str(),
            "f42c6fc52df0ebef9ebb4b90b38a5f90\
             2e83fe1b135a70e23aed762e9710a12e"
        );
    }

    #[test]
    fn deserialization_revalidates_typed_values() {
        let malformed = r#"{"version":1,"ap_ssid":"bad\nssid","sta_ssid":"ok","country":"US"}"#;
        assert!(serde_json::from_str::<NetworkConfigSummary>(malformed).is_err());
    }
}
