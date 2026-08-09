//! Fixed Wi-Fi router topology and its desired/observed state.

pub const LAN_BRIDGE: &str = "br-lan";
pub const LAN_MEMBER: &str = "p2p0";
pub const WAN_INTERFACE: &str = "wlan0";
pub const LAN_ADDRESS: &str = "192.168.8.1/24";
pub const LAN_SUBNET: &str = "192.168.8.0/24";
pub const ROUTER_FILTER_CHAIN: &str = "HYZ_ROUTER_FWD";
pub const ROUTER_NAT_CHAIN: &str = "HYZ_ROUTER_NAT";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe<T> {
    Known(T),
    Unknown(String),
}

impl<T> Probe<T> {
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown(_) => None,
        }
    }

    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> Probe<U> {
        match self {
            Self::Known(value) => Probe::Known(map(value)),
            Self::Unknown(reason) => Probe::Unknown(reason),
        }
    }

    pub fn and_then<U>(self, map: impl FnOnce(T) -> Probe<U>) -> Probe<U> {
        match self {
            Self::Known(value) => map(value),
            Self::Unknown(reason) => Probe::Unknown(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedResource {
    Absent,
    Owned { token: String },
    Foreign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardingDesired {
    Disabled,
    Enabled,
}

/// Management LAN is intentionally not switchable. The only controllable router
/// state is downstream forwarding/NAT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkDesired {
    pub forwarding: ForwardingDesired,
}

impl NetworkDesired {
    pub const fn management_only() -> Self {
        Self {
            forwarding: ForwardingDesired::Disabled,
        }
    }

    pub const fn forwarding() -> Self {
        Self {
            forwarding: ForwardingDesired::Enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkObserved {
    pub bridge: Probe<OwnedResource>,
    pub bridge_up: Probe<bool>,
    pub lan_address_present: Probe<bool>,
    pub ap_attached: Probe<bool>,
    pub management_services_healthy: Probe<bool>,
    pub wan_default_route_present: Probe<bool>,
    pub ipv4_forwarding: Probe<bool>,
    /// Saved pre-daemon value while this runtime owns mutations of the global forwarding switch.
    pub previous_ipv4_forwarding: Probe<Option<bool>>,
    pub router_firewall: Probe<OwnedResource>,
}

impl NetworkObserved {
    pub fn unknown(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            bridge: Probe::Unknown(reason.clone()),
            bridge_up: Probe::Unknown(reason.clone()),
            lan_address_present: Probe::Unknown(reason.clone()),
            ap_attached: Probe::Unknown(reason.clone()),
            management_services_healthy: Probe::Unknown(reason.clone()),
            wan_default_route_present: Probe::Unknown(reason.clone()),
            ipv4_forwarding: Probe::Unknown(reason.clone()),
            previous_ipv4_forwarding: Probe::Unknown(reason.clone()),
            router_firewall: Probe::Unknown(reason),
        }
    }

    pub fn management_ready(&self) -> bool {
        matches!(self.bridge, Probe::Known(OwnedResource::Owned { .. }))
            && self.bridge_up == Probe::Known(true)
            && self.lan_address_present == Probe::Known(true)
            && self.ap_attached == Probe::Known(true)
            && self.management_services_healthy == Probe::Known(true)
    }

    pub fn ready_for(&self, desired: &NetworkDesired) -> bool {
        if !self.management_ready() {
            return false;
        }
        match desired.forwarding {
            ForwardingDesired::Enabled => {
                self.wan_default_route_present == Probe::Known(true)
                    && self.ipv4_forwarding == Probe::Known(true)
                    && matches!(self.previous_ipv4_forwarding, Probe::Known(Some(_)))
                    && matches!(
                        self.router_firewall,
                        Probe::Known(OwnedResource::Owned { .. })
                    )
            }
            ForwardingDesired::Disabled => {
                self.ipv4_forwarding == Probe::Known(false)
                    && self.router_firewall == Probe::Known(OwnedResource::Absent)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkAction {
    EnsureOwnedBridge { token: String },
    RemoveOwnedBridge { token: String },
    ConfigureBridge,
    SetBridgeDown,
    AssignLanAddress,
    RemoveLanAddress,
    AttachAp,
    DetachAp,
    EnsureManagementServices,
    StopManagementServices,
    WaitForWanRoute,
    CaptureIpv4Forwarding,
    EnableIpv4Forwarding,
    DisableIpv4Forwarding,
    InstallRouterFirewall { token: String },
    RemoveRouterFirewall { token: String },
    RestoreIpv4Forwarding,
}
