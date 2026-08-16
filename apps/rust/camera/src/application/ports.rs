use crate::domain::{
    BoundedFrameQueue, CameraAccessScope, CameraPipelineState, CameraSessionId, CameraStreamProfile,
};
use std::sync::Arc;

pub trait KeyframeRequester: Send + Sync {
    fn request_keyframe(&self) -> Result<(), MediaError>;
}

pub trait MediaTerminator: Send + Sync {
    fn terminate(&self);
}

pub trait RunningMedia: Send {
    /// 为一个 viewer 注册独立帧队列（同一编码流扇出）；media 已停止时返回已关闭队列。
    fn subscribe(&self) -> Arc<BoundedFrameQueue>;
    /// viewer 退出时注销其队列，停止投递。
    fn unsubscribe(&self, queue: &Arc<BoundedFrameQueue>);
    fn keyframe_requester(&self) -> Arc<dyn KeyframeRequester>;
    fn terminator(&self) -> Arc<dyn MediaTerminator>;
    fn state(&self) -> CameraPipelineState;
    fn stop(self: Box<Self>) -> Result<(), MediaError>;
}

pub trait CameraMediaPort: Send + Sync {
    fn probe(&self) -> Result<(), MediaError>;
    fn start(&self, profile: CameraStreamProfile) -> Result<Box<dyn RunningMedia>, MediaError>;
}

pub trait RunningWebRtcSession: Send {
    fn is_finished(&self) -> bool;
    fn stop(self: Box<Self>) -> Result<(), WebRtcError>;
}

pub struct CreatedWebRtcSession {
    pub answer_sdp: String,
    pub running: Box<dyn RunningWebRtcSession>,
}

pub trait WebRtcSessionPort: Send + Sync {
    fn validate_offer(&self, offer_sdp: &str) -> Result<(), WebRtcError>;

    fn create(
        &self,
        session_id: &CameraSessionId,
        scope: CameraAccessScope,
        offer_sdp: &str,
        frames: Arc<BoundedFrameQueue>,
        keyframe_requester: Arc<dyn KeyframeRequester>,
        media_terminator: Arc<dyn MediaTerminator>,
    ) -> Result<CreatedWebRtcSession, WebRtcError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    #[error("fixed camera device was not found")]
    CameraNotFound,
    #[error("fixed camera device is busy")]
    CameraBusy,
    #[error("required H.264 encoder is unavailable")]
    EncoderUnavailable,
    #[error("media pipeline failed")]
    PipelineFailed,
    #[error("media ownership is unknown")]
    UnknownOwnership,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WebRtcError {
    #[error("SDP offer is unsupported")]
    UnsupportedSdp,
    #[error("SDP offer is too large")]
    SdpTooLarge,
    #[error("camera UDP port pool is exhausted")]
    ResourceExhausted,
    #[error("WebRTC negotiation failed")]
    NegotiationFailed,
    #[error("WebRTC transport failed")]
    TransportFailed,
}
