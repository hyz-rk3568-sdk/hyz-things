//! Pure Tailscale lifecycle state and readiness for fixed remote LAN access.

use super::network::{OwnedResource, Probe};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize, Serializer};
use std::{fmt, net::Ipv4Addr};

pub const TAILSCALE_INTERFACE: &str = "tailscale0";
pub const TAILSCALE_LAN_ROUTE: &str = "192.168.8.0/24";
pub const TAILSCALE_CGNAT_SUBNET: &str = "100.64.0.0/10";
pub const TAILSCALE_UDP_PORT: u16 = 41_641;
pub const TAILSCALE_MANAGEMENT_HTTP_PORT: u16 = 8080;
pub const TAILSCALE_INPUT_CHAIN: &str = "HYZ_TS_INPUT";
pub const TAILSCALE_FORWARD_CHAIN: &str = "HYZ_TS_FWD";
pub const TAILSCALE_NAT_CHAIN: &str = "HYZ_TS_NAT";
pub const MAX_TAILSCALE_PEERS: usize = 128;
pub const MAX_TAILSCALE_PEER_NAME_BYTES: usize = 64;
pub const MAX_TAILSCALE_PEER_OS_BYTES: usize = 32;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailscaleProcessState {
    Absent,
    OwnedLive { token: String },
    OwnedExited { token: String },
    Foreign,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailscaleConnectionKind {
    Direct,
    PeerRelay,
    Derp(String),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TailscalePreferences {
    pub accept_dns: bool,
    pub accept_routes: bool,
    pub advertise_exit_node: bool,
    pub exit_node_selected: bool,
    pub ssh_enabled: bool,
    pub netfilter_off: bool,
}

impl TailscalePreferences {
    pub const FIXED: Self = Self {
        accept_dns: false,
        accept_routes: false,
        advertise_exit_node: false,
        exit_node_selected: false,
        ssh_enabled: false,
        netfilter_off: true,
    };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleDesired {
    pub mode: TailscaleMode,
}

impl TailscaleDesired {
    pub const fn disabled() -> Self {
        Self {
            mode: TailscaleMode::Disabled,
        }
    }

    pub const fn router_only() -> Self {
        Self {
            mode: TailscaleMode::RouterOnly,
        }
    }

    pub const fn lan_subnet_access() -> Self {
        Self {
            mode: TailscaleMode::LanSubnetAccess,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleObserved {
    pub persisted_mode: Probe<Option<TailscaleMode>>,
    pub backend_state: Probe<TailscaleBackendState>,
    pub process: Probe<TailscaleProcessState>,
    pub environment: Probe<TailscaleEnvironment>,
    pub socket: Probe<OwnedResource>,
    pub interface: Probe<OwnedResource>,
    pub authenticated: Probe<bool>,
    pub ipv4: Probe<Option<Ipv4Addr>>,
    pub preferences: Probe<TailscalePreferences>,
    pub route_advertised: Probe<bool>,
    pub router_firewall: Probe<OwnedResource>,
    pub subnet_firewall: Probe<OwnedResource>,
    pub management_listener: Probe<OwnedResource>,
    pub management_listener_ipv4: Probe<Option<Ipv4Addr>>,
    pub connection: Probe<TailscaleConnectionKind>,
}

impl TailscaleObserved {
    pub fn unknown(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            persisted_mode: Probe::Unknown(reason.clone()),
            backend_state: Probe::Unknown(reason.clone()),
            process: Probe::Unknown(reason.clone()),
            environment: Probe::Unknown(reason.clone()),
            socket: Probe::Unknown(reason.clone()),
            interface: Probe::Unknown(reason.clone()),
            authenticated: Probe::Unknown(reason.clone()),
            ipv4: Probe::Unknown(reason.clone()),
            preferences: Probe::Unknown(reason.clone()),
            route_advertised: Probe::Unknown(reason.clone()),
            router_firewall: Probe::Unknown(reason.clone()),
            subnet_firewall: Probe::Unknown(reason.clone()),
            management_listener: Probe::Unknown(reason.clone()),
            management_listener_ipv4: Probe::Unknown(reason.clone()),
            connection: Probe::Unknown(reason),
        }
    }

    pub fn readiness(
        &self,
        desired: &TailscaleDesired,
        ordinary_router_ready: bool,
    ) -> TailscaleReadiness {
        if self.persisted_mode != Probe::Known(Some(desired.mode)) {
            return TailscaleReadiness::NotReady;
        }
        if desired.mode != TailscaleMode::Disabled
            && self.authenticated == Probe::Known(false)
            && self.backend_state == Probe::Known(TailscaleBackendState::NeedsLogin)
            && self.login_surface_absent()
        {
            return TailscaleReadiness::NeedsLogin;
        }
        match desired.mode {
            TailscaleMode::Disabled if self.disabled_ready() => TailscaleReadiness::Ready {
                effective_mode: TailscaleMode::Disabled,
            },
            TailscaleMode::RouterOnly if self.router_only_ready() => TailscaleReadiness::Ready {
                effective_mode: TailscaleMode::RouterOnly,
            },
            TailscaleMode::LanSubnetAccess if ordinary_router_ready && self.lan_subnet_ready() => {
                TailscaleReadiness::Ready {
                    effective_mode: TailscaleMode::LanSubnetAccess,
                }
            }
            TailscaleMode::LanSubnetAccess if self.router_only_ready() => {
                TailscaleReadiness::Ready {
                    effective_mode: TailscaleMode::RouterOnly,
                }
            }
            _ => TailscaleReadiness::NotReady,
        }
    }

    pub fn ready_for(&self, desired: &TailscaleDesired, ordinary_router_ready: bool) -> bool {
        matches!(
            self.readiness(desired, ordinary_router_ready),
            TailscaleReadiness::Ready { .. }
        )
    }

    fn disabled_ready(&self) -> bool {
        self.backend_state == Probe::Known(TailscaleBackendState::Stopped)
            && self.process == Probe::Known(TailscaleProcessState::Absent)
            && self.socket == Probe::Known(OwnedResource::Absent)
            && self.interface == Probe::Known(OwnedResource::Absent)
            && self.route_advertised == Probe::Known(false)
            && self.router_firewall == Probe::Known(OwnedResource::Absent)
            && self.subnet_firewall == Probe::Known(OwnedResource::Absent)
            && self.management_listener == Probe::Known(OwnedResource::Absent)
            && self.management_listener_ipv4 == Probe::Known(None)
    }

    fn router_only_ready(&self) -> bool {
        self.enabled_base_ready()
            && self.route_advertised == Probe::Known(false)
            && self.subnet_firewall == Probe::Known(OwnedResource::Absent)
    }

    fn lan_subnet_ready(&self) -> bool {
        self.enabled_base_ready()
            && self.route_advertised == Probe::Known(true)
            && matches!(
                &self.subnet_firewall,
                Probe::Known(OwnedResource::Owned { .. })
            )
    }

    fn enabled_base_ready(&self) -> bool {
        self.backend_state == Probe::Known(TailscaleBackendState::Running)
            && matches!(self.environment, Probe::Known(_))
            && matches!(
                (&self.process, &self.socket, &self.interface),
                (
                    Probe::Known(TailscaleProcessState::OwnedLive { token: process }),
                    Probe::Known(OwnedResource::Owned { token: socket }),
                    Probe::Known(OwnedResource::Owned { token: interface }),
                ) if process == socket && process == interface
            )
            && self.authenticated == Probe::Known(true)
            && matches!(&self.ipv4, Probe::Known(Some(_)))
            && self.preferences == Probe::Known(TailscalePreferences::FIXED)
            && matches!(
                self.router_firewall,
                Probe::Known(OwnedResource::Owned { .. })
            )
            && matches!(
                self.management_listener,
                Probe::Known(OwnedResource::Owned { .. })
            )
            && matches!(
                (&self.ipv4, &self.management_listener_ipv4),
                (Probe::Known(Some(ipv4)), Probe::Known(Some(listener_ipv4)))
                    if ipv4 == listener_ipv4
            )
    }

    fn login_surface_absent(&self) -> bool {
        self.route_advertised == Probe::Known(false)
            && self.router_firewall == Probe::Known(OwnedResource::Absent)
            && self.subnet_firewall == Probe::Known(OwnedResource::Absent)
            && self.management_listener == Probe::Known(OwnedResource::Absent)
            && self.management_listener_ipv4 == Probe::Known(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailscaleReadiness {
    Ready { effective_mode: TailscaleMode },
    NeedsLogin,
    NotReady,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailscaleAction {
    StartBackend {
        token: String,
        environment: TailscaleEnvironment,
    },
    WaitForBackend,
    StopBackend {
        token: String,
    },
    SetFixedPreferences,
    RestorePreferences {
        preferences: TailscalePreferences,
    },
    AdvertiseLanRoute,
    ClearAdvertisedRoute,
    InstallRouterFirewall {
        token: String,
    },
    RemoveRouterFirewall {
        token: String,
    },
    InstallSubnetFirewall {
        token: String,
    },
    RemoveSubnetFirewall {
        token: String,
    },
    StartManagementListener {
        token: String,
        ipv4: Ipv4Addr,
    },
    StopManagementListener {
        token: String,
    },
    CommitDesiredMode {
        mode: TailscaleMode,
    },
    RestoreDesiredMode {
        mode: Option<TailscaleMode>,
    },
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
