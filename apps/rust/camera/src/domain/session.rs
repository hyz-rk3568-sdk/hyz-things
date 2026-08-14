use serde::{Deserialize, Serialize};
use std::{fmt, net::Ipv4Addr, str::FromStr};

pub const FIXED_LAN_ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 168, 8, 1);
pub const CAMERA_UDP_PORT_START: u16 = 40_000;
pub const CAMERA_UDP_PORT_END: u16 = 40_015;
pub const MAX_SESSION_ID_LEN: usize = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraAccessKind {
    Lan,
    Tailscale,
}

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

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CameraSessionId(String);

impl CameraSessionId {
    pub fn new(generation: &str, random: [u8; 24]) -> Result<Self, SessionIdError> {
        validate_generation(generation)?;
        let mut token = String::with_capacity(48);
        for byte in random {
            use fmt::Write as _;
            let _ = write!(token, "{byte:02x}");
        }
        Self::from_parts(generation, &token)
    }

    pub fn from_parts(generation: &str, token: &str) -> Result<Self, SessionIdError> {
        validate_generation(generation)?;
        if token.len() != 48 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(SessionIdError::InvalidToken);
        }
        let value = format!("{generation}.{token}");
        if value.len() > MAX_SESSION_ID_LEN {
            return Err(SessionIdError::TooLong);
        }
        Ok(Self(value))
    }

    pub fn belongs_to_generation(&self, generation: &str) -> bool {
        self.0
            .split_once('.')
            .is_some_and(|(candidate, _)| candidate == generation)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CameraSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CameraSessionId([REDACTED])")
    }
}

impl fmt::Display for CameraSessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for CameraSessionId {
    type Err = SessionIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (generation, token) = value.split_once('.').ok_or(SessionIdError::InvalidFormat)?;
        Self::from_parts(generation, token)
    }
}

fn validate_generation(generation: &str) -> Result<(), SessionIdError> {
    if generation.is_empty()
        || generation.len() > 32
        || !generation
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(SessionIdError::InvalidGeneration);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SessionIdError {
    #[error("invalid camera daemon generation")]
    InvalidGeneration,
    #[error("invalid camera session token")]
    InvalidToken,
    #[error("invalid camera session id format")]
    InvalidFormat,
    #[error("camera session id is too long")]
    TooLong,
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
