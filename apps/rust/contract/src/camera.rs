//! The `hyz-camera` root-only Unix control protocol (v2).
//!
//! This is the single wire definition shared by the camera daemon's server,
//! the router relay (until the portal takes over signaling) and the hyz-things
//! portal's camera client.

use serde::{Deserialize, Serialize};
use std::{fmt, net::Ipv4Addr, str::FromStr};

pub const CONTROL_PROTOCOL_VERSION: u16 = 2;
pub const CONTROL_SOCKET_PATH: &str = "/run/hyz-camera/control.sock";
pub const MAX_CONTROL_FRAME_BYTES: usize = 64 * 1024;
pub const CONTROL_REQUEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(2);

pub const FIXED_CAPTURE_WIDTH: u16 = 3840;
pub const FIXED_CAPTURE_HEIGHT: u16 = 2160;
pub const MAX_SESSION_ID_LEN: usize = 96;
pub const CAMERA_UDP_PORT_START: u16 = 40_000;
pub const CAMERA_UDP_PORT_END: u16 = 40_015;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraAccessKind {
    Lan,
    Tailscale,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionIdError {
    InvalidGeneration,
    InvalidToken,
    InvalidFormat,
    TooLong,
}

impl fmt::Display for SessionIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidGeneration => "invalid camera daemon generation",
            Self::InvalidToken => "invalid camera session token",
            Self::InvalidFormat => "invalid camera session id format",
            Self::TooLong => "camera session id is too long",
        })
    }
}

impl std::error::Error for SessionIdError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraVideoCodec {
    H264Baseline,
}

/// 画面旋转是媒体管线属性：`videoflip` 在编码前应用，时间戳水印始终叠加在
/// 最终方向画面的左上角，浏览器端不再做 CSS 旋转。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraRotation {
    #[serde(rename = "deg_0")]
    Deg0,
    #[serde(rename = "deg_90")]
    Deg90,
    #[serde(rename = "deg_180")]
    Deg180,
    #[serde(rename = "deg_270")]
    Deg270,
}

impl CameraRotation {
    pub const ALL: [Self; 4] = [Self::Deg0, Self::Deg90, Self::Deg180, Self::Deg270];

    pub const fn degrees(self) -> u16 {
        match self {
            Self::Deg0 => 0,
            Self::Deg90 => 90,
            Self::Deg180 => 180,
            Self::Deg270 => 270,
        }
    }

    /// 「旋转画面」每次点击的循环顺序：0 → 270 → 180 → 90 → 0。
    pub const fn next_rotation(self) -> Self {
        match self {
            Self::Deg0 => Self::Deg270,
            Self::Deg270 => Self::Deg180,
            Self::Deg180 => Self::Deg90,
            Self::Deg90 => Self::Deg0,
        }
    }

