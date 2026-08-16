//! Tailscale wire DTOs: modes, backend state, peer snapshots and login URLs.

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, net::Ipv4Addr};

pub const MAX_TAILSCALE_PEERS: usize = 128;
pub const MAX_TAILSCALE_PEER_NAME_BYTES: usize = 64;
pub const MAX_TAILSCALE_PEER_OS_BYTES: usize = 32;
/// Fixed TCP port for the management-plane HTTP listener on the Tailscale
/// interface. The router firewall opens it; the portal binds it.
pub const TAILSCALE_MANAGEMENT_HTTP_PORT: u16 = 8080;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TailscalePeer {
    pub name: String,
    pub ipv4: Ipv4Addr,
    pub online: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
}

impl TailscalePeer {
    pub fn new(
        name: impl Into<String>,
        ipv4: Ipv4Addr,
        online: bool,
        os: Option<String>,
    ) -> Option<Self> {
        let name = name.into();
        if !valid_peer_text(&name, MAX_TAILSCALE_PEER_NAME_BYTES)
            || !is_tailscale_cgnat_ipv4(ipv4)
            || os
                .as_deref()
                .is_some_and(|value| !valid_peer_text(value, MAX_TAILSCALE_PEER_OS_BYTES))
        {
            return None;
        }
        Some(Self {
            name,
            ipv4,
            online,
            os,
        })
    }
}

impl<'de> Deserialize<'de> for TailscalePeer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WirePeer {
            name: String,
            ipv4: Ipv4Addr,
            online: bool,
            os: Option<String>,
        }

        let wire = WirePeer::deserialize(deserializer)?;
        Self::new(wire.name, wire.ipv4, wire.online, wire.os)
            .ok_or_else(|| D::Error::custom("invalid Tailscale peer"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TailscalePeerSnapshot {
    pub total: usize,
    pub online: usize,
    pub peers: Vec<TailscalePeer>,
}

impl TailscalePeerSnapshot {
    pub fn new(mut peers: Vec<TailscalePeer>) -> Option<Self> {
        if peers.len() > MAX_TAILSCALE_PEERS {
            return None;
        }
        peers.sort_by(|left, right| {
            right
                .online
                .cmp(&left.online)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.ipv4.cmp(&right.ipv4))
        });
        let unique_addresses = peers
            .iter()
            .map(|peer| peer.ipv4)
            .collect::<std::collections::BTreeSet<_>>();
        if unique_addresses.len() != peers.len() {
            return None;
        }
        let online = peers.iter().filter(|peer| peer.online).count();
        Some(Self {
            total: peers.len(),
            online,
            peers,
        })
    }

    pub fn empty() -> Self {
        Self {
            total: 0,
            online: 0,
            peers: Vec::new(),
        }
    }
}

impl<'de> Deserialize<'de> for TailscalePeerSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireSnapshot {
            total: usize,
            online: usize,
            peers: Vec<TailscalePeer>,
        }

        let wire = WireSnapshot::deserialize(deserializer)?;
        let snapshot = Self::new(wire.peers)
            .ok_or_else(|| D::Error::custom("invalid Tailscale peer snapshot"))?;
        if snapshot.total != wire.total || snapshot.online != wire.online {
            return Err(D::Error::custom("inconsistent Tailscale peer counts"));
        }
        Ok(snapshot)
    }
}

fn valid_peer_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn is_tailscale_cgnat_ipv4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TailscaleMode {
    Disabled,
    RouterOnly,
    LanSubnetAccess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TailscaleBackendState {
    Stopped,
    NeedsLogin,
    Running,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TailscaleEnvironment {
    Direct,
    MihomoExplicit,
}

#[derive(Clone, PartialEq, Eq)]
pub struct TailscaleLoginUrl(String);

impl fmt::Debug for TailscaleLoginUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TailscaleLoginUrl([REDACTED])")
    }
}

impl Serialize for TailscaleLoginUrl {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TailscaleLoginUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).ok_or_else(|| D::Error::custom("invalid Tailscale login URL"))
    }
}

impl TailscaleLoginUrl {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        if value.len() <= 2_048 && value.starts_with("https://login.tailscale.com/") {
            Some(Self(value))
        } else {
            None
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_snapshot_enforces_bounds_uniqueness_and_stable_order() {
        let offline = TailscalePeer::new(
            "tablet",
            Ipv4Addr::new(100, 64, 0, 9),
            false,
            Some("android".to_owned()),
        )
        .unwrap();
        let online = TailscalePeer::new(
            "laptop",
            Ipv4Addr::new(100, 64, 0, 8),
            true,
            Some("linux".to_owned()),
        )
        .unwrap();
        let snapshot = TailscalePeerSnapshot::new(vec![offline, online.clone()]).unwrap();
        assert_eq!(snapshot.total, 2);
        assert_eq!(snapshot.online, 1);
        assert_eq!(snapshot.peers[0], online);

        assert!(TailscalePeer::new(" bad ", Ipv4Addr::new(100, 64, 0, 10), true, None,).is_none());
        assert!(TailscalePeer::new("public", Ipv4Addr::new(192, 0, 2, 1), true, None,).is_none());
        let duplicate =
            TailscalePeer::new("phone", Ipv4Addr::new(100, 64, 0, 8), false, None).unwrap();
        assert!(TailscalePeerSnapshot::new(vec![online, duplicate]).is_none());
        assert!(serde_json::from_str::<TailscalePeerSnapshot>(
            r#"{"total":2,"online":1,"peers":[]}"#
        )
        .is_err());
        assert!(serde_json::from_str::<TailscalePeer>(
            r#"{"name":"public","ipv4":"192.0.2.1","online":true}"#
        )
        .is_err());
    }
}
