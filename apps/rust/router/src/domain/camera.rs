use serde::{Deserialize, Serialize};
use std::{net::Ipv4Addr, str::FromStr};

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
    pub bitrate_bps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraStreamPreset {
    Uhd4k20m,
    Qhd1440p10m,
    Fhd1080p5m,
    Hd720p25m,
}

impl CameraStreamPreset {
    pub const ALL: [Self; 4] = [
        Self::Uhd4k20m,
        Self::Qhd1440p10m,
        Self::Fhd1080p5m,
        Self::Hd720p25m,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Uhd4k20m => "uhd4k20m",
            Self::Qhd1440p10m => "qhd1440p10m",
            Self::Fhd1080p5m => "fhd1080p5m",
            Self::Hd720p25m => "hd720p25m",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Uhd4k20m => "4K · 20 Mbps",
            Self::Qhd1440p10m => "1440p · 10 Mbps",
            Self::Fhd1080p5m => "1080p · 5 Mbps",
            Self::Hd720p25m => "720p · 2.5 Mbps",
        }
    }

    pub const fn width(self) -> u16 {
        match self {
            Self::Uhd4k20m => 3840,
            Self::Qhd1440p10m => 2560,
            Self::Fhd1080p5m => 1920,
            Self::Hd720p25m => 1280,
        }
    }

    pub const fn height(self) -> u16 {
        match self {
            Self::Uhd4k20m => 2160,
            Self::Qhd1440p10m => 1440,
            Self::Fhd1080p5m => 1080,
            Self::Hd720p25m => 720,
        }
    }

    pub const fn fps(self) -> u8 {
        30
    }

    pub const fn bitrate_bps(self) -> u32 {
        match self {
            Self::Uhd4k20m => 20_000_000,
            Self::Qhd1440p10m => 10_000_000,
            Self::Fhd1080p5m => 5_000_000,
            Self::Hd720p25m => 2_500_000,
        }
    }
}

impl Default for CameraStreamPreset {
    fn default() -> Self {
        Self::Hd720p25m
    }
}

impl FromStr for CameraStreamPreset {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        CameraStreamPreset::ALL
            .into_iter()
            .find(|preset| preset.id() == value)
            .ok_or(())
    }
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
