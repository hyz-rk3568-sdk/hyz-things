//! Per-LAN-device routing policy wire DTOs.

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use std::{collections::BTreeSet, fmt, net::Ipv4Addr, str::FromStr};

use crate::activity::LanActivitySnapshot;

pub const DEVICE_POLICY_VERSION: u8 = 1;
pub const MAX_DEVICE_POLICIES: usize = 32;
pub const MAX_DEVICE_LABEL_BYTES: usize = 32;
pub const MAX_LAN_HOSTNAME_BYTES: usize = 63;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LanDeviceMac([u8; 6]);

impl LanDeviceMac {
    pub const fn octets(self) -> [u8; 6] {
        self.0
    }
}

impl fmt::Display for LanDeviceMac {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl FromStr for LanDeviceMac {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != 17 || value.bytes().any(|byte| byte.is_ascii_control()) {
            return Err("MAC must use six colon-separated hexadecimal octets");
        }
        let mut octets = [0u8; 6];
        let mut parts = value.split(':');
        for octet in &mut octets {
            let part = parts.next().ok_or("MAC has too few octets")?;
            if part.len() != 2 || !part.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("MAC octet is invalid");
            }
            *octet = u8::from_str_radix(part, 16).map_err(|_| "MAC octet is invalid")?;
        }
        if parts.next().is_some() {
            return Err("MAC has too many octets");
        }
        if octets == [0; 6] || octets == [0xff; 6] || octets[0] & 1 != 0 {
            return Err("MAC must be a non-zero unicast address");
        }
        Ok(Self(octets))
    }
}

