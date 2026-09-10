use super::*;

pub(crate) const CAMERA_STATUS_ENDPOINT: &str = "/api/v1/camera/status";

pub(crate) const CAMERA_VIEWER_TOKEN_ENDPOINT: &str = "/api/v1/camera/viewer-token";

pub(crate) const CAMERA_SESSION_CREATE_ENDPOINT: &str = "/api/v1/control/camera/session/create";

pub(crate) const CAMERA_SESSION_CLOSE_ENDPOINT: &str = "/api/v1/control/camera/session/close";

pub(crate) const CAMERA_PROFILE_UPDATE_ENDPOINT: &str = "/api/v1/control/camera/profile";

pub(crate) const CAMERA_ROTATION_UPDATE_ENDPOINT: &str = "/api/v1/control/camera/rotation";

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraViewerTokenDto {
    pub(crate) token: String,
    pub(crate) expires_in_seconds: u64,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraStatusResponseDto {
    pub(crate) camera: CameraStatus,
    pub(crate) available_presets: Vec<CameraStreamPreset>,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraProfileUpdateRequestDto {
    pub(crate) preset: CameraStreamPreset,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraProfileUpdateResponseDto {
    pub(crate) applied: bool,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraRotationUpdateRequestDto {
    pub(crate) rotation: CameraRotation,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraRotationUpdateResponseDto {
    pub(crate) applied: bool,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraSessionCreateRequestDto {
    pub(crate) offer_sdp: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraSessionCreateResponseDto {
    pub(crate) session_id: String,
    pub(crate) answer_sdp: String,
    pub(crate) negotiation_timeout_seconds: u16,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CameraSessionCloseRequestDto {
    pub(crate) session_id: String,
}
