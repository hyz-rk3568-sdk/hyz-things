use serde::Serialize;
use std::net::{Ipv4Addr, Ipv6Addr};

pub use hyz_contract::camera::{
    CameraAccessKind, CameraSessionId, SessionIdError, MAX_SESSION_ID_LEN,
};

pub const FIXED_LAN_ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 168, 8, 1);
pub const CAMERA_UDP_PORT_START: u16 = 40_000;
pub const CAMERA_UDP_PORT_END: u16 = 40_015;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraAccessScope {
    Lan {
        address: Ipv4Addr,
        ipv6_address: Option<Ipv6Addr>,
    },
    Tailscale {
        address: Ipv4Addr,
    },
}

impl CameraAccessScope {
    pub fn validate(kind: CameraAccessKind, address: Ipv4Addr) -> Result<Self, AccessScopeError> {
        Self::validate_with_ipv6(kind, address, None)
    }

    pub fn validate_with_ipv6(
        kind: CameraAccessKind,
        address: Ipv4Addr,
        ipv6_address: Option<Ipv6Addr>,
    ) -> Result<Self, AccessScopeError> {
        if address.is_unspecified()
            || address.is_loopback()
            || address.is_multicast()
            || address == Ipv4Addr::BROADCAST
        {
            return Err(AccessScopeError::UnsafeAddress);
        }
        if ipv6_address.is_some_and(|address| !is_public_global_ipv6(address)) {
            return Err(AccessScopeError::UnsafeIpv6Address);
        }

        match kind {
            CameraAccessKind::Lan if address == FIXED_LAN_ADDRESS => Ok(Self::Lan {
                address,
                ipv6_address,
            }),
            CameraAccessKind::Lan => Err(AccessScopeError::InvalidLanAddress),
            CameraAccessKind::Tailscale if ipv6_address.is_some() => {
                Err(AccessScopeError::UnexpectedIpv6Address)
            }
            CameraAccessKind::Tailscale if is_tailscale_cgnat(address) => {
                Ok(Self::Tailscale { address })
            }
            CameraAccessKind::Tailscale => Err(AccessScopeError::InvalidTailscaleAddress),
        }
    }

    pub fn address(self) -> Ipv4Addr {
        match self {
            Self::Lan { address, .. } | Self::Tailscale { address } => address,
        }
    }

    pub fn ipv6_address(self) -> Option<Ipv6Addr> {
        match self {
            Self::Lan { ipv6_address, .. } => ipv6_address,
            Self::Tailscale { .. } => None,
        }
    }

    pub fn kind(self) -> CameraAccessKind {
        match self {
            Self::Lan { .. } => CameraAccessKind::Lan,
            Self::Tailscale { .. } => CameraAccessKind::Tailscale,
        }
    }
}

fn is_public_global_ipv6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    let in_global_unicast_range = segments[0] & 0xe000 == 0x2000;
    let documentation = (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || (segments[0] == 0x3fff && segments[1] & 0xf000 == 0);
    let benchmarking = segments[0] == 0x2001 && segments[1] == 0x0002 && segments[2] == 0;
    let orchid = segments[0] == 0x2001 && matches!(segments[1] & 0xfff0, 0x0010 | 0x0020);
    in_global_unicast_range && !documentation && !benchmarking && !orchid && segments[0] != 0x2002
}

pub fn is_tailscale_cgnat(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AccessScopeError {
    #[error("unsafe camera address")]
    UnsafeAddress,
    #[error("LAN address does not match the fixed management address")]
    InvalidLanAddress,
    #[error("IPv6 camera candidate is not a public global unicast address")]
    UnsafeIpv6Address,
    #[error("Tailscale camera scope cannot include an IPv6 candidate")]
    UnexpectedIpv6Address,
    #[error("Tailscale address is outside 100.64.0.0/10")]
    InvalidTailscaleAddress,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraSessionState {
    Negotiating,
    Connecting,
    Connected,
    Closing,
    Closed,
    Failed,
}
