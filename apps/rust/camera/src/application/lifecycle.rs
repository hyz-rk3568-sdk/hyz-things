use super::ports::{
    CameraMediaPort, CreatedWebRtcSession, MediaError, MediaTerminator, RunningMedia,
    RunningWebRtcSession, WebRtcError, WebRtcSessionPort,
};
use crate::domain::{
    BoundedFrameQueue, CameraAccessKind, CameraAccessScope, CameraErrorCategory,
    CameraPipelineState, CameraRotation, CameraSessionId, CameraStatus, CameraStreamPreset,
    CameraStreamProfile,
};
use std::{
    net::Ipv4Addr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

pub const NEGOTIATION_TIMEOUT_SECONDS: u16 = 30;

/// 同一编码流同时扇出的最大 viewer 数。超过返回 `TooManyViewers`（控制协议映射为
/// `ResourceExhausted`，router 侧 HTTP 503）。
pub const MAX_VIEWERS: usize = 4;

pub struct CameraApplication {
    media: Arc<dyn CameraMediaPort>,
    webrtc: Arc<dyn WebRtcSessionPort>,
    generation: String,
    state: Mutex<ApplicationState>,
}

struct ApplicationState {
    accepting: bool,
    media: Option<SharedMedia>,
    sessions: Vec<ActiveSession>,
    last_error: Option<CameraErrorCategory>,
    preset: CameraStreamPreset,
    rotation: CameraRotation,
}

/// 所有 viewer 共享的一路媒体管线。`viewers` 计数持有该管线 terminator 的会话线程：
/// 归零（最后一个 viewer 退出）时立即停止管线。`profile` 记录启动该管线时的完整
/// 媒体配置（预设 + 旋转）：配置变更后旧管线必须重建，不能把残留管线复用给
/// 期望新配置的 viewer。
struct SharedMedia {
    running: Box<dyn RunningMedia>,
    viewers: Arc<AtomicUsize>,
    profile: CameraStreamProfile,
}

struct ActiveSession {
    id: CameraSessionId,
    webrtc: Box<dyn RunningWebRtcSession>,
    frames: Arc<BoundedFrameQueue>,
}

/// 引用计数 terminator：每个 viewer 会话线程的 `MediaTerminationGuard` 持有一个，
/// 线程退出时 `terminate()` 递减；减到 0 才真正停止共享管线（幂等，天然防止单个
/// viewer 退出误停其他 viewer 的管线）。
struct RefCountedTerminator {
    inner: Arc<dyn MediaTerminator>,
    viewers: Arc<AtomicUsize>,
}

impl MediaTerminator for RefCountedTerminator {
    fn terminate(&self) {
        if self.viewers.load(Ordering::Acquire) == 0 {
            return;
        }
        let previous = self.viewers.fetch_sub(1, Ordering::AcqRel);
        if previous == 1 {
            self.inner.terminate();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateSessionResult {
    pub session_id: CameraSessionId,
    pub answer_sdp: String,
    pub negotiation_timeout_seconds: u16,
}

impl CameraApplication {
    pub fn new(
        media: Arc<dyn CameraMediaPort>,
        webrtc: Arc<dyn WebRtcSessionPort>,
        generation: String,
    ) -> Result<Self, CameraApplicationError> {
        if generation.is_empty() || generation.len() > 32 {
            return Err(CameraApplicationError::InvalidGeneration);
        }
        let last_error = media.probe().err().map(media_category);
        Ok(Self {
            media,
            webrtc,
            generation,
            state: Mutex::new(ApplicationState {
                accepting: true,
                media: None,
                sessions: Vec::new(),
                last_error,
                preset: CameraStreamPreset::default(),
                rotation: CameraRotation::Deg0,
            }),
        })
    }

    pub fn status(&self) -> CameraStatus {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reap_finished_sessions(&mut state);
        stop_shared_media_when_last_viewer_left(&mut state);
        if state.accepting && state.media.is_none() {
            state.last_error = self.media.probe().err().map(media_category);
        }
        let pipeline = state
            .media
            .as_ref()
            .map(|shared| shared.running.state())
            .unwrap_or(CameraPipelineState::Stopped);
        CameraStatus {
            available: state.accepting && state.last_error.is_none(),
            pipeline,
            active_sessions: u8::try_from(state.sessions.len()).unwrap_or(u8::MAX),
            profile: configured_profile(&state),
            error: state.last_error,
        }
    }

    pub fn set_profile(&self, preset: CameraStreamPreset) -> Result<(), CameraApplicationError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.accepting {
            return Err(CameraApplicationError::ShuttingDown);
        }
        if !state.sessions.is_empty() {
            return Err(CameraApplicationError::SessionBusy);
        }
        if state.preset == preset {
            return Ok(());
        }
        state.preset = preset;
        // 配置已变化：可能仍在跑的上一个配置的共享管线必须停止，否则后续
        // viewer 会通过 create_session 复用旧预设的画面。
        take_and_stop_media(&mut state);
        state.last_error = self.media.probe().err().map(media_category);
        Ok(())
    }

    pub fn set_rotation(&self, rotation: CameraRotation) -> Result<(), CameraApplicationError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.accepting {
            return Err(CameraApplicationError::ShuttingDown);
        }
        if !state.sessions.is_empty() {
            return Err(CameraApplicationError::SessionBusy);
        }
        if state.rotation == rotation {
            return Ok(());
        }
        state.rotation = rotation;
        // 配置已变化：停止旧配置的共享管线，防止后续 viewer 复用旧旋转画面。
        take_and_stop_media(&mut state);
        state.last_error = self.media.probe().err().map(media_category);
        Ok(())
    }

    pub fn create_session(
        &self,
        kind: CameraAccessKind,
        address: Ipv4Addr,
        offer_sdp: &str,
    ) -> Result<CreateSessionResult, CameraApplicationError> {
        let scope = CameraAccessScope::validate(kind, address)
            .map_err(|_| CameraApplicationError::InvalidAccessScope)?;
        self.webrtc
            .validate_offer(offer_sdp)
            .map_err(CameraApplicationError::WebRtc)?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !state.accepting {
            return Err(CameraApplicationError::ShuttingDown);
        }
        reap_finished_sessions(&mut state);
        stop_shared_media_when_last_viewer_left(&mut state);
        if state.sessions.len() >= MAX_VIEWERS {
            return Err(CameraApplicationError::TooManyViewers);
        }

        let desired = configured_profile(&state);
        let reuse = state
            .media
            .as_ref()
            .is_some_and(|shared| shared.profile == desired);
        if !reuse {
            // 无管线，或残留管线配置与当前配置不一致：停止旧管线并按当前
            // 配置重建，避免 viewer 拿到错误预设/旋转的画面。
            take_and_stop_media(&mut state);
            self.media.probe().map_err(|error| {
                state.last_error = Some(media_category(error));
                CameraApplicationError::Media(error)
            })?;
            let running = self.media.start(desired).map_err(|error| {
                state.last_error = Some(media_category(error));
                CameraApplicationError::Media(error)
            })?;
            state.media = Some(SharedMedia {
                running,
                viewers: Arc::new(AtomicUsize::new(0)),
                profile: desired,
            });
        }

        let mut random = [0u8; 24];
        if getrandom::fill(&mut random).is_err() {
            take_and_stop_media(&mut state);
            state.last_error = Some(CameraErrorCategory::Unknown);
            return Err(CameraApplicationError::RandomUnavailable);
        }
        let id = match CameraSessionId::new(&self.generation, random) {
            Ok(id) => id,
            Err(_) => {
                take_and_stop_media(&mut state);
                return Err(CameraApplicationError::InvalidGeneration);
            }
        };
        let shared = state.media.as_ref().expect("media was just started");
        let frames = shared.running.subscribe();
        let keyframe_requester = shared.running.keyframe_requester();
        let terminator = Arc::new(RefCountedTerminator {
            inner: shared.running.terminator(),
            viewers: Arc::clone(&shared.viewers),
        });
        // 先计数再 spawn：会话线程的 guard 退出时递减，避免在创建成功前减到 0。
        shared.viewers.fetch_add(1, Ordering::AcqRel);

        let CreatedWebRtcSession {
            answer_sdp,
            running,
        } = match self.webrtc.create(
            &id,
            scope,
            offer_sdp,
            Arc::clone(&frames),
            keyframe_requester,
            terminator,
        ) {
            Ok(created) => created,
            Err(error) => {
                rollback_created_viewer(&mut state, &frames);
                state.last_error = Some(webrtc_category(error));
                return Err(CameraApplicationError::WebRtc(error));
            }
        };

        state.last_error = None;
        state.sessions.push(ActiveSession {
            id: id.clone(),
            webrtc: running,
            frames,
        });
        Ok(CreateSessionResult {
            session_id: id,
            answer_sdp,
            negotiation_timeout_seconds: NEGOTIATION_TIMEOUT_SECONDS,
        })
    }

    pub fn close_session(&self, id: &CameraSessionId) -> Result<(), CameraApplicationError> {
        if !id.belongs_to_generation(&self.generation) {
            return Err(CameraApplicationError::UnknownSession);
        }
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.sessions.is_empty() {
            return Ok(());
        }
        let Some(index) = state.sessions.iter().position(|session| &session.id == id) else {
            return Err(CameraApplicationError::UnknownSession);
        };
        let session = state.sessions.remove(index);
        if let Some(shared) = state.media.as_ref() {
            shared.running.unsubscribe(&session.frames);
        }
        let webrtc_result = session.webrtc.stop();
        stop_shared_media_when_last_viewer_left(&mut state);
        webrtc_result.map_err(CameraApplicationError::WebRtc)
    }

    pub fn shutdown(&self) -> Result<(), CameraApplicationError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.accepting = false;
        let sessions = std::mem::take(&mut state.sessions);
        let mut first_error = None;
        for session in sessions {
            if let Some(shared) = state.media.as_ref() {
                shared.running.unsubscribe(&session.frames);
            }
            if let Err(error) = session.webrtc.stop() {
                first_error.get_or_insert(CameraApplicationError::WebRtc(error));
            }
        }
        if let Some(shared) = state.media.take() {
            if let Err(error) = shared.running.stop() {
                first_error.get_or_insert(CameraApplicationError::Media(error));
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn configured_profile(state: &ApplicationState) -> CameraStreamProfile {
    CameraStreamProfile {
        rotation: state.rotation,
        ..state.preset.profile()
    }
}

/// 移除已完成（线程自然退出）的会话：先退订帧队列，再 stop（join 线程，guard 在
/// join 内递减引用计数）。
fn reap_finished_sessions(state: &mut ApplicationState) {
    let mut index = 0;
    while index < state.sessions.len() {
        if !state.sessions[index].webrtc.is_finished() {
            index += 1;
            continue;
        }
        let session = state.sessions.remove(index);
        if let Some(shared) = state.media.as_ref() {
            shared.running.unsubscribe(&session.frames);
        }
        if let Err(error) = session.webrtc.stop() {
            state.last_error = Some(webrtc_category(error));
        }
    }
}

/// 最后一个 viewer 离开后停止共享管线（幂等：引用计数 guard 也可能已触发停止）。
fn stop_shared_media_when_last_viewer_left(state: &mut ApplicationState) {
    if !state.sessions.is_empty() {
        return;
    }
    take_and_stop_media(state);
}

/// 取出并停止共享管线（幂等）。
fn take_and_stop_media(state: &mut ApplicationState) {
    if let Some(shared) = state.media.take() {
        if let Err(error) = shared.running.stop() {
            state.last_error = Some(media_category(error));
        }
    }
}

/// 创建会话失败后的回滚：退订帧队列、归还引用计数；无其他 viewer 时停止共享管线。
fn rollback_created_viewer(state: &mut ApplicationState, frames: &Arc<BoundedFrameQueue>) {
    if let Some(shared) = state.media.as_ref() {
        shared.viewers.fetch_sub(1, Ordering::AcqRel);
        shared.running.unsubscribe(frames);
    }
    stop_shared_media_when_last_viewer_left(state);
}

fn media_category(error: MediaError) -> CameraErrorCategory {
    match error {
        MediaError::CameraNotFound => CameraErrorCategory::CameraNotFound,
        MediaError::CameraBusy => CameraErrorCategory::CameraBusy,
        MediaError::EncoderUnavailable => CameraErrorCategory::EncoderUnavailable,
        MediaError::PipelineFailed => CameraErrorCategory::MediaPipelineFailed,
        MediaError::UnknownOwnership => CameraErrorCategory::Unknown,
    }
}

fn webrtc_category(error: WebRtcError) -> CameraErrorCategory {
    match error {
        WebRtcError::ResourceExhausted => CameraErrorCategory::ResourceExhausted,
        WebRtcError::UnsupportedSdp | WebRtcError::SdpTooLarge | WebRtcError::NegotiationFailed => {
            CameraErrorCategory::WebRtcNegotiationFailed
        }
        WebRtcError::TransportFailed => CameraErrorCategory::WebRtcTransportFailed,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CameraApplicationError {
    #[error("camera access scope is invalid")]
    InvalidAccessScope,
    #[error("camera stream preset is invalid")]
    InvalidProfile,
    #[error("camera daemon generation is invalid")]
    InvalidGeneration,
    #[error("camera daemon is shutting down")]
    ShuttingDown,
    #[error("camera already has the maximum number of viewers")]
    TooManyViewers,
    #[error("camera is busy with active viewers")]
    SessionBusy,
    #[error("camera session is unknown")]
    UnknownSession,
    #[error("secure random generation is unavailable")]
    RandomUnavailable,
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    WebRtc(#[from] WebRtcError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::KeyframeRequester;
    use crate::domain::{FrameHub, FIXED_LAN_ADDRESS};
    use std::sync::{
        atomic::{AtomicBool, AtomicU8},
        Mutex,
    };

    fn offer() -> String {
        "v=0\r\n".to_owned()
    }

    #[derive(Default)]
    struct FakeMediaPort {
        starts: AtomicUsize,
        stop_count: Arc<AtomicUsize>,
        state: Arc<AtomicU8>,
        hub: Arc<FrameHub>,
        /// 最近一次 start 使用的完整媒体配置：断言管线按当前配置（预设+旋转）重建。
        last_profile: Mutex<Option<CameraStreamProfile>>,
    }

    impl CameraMediaPort for FakeMediaPort {
        fn probe(&self) -> Result<(), MediaError> {
            Ok(())
        }

        fn start(&self, profile: CameraStreamProfile) -> Result<Box<dyn RunningMedia>, MediaError> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            *self
                .last_profile
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(profile);
            self.state.store(1, Ordering::Release);
            Ok(Box::new(FakeRunningMedia {
                hub: Arc::clone(&self.hub),
                stop_count: Arc::clone(&self.stop_count),
                state: Arc::clone(&self.state),
            }))
        }
    }

    struct FakeRunningMedia {
        hub: Arc<FrameHub>,
        stop_count: Arc<AtomicUsize>,
        state: Arc<AtomicU8>,
    }

    impl RunningMedia for FakeRunningMedia {
        fn subscribe(&self) -> Arc<BoundedFrameQueue> {
            self.hub.subscribe()
        }

        fn unsubscribe(&self, queue: &Arc<BoundedFrameQueue>) {
            self.hub.unsubscribe(queue);
        }

        fn keyframe_requester(&self) -> Arc<dyn KeyframeRequester> {
            Arc::new(FakeKeyframeRequester)
        }

        fn terminator(&self) -> Arc<dyn MediaTerminator> {
            Arc::new(FakeMediaTerminator {
                hub: Arc::clone(&self.hub),
                state: Arc::clone(&self.state),
            })
        }

        fn state(&self) -> CameraPipelineState {
            if self.state.load(Ordering::Acquire) == 1 {
                CameraPipelineState::Streaming
            } else {
                CameraPipelineState::Stopped
            }
        }

        fn stop(self: Box<Self>) -> Result<(), MediaError> {
            self.state.store(0, Ordering::Release);
            self.hub.close();
            self.stop_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct FakeKeyframeRequester;

    impl KeyframeRequester for FakeKeyframeRequester {
        fn request_keyframe(&self) -> Result<(), MediaError> {
            Ok(())
        }
    }

    struct FakeMediaTerminator {
        hub: Arc<FrameHub>,
        state: Arc<AtomicU8>,
    }

    impl MediaTerminator for FakeMediaTerminator {
        fn terminate(&self) {
            self.state.store(0, Ordering::Release);
            self.hub.close();
        }
    }

    #[derive(Default)]
    struct FakeWebRtcPort {
        created: AtomicUsize,
        sessions: Mutex<Vec<Arc<FakeRunningSession>>>,
    }

    impl WebRtcSessionPort for FakeWebRtcPort {
        fn validate_offer(&self, _offer_sdp: &str) -> Result<(), WebRtcError> {
            Ok(())
        }

        fn create(
            &self,
            _session_id: &CameraSessionId,
            _scope: CameraAccessScope,
            _offer_sdp: &str,
            _frames: Arc<BoundedFrameQueue>,
            _keyframe_requester: Arc<dyn KeyframeRequester>,
            _media_terminator: Arc<dyn MediaTerminator>,
        ) -> Result<CreatedWebRtcSession, WebRtcError> {
            self.created.fetch_add(1, Ordering::SeqCst);
            let session = Arc::new(FakeRunningSession::default());
            self.sessions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(Arc::clone(&session));
            Ok(CreatedWebRtcSession {
                answer_sdp: "v=0\r\n".to_owned(),
                running: Box::new(session),
            })
        }
    }

    #[derive(Default)]
    struct FakeRunningSession {
        finished: AtomicBool,
        stopped: AtomicBool,
    }

    impl RunningWebRtcSession for Arc<FakeRunningSession> {
        fn is_finished(&self) -> bool {
            self.finished.load(Ordering::Acquire)
        }

        fn stop(self: Box<Self>) -> Result<(), WebRtcError> {
            self.stopped.store(true, Ordering::Release);
            Ok(())
        }
    }

    #[test]
    fn concurrent_sessions_share_one_pipeline() {
        let media = Arc::new(FakeMediaPort::default());
        let webrtc = Arc::new(FakeWebRtcPort::default());
        let app = CameraApplication::new(media.clone(), webrtc.clone(), "test".to_owned()).unwrap();
        let first = app
            .create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        let second = app
            .create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        assert_ne!(first.session_id, second.session_id);
        let status = app.status();
        assert_eq!(status.active_sessions, 2);
        assert_eq!(status.pipeline, CameraPipelineState::Streaming);
        assert_eq!(media.starts.load(Ordering::SeqCst), 1);
        assert_eq!(media.stop_count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn viewer_cap_rejects_extra_viewers() {
        let media = Arc::new(FakeMediaPort::default());
        let webrtc = Arc::new(FakeWebRtcPort::default());
        let app = CameraApplication::new(media.clone(), webrtc.clone(), "test".to_owned()).unwrap();
        for _ in 0..MAX_VIEWERS {
            app.create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
                .unwrap();
        }
        assert_eq!(
            app.create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer()),
            Err(CameraApplicationError::TooManyViewers)
        );
        assert_eq!(app.status().active_sessions, MAX_VIEWERS as u8);
    }

    #[test]
    fn closing_one_session_keeps_others_and_pipeline() {
        let media = Arc::new(FakeMediaPort::default());
        let webrtc = Arc::new(FakeWebRtcPort::default());
        let app = CameraApplication::new(media.clone(), webrtc.clone(), "test".to_owned()).unwrap();
        let first = app
            .create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        let second = app
            .create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        app.close_session(&first.session_id).unwrap();
        let status = app.status();
        assert_eq!(status.active_sessions, 1);
        assert_eq!(status.pipeline, CameraPipelineState::Streaming);
        assert_eq!(media.stop_count.load(Ordering::SeqCst), 0);
        app.close_session(&second.session_id).unwrap();
        let status = app.status();
        assert_eq!(status.active_sessions, 0);
        assert_eq!(status.pipeline, CameraPipelineState::Stopped);
        assert_eq!(media.stop_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn finished_session_is_reaped_and_last_one_stops_media() {
        let media = Arc::new(FakeMediaPort::default());
        let webrtc = Arc::new(FakeWebRtcPort::default());
        let app = CameraApplication::new(media.clone(), webrtc.clone(), "test".to_owned()).unwrap();
        app.create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        app.create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        let handles = webrtc
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        assert_eq!(handles.len(), 2);
        handles[0].finished.store(true, Ordering::Release);
        let status = app.status();
        assert_eq!(status.active_sessions, 1);
        assert_eq!(media.stop_count.load(Ordering::SeqCst), 0);
        assert!(handles[0].stopped.load(Ordering::Acquire));
        assert!(!handles[1].stopped.load(Ordering::Acquire));
        handles[1].finished.store(true, Ordering::Release);
        let status = app.status();
        assert_eq!(status.active_sessions, 0);
        assert_eq!(media.stop_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn profile_and_rotation_are_busy_while_any_viewer_is_active() {
        let media = Arc::new(FakeMediaPort::default());
        let webrtc = Arc::new(FakeWebRtcPort::default());
        let app = CameraApplication::new(media.clone(), webrtc.clone(), "test".to_owned()).unwrap();
        let first = app
            .create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        let second = app
            .create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        assert_eq!(
            app.set_profile(CameraStreamPreset::Uhd4k20m),
            Err(CameraApplicationError::SessionBusy)
        );
        assert_eq!(
            app.set_rotation(CameraRotation::Deg90),
            Err(CameraApplicationError::SessionBusy)
        );
        app.close_session(&first.session_id).unwrap();
        assert_eq!(
            app.set_profile(CameraStreamPreset::Uhd4k20m),
            Err(CameraApplicationError::SessionBusy)
        );
        app.close_session(&second.session_id).unwrap();
        assert_eq!(app.set_profile(CameraStreamPreset::Uhd4k20m), Ok(()));
        assert_eq!(app.set_rotation(CameraRotation::Deg90), Ok(()));
    }

    #[test]
    fn configuration_changes_stop_stale_pipeline_and_new_session_uses_new_configuration() {
        let media = Arc::new(FakeMediaPort::default());
        let webrtc = Arc::new(FakeWebRtcPort::default());
        let app = CameraApplication::new(media.clone(), webrtc.clone(), "test".to_owned()).unwrap();
        let session = app
            .create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        assert_eq!(media.starts.load(Ordering::SeqCst), 1);
        app.close_session(&session.session_id).unwrap();
        // 最后一个 viewer 离开：默认 720p 管线已停止。
        assert_eq!(media.stop_count.load(Ordering::SeqCst), 1);

        // 无 viewer 时改配置：旧配置管线立即停止（不等空闲清理），避免
        // 后续 create_session 复用旧预设/旋转的画面。
        app.set_profile(CameraStreamPreset::Uhd4k20m).unwrap();
        app.set_rotation(CameraRotation::Deg90).unwrap();
        assert_eq!(media.stop_count.load(Ordering::SeqCst), 3);

        let session = app
            .create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        assert_eq!(media.starts.load(Ordering::SeqCst), 2);
        let profile = media
            .last_profile
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .expect("media started with a profile");
        assert_eq!(profile.width, 3840);
        assert_eq!(profile.height, 2160);
        assert_eq!(profile.rotation, CameraRotation::Deg90);
        app.close_session(&session.session_id).unwrap();
    }

    #[test]
    fn residual_finished_session_does_not_force_new_viewer_onto_stale_pipeline() {
        let media = Arc::new(FakeMediaPort::default());
        let webrtc = Arc::new(FakeWebRtcPort::default());
        let app = CameraApplication::new(media.clone(), webrtc.clone(), "test".to_owned()).unwrap();
        let _first = app
            .create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        // 模拟浏览器异常退出：会话线程已结束但服务端尚未收到 close。
        let handles = webrtc
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        handles[0].finished.store(true, Ordering::Release);
        assert_eq!(
            app.set_profile(CameraStreamPreset::Uhd4k20m),
            Err(CameraApplicationError::SessionBusy)
        );
        // status 触发 reap 并把最后一个 viewer 的管线停掉。
        let status = app.status();
        assert_eq!(status.active_sessions, 0);
        assert_eq!(media.stop_count.load(Ordering::SeqCst), 1);
        app.set_profile(CameraStreamPreset::Uhd4k20m).unwrap();
        assert_eq!(media.stop_count.load(Ordering::SeqCst), 2);
        app.create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        assert_eq!(media.starts.load(Ordering::SeqCst), 2);
        let profile = media
            .last_profile
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .expect("media started with a profile");
        assert_eq!(profile.width, 3840);
        assert_eq!(profile.height, 2160);
    }

    #[test]
    fn shutdown_stops_all_sessions_and_media() {
        let media = Arc::new(FakeMediaPort::default());
        let webrtc = Arc::new(FakeWebRtcPort::default());
        let app = CameraApplication::new(media.clone(), webrtc.clone(), "test".to_owned()).unwrap();
        app.create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        app.create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        let handles = webrtc
            .sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        app.shutdown().unwrap();
        assert_eq!(media.stop_count.load(Ordering::SeqCst), 1);
        for handle in &handles {
            assert!(handle.stopped.load(Ordering::Acquire));
        }
        assert_eq!(
            app.create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer()),
            Err(CameraApplicationError::ShuttingDown)
        );
        assert!(!app.status().available);
    }

    #[test]
    fn close_session_rejects_unknown_ids_and_repeats() {
        let media = Arc::new(FakeMediaPort::default());
        let webrtc = Arc::new(FakeWebRtcPort::default());
        let app = CameraApplication::new(media.clone(), webrtc.clone(), "test".to_owned()).unwrap();
        let first = app
            .create_session(CameraAccessKind::Lan, FIXED_LAN_ADDRESS, &offer())
            .unwrap();
        let foreign = CameraSessionId::from_parts("other", &"a".repeat(48)).unwrap();
        assert_eq!(
            app.close_session(&foreign),
            Err(CameraApplicationError::UnknownSession)
        );
        app.close_session(&first.session_id).unwrap();
        assert_eq!(app.close_session(&first.session_id), Ok(()));
    }

    #[test]
    fn refcounted_terminator_stops_only_when_last_viewer_exits() {
        struct CountingTerminator(AtomicUsize);
        impl MediaTerminator for CountingTerminator {
            fn terminate(&self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let inner = Arc::new(CountingTerminator(AtomicUsize::new(0)));
        let viewers = Arc::new(AtomicUsize::new(2));
        let first = RefCountedTerminator {
            inner: inner.clone(),
            viewers: Arc::clone(&viewers),
        };
        let second = RefCountedTerminator {
            inner: inner.clone(),
            viewers: Arc::clone(&viewers),
        };
        first.terminate();
        assert_eq!(inner.0.load(Ordering::SeqCst), 0);
        second.terminate();
        assert_eq!(inner.0.load(Ordering::SeqCst), 1);
        // 已归零后再次触发不会下溢或重复停止。
        let extra = RefCountedTerminator {
            inner: inner.clone(),
            viewers,
        };
        extra.terminate();
        assert_eq!(inner.0.load(Ordering::SeqCst), 1);
    }
}
