use crate::domain::{
    AudioFrame, BoundedAudioQueue, BoundedFrameQueue, CameraAccessScope, CameraPipelineState,
    CameraSessionId, CameraStreamProfile,
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

/// 全双工对讲音频媒体：采集（设备麦克风 → Opus 扇出）与回放（浏览器 Opus →
/// 设备喇叭，经 audiomixer 混音）由同一条 GStreamer 管线承载，以保证
/// `webrtcdsp`/`webrtcechoprobe` 的回声参考信号耦合。
pub trait CameraAudioPort: Send + Sync {
    fn probe(&self) -> Result<(), MediaError>;
    fn start(&self) -> Result<Box<dyn RunningAudioMedia>, MediaError>;
}

pub trait RunningAudioMedia: Send {
    /// 为一个音频会话注册独立采集队列（扇出）；media 已停止时返回已关闭队列。
    fn subscribe_capture(&self) -> Arc<BoundedAudioQueue>;
    fn unsubscribe_capture(&self, queue: &Arc<BoundedAudioQueue>);
    /// 引用计数 terminator：最后一个音频会话线程退出时停止音频管线。
    fn terminator(&self) -> Arc<dyn MediaTerminator>;
    /// 共享回放 sink：会话注册/注销自己的混音输入并推送 Opus。
    fn playback(&self) -> Arc<dyn AudioSink>;
    fn state(&self) -> CameraPipelineState;
    fn stop(self: Box<Self>) -> Result<(), MediaError>;
}

/// 浏览器 → 设备喇叭的回放输入。一个会话一个混音支路（上限 `MAX_VIEWERS`），
/// 多个说话者同时发声时由 GStreamer `audiomixer` 混音。
pub trait AudioSink: Send + Sync {
    fn register(&self, session_id: &str) -> Result<(), MediaError>;
    fn unregister(&self, session_id: &str);
    fn push_opus(&self, session_id: &str, frame: AudioFrame);
}

/// 会话协商到 audio 时传给 WebRTC adapter 的音频参数。
pub struct SessionAudio {
    pub frames: Arc<BoundedAudioQueue>,
    pub sink: Arc<dyn AudioSink>,
    pub media_terminator: Arc<dyn MediaTerminator>,
}

/// `WebRtcSessionPort::create` 的参数（收敛签名，避免过多参数）。
pub struct SessionCreate {
    pub session_id: CameraSessionId,
    pub scope: CameraAccessScope,
    pub offer_sdp: String,
    pub frames: Arc<BoundedFrameQueue>,
    pub keyframe_requester: Arc<dyn KeyframeRequester>,
    pub media_terminator: Arc<dyn MediaTerminator>,
    pub audio: Option<SessionAudio>,
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

    fn create(&self, session: SessionCreate) -> Result<CreatedWebRtcSession, WebRtcError>;
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
    #[error("fixed audio device was not found")]
    AudioDeviceNotFound,
    #[error("fixed audio device is busy")]
    AudioDeviceBusy,
    #[error("audio pipeline failed")]
    AudioPipelineFailed,
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