    /// 90/270 旋转会交换输出宽高。
    pub const fn swaps_dimensions(self) -> bool {
        matches!(self, Self::Deg90 | Self::Deg270)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraStreamProfile {
    pub width: u16,
    pub height: u16,
    pub fps: u8,
    pub bitrate_bps: u32,
    pub codec: CameraVideoCodec,
    pub rotation: CameraRotation,
}

impl CameraStreamProfile {
    /// 旋转应用后的实际显示高度（90/270 时宽高互换）。
    pub const fn display_height(self) -> u16 {
        if self.rotation.swaps_dimensions() {
            self.width
        } else {
            self.height
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CameraStreamPreset {
    Uhd4k20m,
    Qhd1440p10m,
    Fhd1080p5m,
    #[default]
    Hd720p25m,
}

impl CameraStreamPreset {
    pub const ALL: [Self; 4] = [
        Self::Uhd4k20m,
        Self::Qhd1440p10m,
        Self::Fhd1080p5m,
        Self::Hd720p25m,
    ];

    pub const fn profile(self) -> CameraStreamProfile {
        match self {
            Self::Uhd4k20m => CameraStreamProfile {
                width: FIXED_CAPTURE_WIDTH,
                height: FIXED_CAPTURE_HEIGHT,
                fps: 30,
                bitrate_bps: 20_000_000,
                codec: CameraVideoCodec::H264Baseline,
                rotation: CameraRotation::Deg0,
            },
            Self::Qhd1440p10m => CameraStreamProfile {
                width: 2560,
                height: 1440,
                fps: 30,
                bitrate_bps: 10_000_000,
                codec: CameraVideoCodec::H264Baseline,
                rotation: CameraRotation::Deg0,
            },
            Self::Fhd1080p5m => CameraStreamProfile {
                width: 1920,
                height: 1080,
                fps: 30,
                bitrate_bps: 5_000_000,
                codec: CameraVideoCodec::H264Baseline,
                rotation: CameraRotation::Deg0,
            },
            Self::Hd720p25m => CameraStreamProfile {
                width: 1280,
                height: 720,
                fps: 30,
                bitrate_bps: 2_500_000,
                codec: CameraVideoCodec::H264Baseline,
                rotation: CameraRotation::Deg0,
            },
        }
    }

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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraPipelineState {
    Stopped,
    Starting,
    Streaming,
    Stopping,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraErrorCategory {
    CameraNotFound,
    CameraBusy,
    MediaPipelineFailed,
    EncoderUnavailable,
    #[serde(rename = "webrtc_negotiation_failed")]
    WebRtcNegotiationFailed,
    #[serde(rename = "webrtc_transport_failed")]
    WebRtcTransportFailed,
    ResourceExhausted,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraStatus {
    pub available: bool,
    pub pipeline: CameraPipelineState,
    pub active_sessions: u8,
    pub profile: CameraStreamProfile,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CameraErrorCategory>,
    /// 全双工语音对讲能力（additive：旧 camera 不发送该字段，旧客户端忽略）。
    /// `supported=false` 或缺失时前端不显示对讲控件，直播保持 video-only。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<CameraAudioStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraAudioStatus {
    pub supported: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub version: u16,
    pub operation: ControlOperation,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlOperation {
    Status,
    CreateSession(CreateSessionRequest),
    CloseSession(CloseSessionRequest),
    SetProfile(SetProfileRequest),
    SetRotation(SetRotationRequest),
    Shutdown,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionRequest {
    pub scope: CameraAccessKind,
    pub address: Ipv4Addr,
    pub offer_sdp: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CloseSessionRequest {
    pub session_id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SetProfileRequest {
    pub preset: CameraStreamPreset,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SetRotationRequest {
    pub rotation: CameraRotation,
}

#[derive(Serialize, Deserialize)]
pub struct ControlResponse {
    pub version: u16,
    #[serde(flatten)]
    pub outcome: ControlOutcome,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlOutcome {
    Ok(ControlResult),
    Error(ControlErrorResponse),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlResult {
    Status(CameraStatus),
    SessionCreated(SessionCreatedResponse),
    SessionClosed,
    ProfileSet,
    RotationSet,
    ShutdownAccepted,
}

#[derive(Serialize, Deserialize)]
pub struct SessionCreatedResponse {
    pub session_id: CameraSessionId,
    pub answer_sdp: String,
    pub negotiation_timeout_seconds: u16,
}

#[derive(Serialize, Deserialize)]
pub struct ControlErrorResponse {
    pub code: ControlErrorCode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlErrorCode {
    InvalidVersion,
    InvalidRequest,
    RequestTooLarge,
    PermissionDenied,
    InvalidAccessScope,
    ShuttingDown,
    SessionBusy,
    UnknownSession,
    CameraNotFound,
    CameraBusy,
    EncoderUnavailable,
    MediaPipelineFailed,
    UnsupportedSdp,
    ResourceExhausted,
    WebRtcNegotiationFailed,
    WebRtcTransportFailed,
    Internal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_shape_is_versioned_and_typed() {
        let body = serde_json::to_value(ControlRequest {
            version: CONTROL_PROTOCOL_VERSION,
            operation: ControlOperation::CreateSession(CreateSessionRequest {
                scope: CameraAccessKind::Lan,
                address: Ipv4Addr::new(192, 168, 8, 1),
                offer_sdp: "v=0\r\n".to_owned(),
            }),
        })
        .unwrap();
        assert_eq!(body["version"], 2);
        assert_eq!(body["operation"]["create_session"]["scope"], "lan");
        assert!(body["operation"]["create_session"].get("port").is_none());
    }

    #[test]
    fn status_response_round_trips_with_private_codec() {
        let response = ControlResponse {
            version: CONTROL_PROTOCOL_VERSION,
            outcome: ControlOutcome::Ok(ControlResult::Status(CameraStatus {
                available: true,
                pipeline: CameraPipelineState::Stopped,
                active_sessions: 0,
                profile: CameraStreamProfile {
                    width: 3840,
                    height: 2160,
                    fps: 30,
                    bitrate_bps: 20_000_000,
                    codec: CameraVideoCodec::H264Baseline,
                    rotation: CameraRotation::Deg90,
                },
                error: None,
                audio: Some(CameraAudioStatus { supported: true }),
            })),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("h264_baseline"));
        assert!(json.contains("deg_90"));
        assert!(json.contains("\"audio\":{\"supported\":true}"));
        let decoded: ControlResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.version, CONTROL_PROTOCOL_VERSION);
        let ControlOutcome::Ok(ControlResult::Status(status)) = decoded.outcome else {
            panic!("expected camera status response");
        };
        assert_eq!(status.profile.codec, CameraVideoCodec::H264Baseline);
        assert_eq!(status.profile.rotation, CameraRotation::Deg90);
        assert_eq!(status.audio, Some(CameraAudioStatus { supported: true }));
    }

    #[test]
    fn status_without_audio_field_decodes_with_default() {
        // 旧 camera（协议 v2 无 audio 字段）：序列化时省略，解码端回落到 None。
        let response = ControlResponse {
            version: CONTROL_PROTOCOL_VERSION,
            outcome: ControlOutcome::Ok(ControlResult::Status(CameraStatus {
                available: true,
                pipeline: CameraPipelineState::Stopped,
                active_sessions: 0,
                profile: CameraStreamProfile {
                    width: 1280,
                    height: 720,
                    fps: 30,
                    bitrate_bps: 2_500_000,
                    codec: CameraVideoCodec::H264Baseline,
                    rotation: CameraRotation::Deg0,
                },
                error: None,
                audio: None,
            })),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(!json.contains("audio"));
        let decoded: ControlResponse = serde_json::from_str(&json).unwrap();
        let ControlOutcome::Ok(ControlResult::Status(status)) = decoded.outcome else {
            panic!("expected camera status response");
        };
        assert_eq!(status.audio, None);
    }

    #[test]
    fn session_id_round_trips_and_rejects_traversal() {
        let session = CameraSessionId::from_parts("gen-01", &"a".repeat(48)).unwrap();
        assert_eq!(session.as_str(), format!("gen-01.{}", "a".repeat(48)));
        assert!(session.belongs_to_generation("gen-01"));
        assert!(!session.belongs_to_generation("gen-02"));
        let parsed: CameraSessionId = session.as_str().parse().unwrap();
        assert_eq!(parsed, session);
        assert!(CameraSessionId::from_parts("a/b", &"a".repeat(48)).is_err());
        assert!(CameraSessionId::from_parts("gen", &"a".repeat(47)).is_err());
    }
}
