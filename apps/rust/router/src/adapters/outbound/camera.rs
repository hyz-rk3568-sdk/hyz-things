use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    net::Ipv4Addr,
    os::{
        fd::AsRawFd,
        unix::{fs::FileTypeExt, fs::MetadataExt, net::UnixStream},
    },
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    application::camera::{CameraControlPort, CameraError, CameraSession},
    domain::camera::{
        CameraAccessKind, CameraAccessScope, CameraErrorCategory, CameraPipelineState,
        CameraStatus, CameraStreamPreset, CameraStreamProfile,
    },
};

pub const CAMERA_CONTROL_SOCKET: &str = "/run/hyz-camera/control.sock";
const CAMERA_CONTROL_PROTOCOL_VERSION: u16 = 1;
const CAMERA_CONTROL_MAX_FRAME_BYTES: usize = 64 * 1024;
const CAMERA_CONTROL_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct CameraUnixAdapter {
    socket: PathBuf,
}

impl Default for CameraUnixAdapter {
    fn default() -> Self {
        Self {
            socket: PathBuf::from(CAMERA_CONTROL_SOCKET),
        }
    }
}

impl CameraUnixAdapter {
    #[cfg(test)]
    fn at(socket: PathBuf) -> Self {
        Self { socket }
    }

    fn request(&self, operation: RequestOperation) -> Result<ResponseResult, CameraError> {
        validate_socket(&self.socket)?;
        let mut stream = UnixStream::connect(&self.socket).map_err(|_| CameraError::Unavailable)?;
        verify_root_peer(&stream)?;
        stream
            .set_read_timeout(Some(CAMERA_CONTROL_TIMEOUT))
            .map_err(|_| CameraError::Unavailable)?;
        stream
            .set_write_timeout(Some(CAMERA_CONTROL_TIMEOUT))
            .map_err(|_| CameraError::Unavailable)?;

        let request = ControlRequest {
            version: CAMERA_CONTROL_PROTOCOL_VERSION,
            operation,
        };
        let body = serde_json::to_vec(&request).map_err(|_| CameraError::InvalidRequest)?;
        if body.len() > CAMERA_CONTROL_MAX_FRAME_BYTES {
            return Err(CameraError::InvalidRequest);
        }
        stream
            .write_all(&(body.len() as u32).to_be_bytes())
            .and_then(|_| stream.write_all(&body))
            .map_err(|_| CameraError::Unavailable)?;

        let mut length = [0u8; 4];
        stream
            .read_exact(&mut length)
            .map_err(|_| CameraError::Unavailable)?;
        let length = u32::from_be_bytes(length) as usize;
        if length > CAMERA_CONTROL_MAX_FRAME_BYTES {
            return Err(CameraError::Unavailable);
        }
        let mut response = vec![0u8; length];
        stream
            .read_exact(&mut response)
            .map_err(|_| CameraError::Unavailable)?;
        let response: ControlResponse =
            serde_json::from_slice(&response).map_err(|_| CameraError::Unavailable)?;
        if response.version != CAMERA_CONTROL_PROTOCOL_VERSION {
            return Err(CameraError::Unavailable);
        }
        match response.outcome {
            ResponseOutcome::Ok(result) => Ok(result),
            ResponseOutcome::Error(error) => Err(map_control_error(error.code)),
        }
    }
}

#[async_trait]
impl CameraControlPort for CameraUnixAdapter {
    async fn status(&self, scope: CameraAccessScope) -> Result<CameraStatus, CameraError> {
        let adapter = self.clone();
        let result = tokio::task::spawn_blocking(move || adapter.request(RequestOperation::Status))
            .await
            .map_err(|_| CameraError::Unavailable)??;
        let ResponseResult::Status(status) = result else {
            return Err(CameraError::Unavailable);
        };
        if status.profile.codec != "h264_baseline" {
            return Err(CameraError::Unavailable);
        }
        Ok(CameraStatus {
            available: status.available,
            pipeline: status.pipeline,
            active_sessions: status.active_sessions,
            profile: CameraStreamProfile {
                codec: "h264".to_owned(),
                width: status.profile.width,
                height: status.profile.height,
                fps: status.profile.fps,
                bitrate_bps: status.profile.bitrate_bps,
            },
            access: scope.kind(),
            error_category: status.error,
        })
    }

