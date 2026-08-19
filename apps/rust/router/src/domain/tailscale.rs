//! Pure Tailscale lifecycle state and readiness for fixed remote LAN access.
//! Wire DTOs (modes, peer snapshots, login URLs) are shared via `hyz-contract`;
//! this module keeps the router-side observed/desired state machine.

use std::net::Ipv4Addr;

use super::network::{OwnedResource, Probe};

pub use hyz_contract::tailscale::{
    TailscaleBackendState, TailscaleEnvironment, TailscaleLoginUrl, TailscaleMode, TailscalePeer,
    TailscalePeerSnapshot, MAX_TAILSCALE_PEERS, MAX_TAILSCALE_PEER_NAME_BYTES,
    MAX_TAILSCALE_PEER_OS_BYTES, TAILSCALE_MANAGEMENT_HTTP_PORT,
};

pub const TAILSCALE_INTERFACE: &str = "tailscale0";
pub const TAILSCALE_LAN_ROUTE: &str = "192.168.8.0/24";
pub const TAILSCALE_CGNAT_SUBNET: &str = "100.64.0.0/10";
pub const TAILSCALE_UDP_PORT: u16 = 41_641;
pub const TAILSCALE_ADB_PORT: u16 = 5_555;
pub const TAILSCALE_INPUT_CHAIN: &str = "HYZ_TS_INPUT";
pub const TAILSCALE_FORWARD_CHAIN: &str = "HYZ_TS_FWD";
pub const TAILSCALE_NAT_CHAIN: &str = "HYZ_TS_NAT";

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
    }

    fn login_surface_absent(&self) -> bool {
        self.route_advertised == Probe::Known(false)
            && self.router_firewall == Probe::Known(OwnedResource::Absent)
            && self.subnet_firewall == Probe::Known(OwnedResource::Absent)
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
    CommitDesiredMode {
        mode: TailscaleMode,
    },
    RestoreDesiredMode {
        mode: Option<TailscaleMode>,
    },
}
