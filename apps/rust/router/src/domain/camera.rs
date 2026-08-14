use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

pub const CAMERA_LAN_ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 168, 8, 1);
pub const CAMERA_UDP_PORT_START: u16 = 40_000;
pub const CAMERA_UDP_PORT_END: u16 = 40_015;
pub const CAMERA_MAX_SDP_BYTES: usize = 32 * 1024;
pub const CAMERA_MAX_SESSION_ID_BYTES: usize = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraAccessKind {
    Lan,
    Tailscale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraAccessScope {
    Lan,
    Tailscale { address: Ipv4Addr },
}

impl CameraAccessScope {
    pub const fn kind(self) -> CameraAccessKind {
        match self {
            Self::Lan => CameraAccessKind::Lan,
            Self::Tailscale { .. } => CameraAccessKind::Tailscale,
        }
    }

    pub const fn address(self) -> Ipv4Addr {
        match self {
            Self::Lan => CAMERA_LAN_ADDRESS,
            Self::Tailscale { address } => address,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraPipelineState {
    Stopped,
    Starting,
    Streaming,
    Stopping,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraErrorCategory {
    CameraNotFound,
    CameraBusy,
    MediaPipelineFailed,
    EncoderUnavailable,
    ControlUnavailable,
    #[serde(alias = "web_rtc_negotiation_failed")]
    WebrtcNegotiationFailed,
    #[serde(alias = "web_rtc_transport_failed")]
    WebrtcTransportFailed,
    ResourceExhausted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraStreamProfile {
    pub codec: String,
    pub width: u16,
    pub height: u16,
    pub fps: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraStatus {
    pub available: bool,
    pub pipeline: CameraPipelineState,
    pub active_sessions: u8,
    pub profile: CameraStreamProfile,
    pub access: CameraAccessKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_category: Option<CameraErrorCategory>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_scope_derives_only_fixed_server_addresses() {
        assert_eq!(CameraAccessScope::Lan.address(), CAMERA_LAN_ADDRESS);
        let tailscale = Ipv4Addr::new(100, 64, 1, 2);
        assert_eq!(
            CameraAccessScope::Tailscale { address: tailscale }.address(),
            tailscale
        );
    }
}
