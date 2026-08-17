use std::net::Ipv4Addr;

// Fixed dual-LAN topology and typed uplink selection rules.
pub const LAN_BRIDGE: &str = "br-lan";
pub const ETHERNET_LAN_INTERFACE: &str = "eth1";
pub const AP_INTERFACE: &str = "p2p0";
/// Backwards-compatible name for the AP member during the topology migration.
pub const LAN_MEMBER: &str = AP_INTERFACE;
pub const ETHERNET_WAN_INTERFACE: &str = "eth0";
pub const WIFI_WAN_INTERFACE: &str = "wlan0";
/// Backwards-compatible name for the current Wi-Fi WAN during the topology migration.
pub const WAN_INTERFACE: &str = WIFI_WAN_INTERFACE;
pub const LAN_ADDRESS: &str = "192.168.8.1/24";
pub const LAN_SUBNET: &str = "192.168.8.0/24";
pub const ROUTER_FILTER_CHAIN: &str = "HYZ_ROUTER_FWD";
pub const ROUTER_INPUT_CHAIN: &str = "HYZ_ROUTER_IN";
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
pub enum UplinkId {
    Ethernet,
    Wifi,
}

impl UplinkId {
    pub const fn interface(self) -> &'static str {
        match self {
            Self::Ethernet => ETHERNET_WAN_INTERFACE,
            Self::Wifi => WIFI_WAN_INTERFACE,
        }
    }

    pub const fn metric(self) -> u32 {
        match self {
            Self::Ethernet => 100,
            Self::Wifi => 600,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveUplinkObserved {
    pub uplink: UplinkId,
    pub gateway: Ipv4Addr,
}

impl ActiveUplinkObserved {
    pub const fn new(uplink: UplinkId, gateway: Ipv4Addr) -> Self {
        Self { uplink, gateway }
    }
}

/// The exact router-owned WAN interfaces authorized for forwarding, NAT, and WAN input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouterWanSet {
    Ethernet,
    Wifi,
    EthernetAndWifi,
}

impl RouterWanSet {
    pub const ALL: [Self; 3] = [Self::Ethernet, Self::Wifi, Self::EthernetAndWifi];

    pub const fn interfaces(self) -> &'static [&'static str] {
        match self {
            Self::Ethernet => &[ETHERNET_WAN_INTERFACE],
            Self::Wifi => &[WIFI_WAN_INTERFACE],
            Self::EthernetAndWifi => &[ETHERNET_WAN_INTERFACE, WIFI_WAN_INTERFACE],
        }
    }

    pub fn from_fully_ready(
        ethernet: &UplinkObserved,
        wifi: &UplinkObserved,
    ) -> Probe<Option<Self>> {
        match (ethernet.ready(), wifi.ready()) {
            (true, true) => Probe::Known(Some(Self::EthernetAndWifi)),
            (true, false) => Probe::Known(Some(Self::Ethernet)),
            (false, true) => Probe::Known(Some(Self::Wifi)),
            (false, false) => {
                match uplink_unknown_reason(ethernet).or_else(|| uplink_unknown_reason(wifi)) {
                    Some(reason) => Probe::Unknown(reason.to_owned()),
                    None => Probe::Known(None),
                }
            }
        }
    }
}