    async fn create_session(
        &self,
        scope: CameraAccessScope,
        offer_sdp: String,
    ) -> Result<CameraSession, CameraError> {
        let adapter = self.clone();
        let request = CreateSessionRequest {
            scope: scope.kind(),
            address: scope.address(),
            offer_sdp,
        };
        let result = tokio::task::spawn_blocking(move || {
            adapter.request(RequestOperation::CreateSession(request))
        })
        .await
        .map_err(|_| CameraError::Unavailable)??;
        let ResponseResult::SessionCreated(session) = result else {
            return Err(CameraError::Unavailable);
        };
        Ok(CameraSession {
            session_id: session.session_id,
            answer_sdp: session.answer_sdp,
            negotiation_timeout_seconds: session.negotiation_timeout_seconds,
        })
    }

    async fn close_session(&self, session_id: &str) -> Result<(), CameraError> {
        let adapter = self.clone();
        let request = CloseSessionRequest {
            session_id: session_id.to_owned(),
        };
        let result = tokio::task::spawn_blocking(move || {
            adapter.request(RequestOperation::CloseSession(request))
        })
        .await
        .map_err(|_| CameraError::Unavailable)??;
        match result {
            ResponseResult::SessionClosed => Ok(()),
            _ => Err(CameraError::Unavailable),
        }
    }

    async fn set_profile(&self, preset: CameraStreamPreset) -> Result<(), CameraError> {
        let adapter = self.clone();
        let request = SetProfileRequest { preset };
        let result = tokio::task::spawn_blocking(move || {
            adapter.request(RequestOperation::SetProfile(request))
        })
        .await
        .map_err(|_| CameraError::Unavailable)??;
        match result {
            ResponseResult::ProfileSet => Ok(()),
            _ => Err(CameraError::Unavailable),
        }
    }
}

fn validate_socket(path: &Path) -> Result<(), CameraError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| CameraError::Unavailable)?;
    if metadata.file_type().is_symlink() || !metadata.file_type().is_socket() || metadata.uid() != 0
    {
        return Err(CameraError::Unavailable);
    }
    Ok(())
}

fn verify_root_peer(stream: &UnixStream) -> Result<(), CameraError> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result == 0 && length as usize == std::mem::size_of::<libc::ucred>() && credentials.uid == 0
    {
        Ok(())
    } else {
        Err(CameraError::Unavailable)
    }
}

