use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraPipelineState {
    Stopped,
    Starting,
    Streaming,
    Stopping,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CameraStatus {
    pub available: bool,
    pub pipeline: CameraPipelineState,
    pub active_sessions: u8,
    pub profile: CameraStreamProfile,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CameraErrorCategory>,
}

use super::CameraStreamProfile;