fn uplink_unknown_reason(uplink: &UplinkObserved) -> Option<&str> {
    match &uplink.link {
        Probe::Unknown(reason) => return Some(reason),
        Probe::Known(_) => {}
    }
    match &uplink.session {
        Probe::Unknown(reason) => return Some(reason),
        Probe::Known(_) => {}
    }
    match &uplink.address {
        Probe::Unknown(reason) => return Some(reason),
        Probe::Known(_) => {}
    }
    match &uplink.default_route {
        Probe::Unknown(reason) => return Some(reason),
        Probe::Known(_) => {}
    }
    match &uplink.gateway {
        Probe::Unknown(reason) => return Some(reason),
        Probe::Known(_) => {}
    }
    match &uplink.resolver {
        Probe::Unknown(reason) => Some(reason),
        Probe::Known(_) => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UplinkObserved {
    /// True only when the fixed interface carrier/link and the exact managed DHCP client
    /// identity and active generation have both been verified.
    pub link: Probe<bool>,
    pub session: Probe<bool>,
    pub address: Probe<OwnedResource>,
    /// Exact DHCP-owned default route with the fixed uplink metric.
    pub default_route: Probe<bool>,
    /// Gateway parsed from that exact owned default route, never caller-provided interface text.
    pub gateway: Probe<Option<Ipv4Addr>>,
    /// Validated per-uplink resolver record with the matching DHCP generation.
    pub resolver: Probe<bool>,
}

impl UplinkObserved {
    pub fn unknown(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            link: Probe::Unknown(reason.clone()),
            session: Probe::Unknown(reason.clone()),
            address: Probe::Unknown(reason.clone()),
            default_route: Probe::Unknown(reason.clone()),
            gateway: Probe::Unknown(reason.clone()),
            resolver: Probe::Unknown(reason),
        }
    }

    pub fn unavailable() -> Self {
        Self {
            link: Probe::Known(false),
            session: Probe::Known(false),
            address: Probe::Known(OwnedResource::Absent),
            default_route: Probe::Known(false),
            gateway: Probe::Known(None),
            resolver: Probe::Known(false),
        }
    }

    /// Temporary Wi-Fi-only adapter mapping. Phase C replaces this with independently probed
    /// per-uplink address, route and resolver ownership.
    pub fn wifi_only_route(route: Probe<bool>) -> Self {
        match route {
            Probe::Known(true) => Self {
                link: Probe::Known(true),
                session: Probe::Known(true),
                address: Probe::Known(OwnedResource::Owned {
                    token: "wifi-dhcp".to_owned(),
                }),
                default_route: Probe::Known(true),
                gateway: Probe::Known(Some("192.0.2.1".parse().expect("fixed test gateway"))),
                resolver: Probe::Known(true),
            },
            Probe::Known(false) => Self::unavailable(),
            Probe::Unknown(reason) => Self {
                link: Probe::Unknown(reason.clone()),
                session: Probe::Unknown(reason.clone()),
                address: Probe::Unknown(reason.clone()),
                default_route: Probe::Unknown(reason.clone()),
                gateway: Probe::Unknown(reason.clone()),
                resolver: Probe::Unknown(reason),
            },
        }
    }

    pub fn ready(&self) -> bool {
        self.link == Probe::Known(true)
            && self.session == Probe::Known(true)
            && matches!(self.address, Probe::Known(OwnedResource::Owned { .. }))
            && self.default_route == Probe::Known(true)
            && matches!(self.gateway, Probe::Known(Some(_)))
            && self.resolver == Probe::Known(true)
    }
}

/// Selects a route-capable uplink without probing the public internet. Ethernet has priority.
pub fn active_uplink(ethernet: &UplinkObserved, wifi: &UplinkObserved) -> Option<UplinkId> {
    if ethernet.ready() {
        Some(UplinkId::Ethernet)
    } else if wifi.ready() {
        Some(UplinkId::Wifi)
    } else {
        None
    }
}

pub fn active_uplink_observation(
    ethernet: &UplinkObserved,
    wifi: &UplinkObserved,
) -> Option<ActiveUplinkObserved> {
    match active_uplink(ethernet, wifi) {
        Some(UplinkId::Ethernet) if ethernet.resolver == Probe::Known(true) => ethernet
            .gateway
            .known()
            .and_then(|gateway| *gateway)
            .map(|gateway| ActiveUplinkObserved::new(UplinkId::Ethernet, gateway)),
        Some(UplinkId::Wifi) if wifi.resolver == Probe::Known(true) => wifi
            .gateway
            .known()
            .and_then(|gateway| *gateway)
            .map(|gateway| ActiveUplinkObserved::new(UplinkId::Wifi, gateway)),
        _ => None,
    }
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
    pub ethernet_lan_attached: Probe<bool>,
    pub management_services_healthy: Probe<bool>,
    pub ethernet_uplink: UplinkObserved,
    pub wifi_uplink: UplinkObserved,
    pub ipv4_forwarding: Probe<bool>,
    /// Saved pre-daemon value while this runtime owns mutations of the global forwarding switch.
    pub previous_ipv4_forwarding: Probe<Option<bool>>,
    pub router_firewall: Probe<OwnedResource>,
    /// Exact WAN interfaces currently encoded in the owned router firewall, if any.
    pub firewall_wan_set: Probe<Option<RouterWanSet>>,
}

impl NetworkObserved {
    pub fn unknown(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            bridge: Probe::Unknown(reason.clone()),
            bridge_up: Probe::Unknown(reason.clone()),
            lan_address_present: Probe::Unknown(reason.clone()),
            ap_attached: Probe::Unknown(reason.clone()),
            ethernet_lan_attached: Probe::Unknown(reason.clone()),
            management_services_healthy: Probe::Unknown(reason.clone()),
            ethernet_uplink: UplinkObserved::unknown(reason.clone()),
            wifi_uplink: UplinkObserved::unknown(reason.clone()),
            ipv4_forwarding: Probe::Unknown(reason.clone()),
            previous_ipv4_forwarding: Probe::Unknown(reason.clone()),
            router_firewall: Probe::Unknown(reason.clone()),
            firewall_wan_set: Probe::Unknown(reason),
        }
    }

    pub fn active_uplink(&self) -> Option<UplinkId> {
        active_uplink(&self.ethernet_uplink, &self.wifi_uplink)
    }

    pub fn active_uplink_probe(&self) -> Probe<Option<UplinkId>> {
        if let Some(active) = active_uplink_observation(&self.ethernet_uplink, &self.wifi_uplink) {
            return Probe::Known(Some(active.uplink));
        }
        for uplink in [&self.ethernet_uplink, &self.wifi_uplink] {
            if let Some(reason) = uplink_unknown_reason(uplink) {
                return Probe::Unknown(reason.to_owned());
            }
        }
        Probe::Known(None)
    }

    pub fn active_uplink_observation(&self) -> Probe<Option<ActiveUplinkObserved>> {
        match active_uplink_observation(&self.ethernet_uplink, &self.wifi_uplink) {
            Some(active) => Probe::Known(Some(active)),
            None => match self.active_uplink_probe() {
                Probe::Known(None) => Probe::Known(None),
                Probe::Known(Some(_)) => {
                    Probe::Unknown("selected uplink resolver exposure is absent".to_owned())
                }
                Probe::Unknown(reason) => Probe::Unknown(reason),
            },
        }
    }

    pub fn router_wan_set(&self) -> Probe<Option<RouterWanSet>> {
        RouterWanSet::from_fully_ready(&self.ethernet_uplink, &self.wifi_uplink)
    }

    pub fn management_ready(&self) -> bool {
        matches!(self.bridge, Probe::Known(OwnedResource::Owned { .. }))
            && self.bridge_up == Probe::Known(true)
            && self.lan_address_present == Probe::Known(true)
            && self.ap_attached == Probe::Known(true)
            && self.ethernet_lan_attached == Probe::Known(true)
            && self.management_services_healthy == Probe::Known(true)
    }

    pub fn ready_for(&self, desired: &NetworkDesired) -> bool {
        if !self.management_ready() {
            return false;
        }
        match desired.forwarding {
            ForwardingDesired::Enabled => {
                matches!(
                    self.router_wan_set(),
                    Probe::Known(Some(wan_set)) if self.firewall_wan_set == Probe::Known(Some(wan_set))
                ) && matches!(self.active_uplink_observation(), Probe::Known(Some(_)))
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
                    && self.firewall_wan_set == Probe::Known(None)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkAction {
    EnsureOwnedBridge {
        token: String,
    },
    RemoveOwnedBridge {
        token: String,
    },
    ConfigureBridge,
    SetBridgeDown,
    AssignLanAddress,
    RemoveLanAddress,
    AttachAp,
    DetachAp,
    AttachEthernetLan,
    DetachEthernetLan,
    EnsureManagementServices,
    StopManagementServices,
    WaitForWanRoute,
    CaptureIpv4Forwarding,
    EnableIpv4Forwarding,
    DisableIpv4Forwarding,
    InstallRouterFirewall {
        token: String,
        wan_set: RouterWanSet,
    },
    ReconfigureRouterFirewall {
        token: String,
        previous_wan_set: RouterWanSet,
        wan_set: RouterWanSet,
    },
    RemoveRouterFirewall {
        token: String,
    },
    RestoreIpv4Forwarding,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_uplink() -> UplinkObserved {
        UplinkObserved {
            link: Probe::Known(true),
            session: Probe::Known(true),
            address: Probe::Known(OwnedResource::Owned {
                token: "dhcp".to_owned(),
            }),
            default_route: Probe::Known(true),
            gateway: Probe::Known(Some("192.0.2.1".parse().unwrap())),
            resolver: Probe::Known(true),
        }
    }

    #[test]
    fn ethernet_wins_only_after_all_runtime_owned_l3_state_is_confirmed() {
        let wifi = ready_uplink();
        let mut ethernet = ready_uplink();
        ethernet.resolver = Probe::Known(false);

        assert_eq!(active_uplink(&ethernet, &wifi), Some(UplinkId::Wifi));

        ethernet.resolver = Probe::Known(true);
        assert_eq!(active_uplink(&ethernet, &wifi), Some(UplinkId::Ethernet));
    }

    #[test]
    fn unknown_or_foreign_ethernet_cannot_displace_wifi() {
        let wifi = ready_uplink();
        let mut ethernet = ready_uplink();
        ethernet.address = Probe::Known(OwnedResource::Foreign);
        assert_eq!(active_uplink(&ethernet, &wifi), Some(UplinkId::Wifi));

        ethernet.address = Probe::Unknown("address ownership unavailable".to_owned());
        assert_eq!(active_uplink(&ethernet, &wifi), Some(UplinkId::Wifi));
    }

    #[test]
    fn no_complete_uplink_has_no_active_path() {
        let mut ethernet = ready_uplink();
        let mut wifi = ready_uplink();
        ethernet.default_route = Probe::Known(false);
        wifi.default_route = Probe::Known(false);

        assert_eq!(active_uplink(&ethernet, &wifi), None);
    }
}