#[derive(Serialize)]
struct ControlRequest {
    version: u16,
    operation: RequestOperation,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum RequestOperation {
    Status,
    CreateSession(CreateSessionRequest),
    CloseSession(CloseSessionRequest),
    SetProfile(SetProfileRequest),
}

#[derive(Serialize)]
struct CreateSessionRequest {
    scope: CameraAccessKind,
    address: Ipv4Addr,
    offer_sdp: String,
}

#[derive(Serialize)]
struct CloseSessionRequest {
    session_id: String,
}

#[derive(Serialize)]
struct SetProfileRequest {
    preset: CameraStreamPreset,
}

#[derive(Deserialize)]
struct ControlResponse {
    version: u16,
    #[serde(flatten)]
    outcome: ResponseOutcome,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ResponseOutcome {
    Ok(ResponseResult),
    Error(ControlErrorResponse),
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ResponseResult {
    Status(CameraStatusWire),
    SessionCreated(SessionCreatedResponse),
    SessionClosed,
    ProfileSet,
    ShutdownAccepted,
}

#[derive(Deserialize)]
struct CameraStatusWire {
    available: bool,
    pipeline: CameraPipelineState,
    active_sessions: u8,
    profile: CameraStreamProfileWire,
    #[serde(default)]
    error: Option<CameraErrorCategory>,
}

#[derive(Deserialize)]
struct CameraStreamProfileWire {
    width: u16,
    height: u16,
    fps: u8,
    bitrate_bps: u32,
    codec: String,
}

#[derive(Deserialize)]
struct SessionCreatedResponse {
    session_id: String,
    answer_sdp: String,
    negotiation_timeout_seconds: u16,
}

#[derive(Deserialize)]
struct ControlErrorResponse {
    code: ControlErrorCode,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ControlErrorCode {
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

fn map_control_error(code: ControlErrorCode) -> CameraError {
    match code {
        ControlErrorCode::InvalidRequest
        | ControlErrorCode::RequestTooLarge
        | ControlErrorCode::InvalidAccessScope => CameraError::InvalidRequest,
        ControlErrorCode::ShuttingDown => CameraError::NotReady,
        ControlErrorCode::SessionBusy | ControlErrorCode::CameraBusy => CameraError::Busy,
        ControlErrorCode::UnknownSession => CameraError::UnknownSession,
        ControlErrorCode::UnsupportedSdp => CameraError::UnsupportedOffer,
        ControlErrorCode::ResourceExhausted => CameraError::ResourceExhausted,
        ControlErrorCode::InvalidVersion
        | ControlErrorCode::PermissionDenied
        | ControlErrorCode::CameraNotFound
        | ControlErrorCode::EncoderUnavailable
        | ControlErrorCode::MediaPipelineFailed
        | ControlErrorCode::WebRtcNegotiationFailed
        | ControlErrorCode::WebRtcTransportFailed
        | ControlErrorCode::Internal => CameraError::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_status_wire_maps_private_profile_to_public_h264() {
        let response: ControlResponse = serde_json::from_value(serde_json::json!({
            "version": 1,
            "ok": {
                "status": {
                    "available": true,
                    "pipeline": "stopped",
                    "active_sessions": 0,
                    "profile": {
                        "width": 3840,
                        "height": 2160,
                        "fps": 30,
                        "bitrate_bps": 20000000,
                        "codec": "h264_baseline"
                    }
                }
            }
        }))
        .unwrap();
        let ResponseOutcome::Ok(ResponseResult::Status(status)) = response.outcome else {
            panic!("expected camera status response");
        };
        assert_eq!(status.profile.codec, "h264_baseline");
        assert_eq!(status.profile.width, 3840);
        assert_eq!(status.profile.bitrate_bps, 20_000_000);
    }

    #[test]
    fn request_shape_is_versioned_and_typed() {
        let body = serde_json::to_value(ControlRequest {
            version: CAMERA_CONTROL_PROTOCOL_VERSION,
            operation: RequestOperation::CreateSession(CreateSessionRequest {
                scope: CameraAccessKind::Lan,
                address: Ipv4Addr::new(192, 168, 8, 1),
                offer_sdp: "v=0\r\n".to_owned(),
            }),
        })
        .unwrap();
        assert_eq!(body["version"], 1);
        assert_eq!(body["operation"]["create_session"]["scope"], "lan");
        assert!(body["operation"]["create_session"].get("port").is_none());
    }

    #[test]
    fn set_profile_request_is_typed_and_enum_scoped() {
        let body = serde_json::to_value(ControlRequest {
            version: CAMERA_CONTROL_PROTOCOL_VERSION,
            operation: RequestOperation::SetProfile(SetProfileRequest {
                preset: CameraStreamPreset::Fhd1080p5m,
            }),
        })
        .unwrap();
        assert_eq!(body["version"], 1);
        assert_eq!(body["operation"]["set_profile"]["preset"], "fhd1080p5m");
    }

    #[test]
    fn adapter_socket_path_is_fixed_in_production() {
        assert_eq!(
            CameraUnixAdapter::default().socket,
            PathBuf::from(CAMERA_CONTROL_SOCKET)
        );
        let test = CameraUnixAdapter::at(PathBuf::from("/tmp/test-camera.sock"));
        assert_ne!(test.socket, CameraUnixAdapter::default().socket);
    }
}
