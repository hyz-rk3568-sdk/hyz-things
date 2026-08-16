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
    sessions: Mutex<HashMap<[u8; 32], Vec<String>>>,
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

    /// 一个管理员账号可以同时持有多个观看会话（多窗口/多设备同账号并发）。
    pub async fn create_owned_session(
        &self,
        owner: [u8; 32],
        scope: CameraAccessScope,
        offer_sdp: String,
    ) -> Result<CameraSession, CameraError> {
        let mut sessions = self.sessions.lock().await;
        let session = self.create_session(scope, offer_sdp).await?;
        sessions
            .entry(owner)
            .or_default()
            .push(session.session_id.clone());
        Ok(session)
    }

    pub async fn close_owned_session(
        &self,
        owner: [u8; 32],
        session_id: &str,
    ) -> Result<(), CameraError> {
        let mut sessions = self.sessions.lock().await;
        let (owned_id, empty) = {
            let Some(ids) = sessions.get_mut(&owner) else {
                return Err(CameraError::Forbidden);
            };
            let Some(index) = ids.iter().position(|id| id == session_id) else {
                return Err(CameraError::Forbidden);
            };
            let owned_id = ids.remove(index);
            (owned_id, ids.is_empty())
        };
        if empty {
            sessions.remove(&owner);
        }
        match self.close_session(&owned_id).await {
            Ok(()) | Err(CameraError::UnknownSession) => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub async fn close_owner(&self, owner: [u8; 32]) {
        let session_ids = self
            .sessions
            .lock()
            .await
            .remove(&owner)
            .unwrap_or_default();
        for session_id in session_ids {
            let _ = self.close_session(&session_id).await;
        }
    }

    pub async fn close_all(&self) {
        let sessions = {
            let mut owners = self.sessions.lock().await;
            owners.drain().flat_map(|(_, ids)| ids).collect::<Vec<_>>()
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
    use crate::domain::camera::{
        CameraAccessKind, CameraAudioStatus, CameraPipelineState, CameraStreamProfile,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fake {
        next_session: AtomicU64,
    }

    impl Default for Fake {
        fn default() -> Self {
            Self {
                next_session: AtomicU64::new(1),
            }
        }
    }

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
                audio: Some(CameraAudioStatus { supported: true }),
            })
        }

        async fn create_session(
            &self,
            _scope: CameraAccessScope,
            _offer_sdp: String,
        ) -> Result<CameraSession, CameraError> {
            let index = self.next_session.fetch_add(1, Ordering::SeqCst);
            Ok(CameraSession {
                session_id: format!("{index:064x}"),
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
        let app = CameraApplication::new(Arc::new(Fake::default()));
        let offer = "x".repeat(CAMERA_MAX_SDP_BYTES + 1);
        assert_eq!(
            app.create_session(CameraAccessScope::Lan, offer).await,
            Err(CameraError::InvalidRequest)
        );
    }

    #[tokio::test]
    async fn an_owner_can_hold_multiple_sessions_and_close_only_its_own() {
        let app = CameraApplication::new(Arc::new(Fake::default()));
        let owner = [1u8; 32];
        let first = app
            .create_owned_session(owner, CameraAccessScope::Lan, "v=0\r\n".to_owned())
            .await
            .unwrap();
        let second = app
            .create_owned_session(owner, CameraAccessScope::Lan, "v=0\r\n".to_owned())
            .await
            .unwrap();
        assert_ne!(first.session_id, second.session_id);
        assert_eq!(
            app.close_owned_session([2u8; 32], &first.session_id).await,
            Err(CameraError::Forbidden)
        );
        assert_eq!(
            app.close_owned_session(owner, &first.session_id).await,
            Ok(())
        );
        assert_eq!(
            app.close_owned_session(owner, &second.session_id).await,
            Ok(())
        );
        assert_eq!(
            app.close_owned_session(owner, &first.session_id).await,
            Err(CameraError::Forbidden)
        );
    }

    #[tokio::test]
    async fn logout_closes_all_sessions_of_an_owner() {
        let app = CameraApplication::new(Arc::new(Fake::default()));
        let owner = [7u8; 32];
        app.create_owned_session(owner, CameraAccessScope::Lan, "v=0\r\n".to_owned())
            .await
            .unwrap();
        app.create_owned_session(owner, CameraAccessScope::Lan, "v=0\r\n".to_owned())
            .await
            .unwrap();
        app.close_owner(owner).await;
        // 关闭后同 owner 再次关闭任何会话都视为未持有。
        assert_eq!(
            app.close_owned_session(owner, "any").await,
            Err(CameraError::Forbidden)
        );
    }

    #[tokio::test]
    async fn close_all_drains_every_owners_sessions() {
        let app = CameraApplication::new(Arc::new(Fake::default()));
        for owner in [[1u8; 32], [2u8; 32]] {
            app.create_owned_session(owner, CameraAccessScope::Lan, "v=0\r\n".to_owned())
                .await
                .unwrap();
            app.create_owned_session(owner, CameraAccessScope::Lan, "v=0\r\n".to_owned())
                .await
                .unwrap();
        }
        app.close_all().await;
        for owner in [[1u8; 32], [2u8; 32]] {
            assert_eq!(
                app.close_owned_session(owner, "any").await,
                Err(CameraError::Forbidden)
            );
        }
    }

    #[tokio::test]
    async fn accepts_generation_scoped_session_ids() {
        let app = CameraApplication::new(Arc::new(Fake::default()));
        let session_id = format!("{}.{}", "generation", "a".repeat(48));
        assert_eq!(app.close_session(&session_id).await, Ok(()));
        assert_eq!(
            app.close_session("contains/slash").await,
            Err(CameraError::InvalidRequest)
        );
    }

    #[tokio::test]
    async fn derives_public_access_from_server_scope() {
        let app = CameraApplication::new(Arc::new(Fake::default()));
        let status = app.status(CameraAccessScope::Lan).await.unwrap();
        assert_eq!(status.access, CameraAccessKind::Lan);
    }

    #[tokio::test]
    async fn forwards_an_enumerated_preset_to_the_adapter() {
        let app = CameraApplication::new(Arc::new(Fake::default()));
        assert_eq!(
            app.set_profile(CameraStreamPreset::Fhd1080p5m).await,
            Ok(())
        );
    }

    #[tokio::test]
    async fn forwards_an_enumerated_rotation_to_the_adapter() {
        let app = CameraApplication::new(Arc::new(Fake::default()));
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
