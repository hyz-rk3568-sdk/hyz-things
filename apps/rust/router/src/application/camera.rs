use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;

use crate::domain::camera::{
    CameraAccessScope, CameraRotation, CameraStatus, CameraStreamPreset, CAMERA_MAX_SDP_BYTES,
    CAMERA_MAX_SESSION_ID_BYTES,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraError {
    InvalidRequest,
    NotReady,
    Busy,
    UnsupportedOffer,
    ResourceExhausted,
    UnknownSession,
    Forbidden,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraSession {
    pub session_id: String,
    pub answer_sdp: String,
    pub negotiation_timeout_seconds: u16,
}

#[async_trait]
pub trait CameraControlPort: Send + Sync {
    async fn status(&self, scope: CameraAccessScope) -> Result<CameraStatus, CameraError>;

    async fn create_session(
        &self,
        scope: CameraAccessScope,
        offer_sdp: String,
    ) -> Result<CameraSession, CameraError>;

    async fn close_session(&self, session_id: &str) -> Result<(), CameraError>;

    async fn set_profile(&self, preset: CameraStreamPreset) -> Result<(), CameraError>;

    async fn set_rotation(&self, rotation: CameraRotation) -> Result<(), CameraError>;
}

pub struct CameraApplication {
    control: Arc<dyn CameraControlPort>,
    sessions: Mutex<HashMap<[u8; 32], String>>,
}

impl CameraApplication {
    pub fn new(control: Arc<dyn CameraControlPort>) -> Self {
        Self {
            control,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub async fn status(&self, scope: CameraAccessScope) -> Result<CameraStatus, CameraError> {
        self.control.status(scope).await
    }

    pub async fn create_session(
        &self,
        scope: CameraAccessScope,
        offer_sdp: String,
    ) -> Result<CameraSession, CameraError> {
        if offer_sdp.is_empty() || offer_sdp.len() > CAMERA_MAX_SDP_BYTES {
            return Err(CameraError::InvalidRequest);
        }
        self.control.create_session(scope, offer_sdp).await
    }

    pub async fn create_owned_session(
        &self,
        owner: [u8; 32],
        scope: CameraAccessScope,
        offer_sdp: String,
    ) -> Result<CameraSession, CameraError> {
        let mut sessions = self.sessions.lock().await;
        if sessions.contains_key(&owner) {
            return Err(CameraError::Busy);
        }
        let session = self.create_session(scope, offer_sdp).await?;
        sessions.insert(owner, session.session_id.clone());
        Ok(session)
    }

    pub async fn close_owned_session(
        &self,
        owner: [u8; 32],
        session_id: &str,
    ) -> Result<(), CameraError> {
        let mut sessions = self.sessions.lock().await;
        if sessions.get(&owner).map(String::as_str) != Some(session_id) {
            return Err(CameraError::Forbidden);
        }
        match self.close_session(session_id).await {
            Ok(()) | Err(CameraError::UnknownSession) => {
                sessions.remove(&owner);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub async fn close_owner(&self, owner: [u8; 32]) {
        let session_id = self.sessions.lock().await.remove(&owner);
        if let Some(session_id) = session_id {
            let _ = self.close_session(&session_id).await;
        }
    }

    pub async fn close_all(&self) {
        let sessions = {
            let mut owners = self.sessions.lock().await;
            owners
                .drain()
                .map(|(_, session)| session)
                .collect::<Vec<_>>()
        };
        for session_id in sessions {
            let _ = self.close_session(&session_id).await;
        }
    }

    pub async fn close_session(&self, session_id: &str) -> Result<(), CameraError> {
        let valid = !session_id.is_empty()
            && session_id.len() <= CAMERA_MAX_SESSION_ID_BYTES
            && session_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
        if !valid {
            return Err(CameraError::InvalidRequest);
        }
        self.control.close_session(session_id).await
    }

    pub async fn set_profile(&self, preset: CameraStreamPreset) -> Result<(), CameraError> {
        if !CameraStreamPreset::ALL.contains(&preset) {
            return Err(CameraError::InvalidRequest);
        }
        self.control.set_profile(preset).await
    }

    pub async fn set_rotation(&self, rotation: CameraRotation) -> Result<(), CameraError> {
        if !CameraRotation::ALL.contains(&rotation) {
            return Err(CameraError::InvalidRequest);
        }
        self.control.set_rotation(rotation).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::camera::{CameraAccessKind, CameraPipelineState, CameraStreamProfile};

    struct Fake;

    #[async_trait]
    impl CameraControlPort for Fake {
        async fn status(&self, scope: CameraAccessScope) -> Result<CameraStatus, CameraError> {
            Ok(CameraStatus {
                available: true,
                pipeline: CameraPipelineState::Stopped,
                active_sessions: 0,
                profile: CameraStreamProfile {
                    codec: "h264".to_owned(),
                    width: 3840,
                    height: 2160,
                    fps: 30,
                    bitrate_bps: 20_000_000,
                    rotation: CameraRotation::Deg0,
                },
                access: scope.kind(),
                error_category: None,
            })
        }

        async fn create_session(
            &self,
            _scope: CameraAccessScope,
            _offer_sdp: String,
        ) -> Result<CameraSession, CameraError> {
            Ok(CameraSession {
                session_id: "a".repeat(64),
                answer_sdp: "v=0\r\n".to_owned(),
                negotiation_timeout_seconds: 30,
            })
        }

        async fn close_session(&self, _session_id: &str) -> Result<(), CameraError> {
            Ok(())
        }

        async fn set_profile(&self, _preset: CameraStreamPreset) -> Result<(), CameraError> {
            Ok(())
        }

        async fn set_rotation(&self, _rotation: CameraRotation) -> Result<(), CameraError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn rejects_oversized_offer_before_calling_adapter() {
        let app = CameraApplication::new(Arc::new(Fake));
        let offer = "x".repeat(CAMERA_MAX_SDP_BYTES + 1);
        assert_eq!(
            app.create_session(CameraAccessScope::Lan, offer).await,
            Err(CameraError::InvalidRequest)
        );
    }

    #[tokio::test]
    async fn ownership_is_shared_by_the_camera_application() {
        let app = CameraApplication::new(Arc::new(Fake));
        let owner = [1u8; 32];
        let session = app
            .create_owned_session(owner, CameraAccessScope::Lan, "v=0\r\n".to_owned())
            .await
            .unwrap();
        assert_eq!(
            app.create_owned_session(owner, CameraAccessScope::Lan, "v=0\r\n".to_owned())
                .await,
            Err(CameraError::Busy)
        );
        assert_eq!(
            app.close_owned_session([2u8; 32], &session.session_id)
                .await,
            Err(CameraError::Forbidden)
        );
        assert_eq!(
            app.close_owned_session(owner, &session.session_id).await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn accepts_generation_scoped_session_ids() {
        let app = CameraApplication::new(Arc::new(Fake));
        let session_id = format!("{}.{}", "generation", "a".repeat(48));
        assert_eq!(app.close_session(&session_id).await, Ok(()));
        assert_eq!(
            app.close_session("contains/slash").await,
            Err(CameraError::InvalidRequest)
        );
    }

    #[tokio::test]
    async fn derives_public_access_from_server_scope() {
        let app = CameraApplication::new(Arc::new(Fake));
        let status = app.status(CameraAccessScope::Lan).await.unwrap();
        assert_eq!(status.access, CameraAccessKind::Lan);
    }

    #[tokio::test]
    async fn forwards_an_enumerated_preset_to_the_adapter() {
        let app = CameraApplication::new(Arc::new(Fake));
        assert_eq!(
            app.set_profile(CameraStreamPreset::Fhd1080p5m).await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn forwards_an_enumerated_rotation_to_the_adapter() {
        let app = CameraApplication::new(Arc::new(Fake));
        for rotation in CameraRotation::ALL {
            assert_eq!(app.set_rotation(rotation).await, Ok(()));
        }
    }

    #[test]
    fn rotation_serde_rejects_unknown_degrees() {
        assert!(serde_json::from_str::<CameraRotation>("\"deg_0\"").is_ok());
        assert!(serde_json::from_str::<CameraRotation>("\"deg_90\"").is_ok());
        assert!(serde_json::from_str::<CameraRotation>("\"deg_45\"").is_err());
        assert!(serde_json::from_str::<CameraRotation>("\"clockwise\"").is_err());
    }
}
