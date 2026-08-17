//! DHCP hook event wire DTOs (udhcpc callback -> router control socket).

use serde::{Deserialize, Deserializer, Serialize};
use std::net::Ipv4Addr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DhcpUplink {
    Ethernet,
    #[default]
    Wifi,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct DhcpGeneration(String);

impl<'de> Deserialize<'de> for DhcpGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl DhcpGeneration {
    pub fn new(value: String) -> Result<Self, &'static str> {
        if value.is_empty()
            || value.len() > 96
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
        {
            return Err("DHCP generation has invalid characters or length");
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DhcpEvent {
    /// Missing only for the previous router control protocol; it represented Wi-Fi.
    #[serde(default)]
    pub uplink: DhcpUplink,
    pub generation: DhcpGeneration,
    pub transition: DhcpTransition,
}

impl DhcpEvent {
    pub fn new(uplink: DhcpUplink, generation: DhcpGeneration, transition: DhcpTransition) -> Self {
        Self {
            uplink,
            generation,
            transition,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DhcpTransition {
    Deconfig,
    Lease { lease: DhcpLease },
    NoChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DhcpLease {
    pub address: Ipv4Addr,
    pub prefix: u8,
    pub broadcast: Option<Ipv4Addr>,
    pub routers: Vec<Ipv4Addr>,
    pub static_routes: Vec<(String, Ipv4Addr)>,
    pub dns: Vec<Ipv4Addr>,
    pub search: Vec<String>,
}

impl DhcpLease {
    pub fn resolver_lines(&self, interface: &str) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.search.is_empty() {
            lines.push(format!("search {} # {interface}", self.search.join(" ")));
        }
        lines.extend(
            self.dns
                .iter()
                .map(|address| format!("nameserver {address} # {interface}")),
        );
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_uplink_from_the_previous_protocol_maps_only_to_wifi() {
        let event: DhcpEvent =
            serde_json::from_str(r#"{"generation":"wifi-1","transition":{"kind":"no_change"}}"#)
                .unwrap();
        assert_eq!(event.uplink, DhcpUplink::Wifi);
    }

    #[test]
    fn ethernet_uplink_round_trips_as_a_fixed_value() {
        let event = DhcpEvent::new(
            DhcpUplink::Ethernet,
            DhcpGeneration::new("ethernet-1".to_owned()).unwrap(),
            DhcpTransition::NoChange,
        );
        let encoded = serde_json::to_string(&event).unwrap();
        assert!(encoded.contains("\"uplink\":\"ethernet\""));
        assert_eq!(serde_json::from_str::<DhcpEvent>(&encoded).unwrap(), event);
    }
}
