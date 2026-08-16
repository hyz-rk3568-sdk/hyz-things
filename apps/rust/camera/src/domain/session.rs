use serde::Serialize;
use std::net::Ipv4Addr;

pub use hyz_contract::camera::{
    CameraAccessKind, CameraSessionId, SessionIdError, MAX_SESSION_ID_LEN,
};

pub const FIXED_LAN_ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 168, 8, 1);
pub const CAMERA_UDP_PORT_START: u16 = 40_000;
pub const CAMERA_UDP_PORT_END: u16 = 40_015;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraAccessScope {
    Lan { address: Ipv4Addr },
    Tailscale { address: Ipv4Addr },
}

impl CameraAccessScope {
    pub fn validate(kind: CameraAccessKind, address: Ipv4Addr) -> Result<Self, AccessScopeError> {
        if address.is_unspecified()
            || address.is_loopback()
            || address.is_multicast()
            || address == Ipv4Addr::BROADCAST
        {
            return Err(AccessScopeError::UnsafeAddress);
        }

        match kind {
            CameraAccessKind::Lan if address == FIXED_LAN_ADDRESS => Ok(Self::Lan { address }),
            CameraAccessKind::Lan => Err(AccessScopeError::InvalidLanAddress),
            CameraAccessKind::Tailscale if is_tailscale_cgnat(address) => {
                Ok(Self::Tailscale { address })
            }
            CameraAccessKind::Tailscale => Err(AccessScopeError::InvalidTailscaleAddress),
        }
    }

    pub fn address(self) -> Ipv4Addr {
        match self {
            Self::Lan { address } | Self::Tailscale { address } => address,
        }
    }

    pub fn kind(self) -> CameraAccessKind {
        match self {
            Self::Lan { .. } => CameraAccessKind::Lan,
            Self::Tailscale { .. } => CameraAccessKind::Tailscale,
        }
    }
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
