//! Tailscale wire DTOs: modes, backend state, peer snapshots and login URLs.

use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fmt,
    net::{Ipv4Addr, SocketAddr},
};

pub const MAX_TAILSCALE_PEERS: usize = 128;
pub const MAX_TAILSCALE_PEER_NAME_BYTES: usize = 64;
pub const MAX_TAILSCALE_PEER_OS_BYTES: usize = 32;
pub const MAX_TAILSCALE_RELAY_BYTES: usize = 32;
/// Fixed TCP port for the management-plane HTTP listener on the Tailscale
/// interface. The router firewall opens it; the portal binds it.
pub const TAILSCALE_MANAGEMENT_HTTP_PORT: u16 = 8080;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TailscalePeerConnection {
    Direct { address: SocketAddr },
    Relay { region: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TailscalePeer {
    pub name: String,
    pub ipv4: Ipv4Addr,
    pub online: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<TailscalePeerConnection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rx_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_bytes: Option<u64>,
}

impl TailscalePeer {
    pub fn new(
        name: impl Into<String>,
        ipv4: Ipv4Addr,
        online: bool,
        os: Option<String>,
    ) -> Option<Self> {
        Self::with_details(name, ipv4, online, os, None, None, None, None, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_details(
        name: impl Into<String>,
        ipv4: Ipv4Addr,
        online: bool,
        os: Option<String>,
        active: Option<bool>,
        connection: Option<TailscalePeerConnection>,
        last_seen_unix_ms: Option<u64>,
        rx_bytes: Option<u64>,
        tx_bytes: Option<u64>,
    ) -> Option<Self> {
        let name = name.into();
        if !valid_peer_text(&name, MAX_TAILSCALE_PEER_NAME_BYTES)
            || !is_tailscale_cgnat_ipv4(ipv4)
            || os
                .as_deref()
                .is_some_and(|value| !valid_peer_text(value, MAX_TAILSCALE_PEER_OS_BYTES))
            || connection
                .as_ref()
                .is_some_and(|value| !valid_peer_connection(value))
        {
            return None;
        }
        Some(Self {
            name,
            ipv4,
            online,
            os,
            active,
            connection,
            last_seen_unix_ms,
            rx_bytes,
            tx_bytes,
        })
    }
}

fn valid_peer_connection(connection: &TailscalePeerConnection) -> bool {
    match connection {
        TailscalePeerConnection::Direct { .. } => true,
        TailscalePeerConnection::Relay { region } => {
            valid_peer_text(region, MAX_TAILSCALE_RELAY_BYTES)
        }
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
            #[serde(default)]
            active: Option<bool>,
            #[serde(default)]
            connection: Option<TailscalePeerConnection>,
            #[serde(default)]
            last_seen_unix_ms: Option<u64>,
            #[serde(default)]
            rx_bytes: Option<u64>,
            #[serde(default)]
            tx_bytes: Option<u64>,
        }

        let wire = WirePeer::deserialize(deserializer)?;
        Self::with_details(
            wire.name,
            wire.ipv4,
            wire.online,
            wire.os,
            wire.active,
            wire.connection,
            wire.last_seen_unix_ms,
            wire.rx_bytes,
            wire.tx_bytes,
        )
        .ok_or_else(|| D::Error::custom("invalid Tailscale peer"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TailscalePeerSnapshot {
    pub total: usize,
    pub online: usize,
    pub peers: Vec<TailscalePeer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_node: Option<TailscalePeer>,
}

impl TailscalePeerSnapshot {
    pub fn new(peers: Vec<TailscalePeer>) -> Option<Self> {
        Self::new_with_self(None, peers)
    }

    pub fn new_with_self(
        self_node: Option<TailscalePeer>,
        mut peers: Vec<TailscalePeer>,
    ) -> Option<Self> {
        if peers.len() > MAX_TAILSCALE_PEERS {
            return None;
        }
        if self_node
            .as_ref()
            .is_some_and(|local| peers.iter().any(|peer| peer.ipv4 == local.ipv4))
        {
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
            self_node,
        })
    }

    pub fn device_total(&self) -> usize {
        self.total + usize::from(self.self_node.is_some())
    }

    pub fn device_online(&self) -> usize {
        self.online + usize::from(self.self_node.as_ref().is_some_and(|node| node.online))
    }

    pub fn empty() -> Self {
        Self {
            total: 0,
            online: 0,
            peers: Vec::new(),
            self_node: None,
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
            #[serde(default)]
            self_node: Option<TailscalePeer>,
        }

        let wire = WireSnapshot::deserialize(deserializer)?;
        let snapshot = Self::new_with_self(wire.self_node, wire.peers)
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
    fn peer_snapshot_preserves_optional_connection_details_and_local_node() {
        let local = TailscalePeer::with_details(
            "hyz-things",
            Ipv4Addr::new(100, 64, 0, 7),
            false,
            Some("linux".to_owned()),
            Some(false),
            None,
            Some(1_786_000_000_000),
            Some(123),
            Some(456),
        )
        .unwrap();
        let peer = TailscalePeer::with_details(
            "desktop",
            Ipv4Addr::new(100, 64, 0, 8),
            true,
            Some("windows".to_owned()),
            Some(true),
            Some(TailscalePeerConnection::Direct {
                address: "192.168.1.3:41641".parse().unwrap(),
            }),
            None,
            Some(789),
            Some(1_234),
        )
        .unwrap();
        let snapshot =
            TailscalePeerSnapshot::new_with_self(Some(local.clone()), vec![peer.clone()]).unwrap();

        assert_eq!(snapshot.device_total(), 2);
        assert_eq!(snapshot.device_online(), 1);
        assert_eq!(snapshot.self_node, Some(local));
        assert_eq!(snapshot.peers, vec![peer]);

        let encoded = serde_json::to_string(&snapshot).unwrap();
        let decoded: TailscalePeerSnapshot = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn legacy_peer_snapshot_without_optional_details_remains_valid() {
        let decoded: TailscalePeerSnapshot = serde_json::from_str(
            r#"{"total":1,"online":1,"peers":[{"name":"laptop","ipv4":"100.64.0.8","online":true,"os":"linux"}]}"#,
        )
        .unwrap();
        assert_eq!(decoded.device_total(), 1);
        assert_eq!(decoded.peers[0].active, None);
        assert_eq!(decoded.peers[0].connection, None);
    }

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