impl Serialize for LanDeviceMac {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for LanDeviceMac {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LanDeviceLabel(String);

impl LanDeviceLabel {
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.len() > MAX_DEVICE_LABEL_BYTES
            || value.chars().any(char::is_control)
            || value.trim() != value
        {
            return Err("label must be at most 32 UTF-8 bytes, trimmed, and contain no controls");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for LanDeviceLabel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LanDeviceLabel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRoutePolicy {
    Direct,
    Proxy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevicePolicyEntry {
    pub mac: LanDeviceMac,
    pub label: LanDeviceLabel,
    pub policy: DeviceRoutePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DevicePolicyConfigV1 {
    pub version: u8,
    pub generation: u64,
    pub entries: Vec<DevicePolicyEntry>,
}

impl DevicePolicyConfigV1 {
    pub fn empty() -> Self {
        Self {
            version: DEVICE_POLICY_VERSION,
            generation: 0,
            entries: Vec::new(),
        }
    }

    pub fn new(generation: u64, mut entries: Vec<DevicePolicyEntry>) -> Result<Self, &'static str> {
        if entries.len() > MAX_DEVICE_POLICIES {
            return Err("at most 32 device policies are allowed");
        }
        entries.sort_by_key(|entry| entry.mac);
        if entries.windows(2).any(|pair| pair[0].mac == pair[1].mac) {
            return Err("device policy MAC addresses must be unique");
        }
        Ok(Self {
            version: DEVICE_POLICY_VERSION,
            generation,
            entries,
        })
    }

    pub fn direct_macs(&self) -> BTreeSet<LanDeviceMac> {
        self.entries
            .iter()
            .filter(|entry| entry.policy == DeviceRoutePolicy::Direct)
            .map(|entry| entry.mac)
            .collect()
    }

    pub fn policy_for(&self, mac: LanDeviceMac) -> DeviceRoutePolicy {
        self.entries
            .binary_search_by_key(&mac, |entry| entry.mac)
            .ok()
            .map(|index| self.entries[index].policy)
            .unwrap_or(DeviceRoutePolicy::Proxy)
    }
}

impl<'de> Deserialize<'de> for DevicePolicyConfigV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            version: u8,
            generation: u64,
            entries: Vec<DevicePolicyEntry>,
        }
        let wire = Wire::deserialize(deserializer)?;
        if wire.version != DEVICE_POLICY_VERSION {
            return Err(D::Error::custom("unsupported device policy version"));
        }
        let entries = wire.entries.clone();
        let config = Self::new(wire.generation, wire.entries).map_err(D::Error::custom)?;
        // Persisted and protocol representations are canonical, not merely normalizable.
        if config.entries != entries {
            return Err(D::Error::custom("device policy entries are not canonical"));
        }
        Ok(config)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevicePolicyUpdateRequest {
    pub expected_generation: u64,
    pub entries: Vec<DevicePolicyEntry>,
}

impl DevicePolicyUpdateRequest {
    pub fn candidate(self) -> Result<DevicePolicyConfigV1, &'static str> {
        DevicePolicyConfigV1::new(
            self.expected_generation
                .checked_add(1)
                .ok_or("device policy generation overflow")?,
            self.entries,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanClientObservation {
    pub mac: LanDeviceMac,
    pub lease_address: Option<Ipv4Addr>,
    pub hostname: Option<String>,
    pub associated: bool,
    pub policy: DeviceRoutePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevicePolicySnapshot {
    pub config: DevicePolicyConfigV1,
    pub clients: Vec<LanClientObservation>,
    pub effective: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<LanActivitySnapshot>,
}

pub fn validate_hostname(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_LAN_HOSTNAME_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(mac: &str, policy: DeviceRoutePolicy) -> DevicePolicyEntry {
        DevicePolicyEntry {
            mac: mac.parse().unwrap(),
            label: LanDeviceLabel::new("phone").unwrap(),
            policy,
        }
    }

    #[test]
    fn mac_validation_canonicalizes_private_addresses_and_rejects_non_unicast() {
        assert_eq!(
            "02:AA:00:00:00:01"
                .parse::<LanDeviceMac>()
                .unwrap()
                .to_string(),
            "02:aa:00:00:00:01"
        );
        for invalid in [
            "00:00:00:00:00:00",
            "ff:ff:ff:ff:ff:ff",
            "01:00:00:00:00:00",
            "02-00-00-00-00-01",
            "02:00:00:00:00:\n",
        ] {
            assert!(invalid.parse::<LanDeviceMac>().is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn config_sorts_defaults_to_proxy_and_rejects_duplicates_and_excess() {
        let config = DevicePolicyConfigV1::new(
            4,
            vec![
                entry("02:00:00:00:00:02", DeviceRoutePolicy::Direct),
                entry("02:00:00:00:00:01", DeviceRoutePolicy::Proxy),
            ],
        )
        .unwrap();
        assert_eq!(config.entries[0].mac.to_string(), "02:00:00:00:00:01");
        assert_eq!(
            config.policy_for("02:00:00:00:00:03".parse().unwrap()),
            DeviceRoutePolicy::Proxy
        );
        assert!(DevicePolicyConfigV1::new(
            0,
            vec![entry("02:00:00:00:00:01", DeviceRoutePolicy::Direct); 2]
        )
        .is_err());
        assert!(DevicePolicyConfigV1::new(
            0,
            (0..=MAX_DEVICE_POLICIES)
                .map(|index| entry(
                    &format!("02:00:00:00:00:{index:02x}"),
                    DeviceRoutePolicy::Direct
                ))
                .collect()
        )
        .is_err());
    }

    #[test]
    fn default_proxy_entries_may_persist_a_stable_display_label() {
        let config = DevicePolicyConfigV1::new(
            1,
            vec![DevicePolicyEntry {
                mac: "02:00:00:00:00:01".parse().unwrap(),
                label: LanDeviceLabel::new("我的 iPhone").unwrap(),
                policy: DeviceRoutePolicy::Proxy,
            }],
        )
        .unwrap();
        assert_eq!(config.entries[0].label.as_str(), "我的 iPhone");
        assert_eq!(
            config.policy_for("02:00:00:00:00:01".parse().unwrap()),
            DeviceRoutePolicy::Proxy
        );
        assert!(config.direct_macs().is_empty());
    }

    #[test]
    fn json_rejects_unknown_fields_bad_version_and_noncanonical_order() {
        assert!(serde_json::from_str::<DevicePolicyConfigV1>(
            r#"{"version":2,"generation":0,"entries":[]}"#
        )
        .is_err());
        assert!(serde_json::from_str::<DevicePolicyConfigV1>(
            r#"{"version":1,"generation":0,"entries":[],"rules":[]}"#
        )
        .is_err());
        let sorted = r#"{"version":1,"generation":0,"entries":[{"mac":"02:00:00:00:00:01","label":"","policy":"proxy"},{"mac":"02:00:00:00:00:02","label":"","policy":"direct"}]}"#;
        let unsorted = r#"{"version":1,"generation":0,"entries":[{"mac":"02:00:00:00:00:02","label":"","policy":"direct"},{"mac":"02:00:00:00:00:01","label":"","policy":"proxy"}]}"#;
        assert!(serde_json::from_str::<DevicePolicyConfigV1>(sorted).is_ok());
        assert!(serde_json::from_str::<DevicePolicyConfigV1>(unsorted).is_err());
    }

    #[test]
    fn maximum_update_request_fits_the_fixed_http_json_limit() {
        let entries = (0..MAX_DEVICE_POLICIES)
            .map(|index| DevicePolicyEntry {
                mac: format!("02:00:00:00:00:{index:02x}").parse().unwrap(),
                label: LanDeviceLabel::new("\"".repeat(MAX_DEVICE_LABEL_BYTES)).unwrap(),
                policy: DeviceRoutePolicy::Direct,
            })
            .collect();
        let request = DevicePolicyUpdateRequest {
            expected_generation: u64::MAX - 1,
            entries,
        };
        assert!(serde_json::to_vec(&request).unwrap().len() <= 4 * 1024);
    }
}
