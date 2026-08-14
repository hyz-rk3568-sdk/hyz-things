use super::ports::{
    CameraMediaPort, CreatedWebRtcSession, MediaError, RunningMedia, RunningWebRtcSession,
    WebRtcError, WebRtcSessionPort,
};
use crate::domain::{
    CameraAccessKind, CameraAccessScope, CameraErrorCategory, CameraPipelineState, CameraSessionId,
    CameraStatus, FIXED_STREAM_PROFILE,
};
use std::{
    net::Ipv4Addr,
    sync::{Arc, Mutex},
};

pub const NEGOTIATION_TIMEOUT_SECONDS: u16 = 30;

pub struct CameraApplication {
    media: Arc<dyn CameraMediaPort>,
    webrtc: Arc<dyn WebRtcSessionPort>,
    generation: String,
    state: Mutex<ApplicationState>,
}

struct ApplicationState {
    accepting: bool,
    active: Option<ActiveSession>,
    last_error: Option<CameraErrorCategory>,
}

struct ActiveSession {
    id: CameraSessionId,
    media: Box<dyn RunningMedia>,
    webrtc: Box<dyn RunningWebRtcSession>,
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
                active: None,
                last_error,
            }),
        })
    }

    pub fn status(&self) -> CameraStatus {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reap_finished_session(&mut state);
        if state.accepting && state.active.is_none() {
            state.last_error = self.media.probe().err().map(media_category);
        }
        let pipeline = state
            .active
            .as_ref()
            .map(|session| session.media.state())
            .unwrap_or(CameraPipelineState::Stopped);
        CameraStatus {
            available: state.accepting && state.last_error.is_none(),
            pipeline,
            active_sessions: u8::from(state.active.is_some()),
            profile: FIXED_STREAM_PROFILE,
            error: state.last_error,
        }
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
        reap_finished_session(&mut state);
        if state.active.is_some() {
            return Err(CameraApplicationError::SessionBusy);
        }

        self.media.probe().map_err(|error| {
            state.last_error = Some(media_category(error));
            CameraApplicationError::Media(error)
        })?;
        let media = self.media.start().map_err(|error| {
            state.last_error = Some(media_category(error));
            CameraApplicationError::Media(error)
        })?;

        let mut random = [0u8; 24];
        if getrandom::fill(&mut random).is_err() {
            let _ = media.stop();
            state.last_error = Some(CameraErrorCategory::Unknown);
            return Err(CameraApplicationError::RandomUnavailable);
        }
        let id = CameraSessionId::new(&self.generation, random)
            .map_err(|_| CameraApplicationError::InvalidGeneration)?;

        let CreatedWebRtcSession {
            answer_sdp,
            running,
        } = match self.webrtc.create(
            &id,
            scope,
            offer_sdp,
            media.frames(),
            media.keyframe_requester(),
            media.terminator(),
        ) {
            Ok(created) => created,
            Err(error) => {
                let _ = media.stop();
                state.last_error = Some(webrtc_category(error));
                return Err(CameraApplicationError::WebRtc(error));
            }
        };

        state.last_error = None;
        state.active = Some(ActiveSession {
            id: id.clone(),
            media,
            webrtc: running,
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
        let Some(active) = state.active.as_ref() else {
            return Ok(());
        };
        if &active.id != id {
            return Err(CameraApplicationError::UnknownSession);
        }
        let active = state.active.take().expect("active session was checked");
        let webrtc_result = active.webrtc.stop();
        let media_result = active.media.stop();
        if let Err(error) = webrtc_result {
            state.last_error = Some(webrtc_category(error));
            return Err(CameraApplicationError::WebRtc(error));
        }
        if let Err(error) = media_result {
            state.last_error = Some(media_category(error));
            return Err(CameraApplicationError::Media(error));
        }
        Ok(())
    }

    pub fn shutdown(&self) -> Result<(), CameraApplicationError> {
        let active = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.accepting = false;
            state.active.take()
        };
        if let Some(active) = active {
            let webrtc_result = active.webrtc.stop();
            let media_result = active.media.stop();
            webrtc_result.map_err(CameraApplicationError::WebRtc)?;
            media_result.map_err(CameraApplicationError::Media)?;
        }
        Ok(())
    }
}

fn reap_finished_session(state: &mut ApplicationState) {
    let finished = state
        .active
        .as_ref()
        .is_some_and(|active| active.webrtc.is_finished());
    if !finished {
        return;
    }
    let active = state.active.take().expect("finished session was checked");
    if let Err(error) = active.webrtc.stop() {
        state.last_error = Some(webrtc_category(error));
    }
    if let Err(error) = active.media.stop() {
        state.last_error = Some(media_category(error));
    }
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
    #[error("camera daemon generation is invalid")]
    InvalidGeneration,
    #[error("camera daemon is shutting down")]
    ShuttingDown,
    #[error("camera already has an active session")]
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
