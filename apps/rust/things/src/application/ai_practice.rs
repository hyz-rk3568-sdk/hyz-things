use crate::domain::ai_practice::AiPracticeQuestionId;
use std::sync::Arc;

#[derive(Debug, PartialEq, Eq)]
pub enum AiPracticeError {
    Invalid,
    NotFound,
    Busy,
    Unavailable,
}

#[async_trait::async_trait]
pub trait AiPracticeFavoritesPort: Send + Sync {
    async fn set(&self, id: &AiPracticeQuestionId, favorite: bool) -> Result<(), AiPracticeError>;
    async fn list(&self) -> Result<Vec<AiPracticeQuestionId>, AiPracticeError>;
}

pub struct AiPracticeFavorites {
    repository: Arc<dyn AiPracticeFavoritesPort>,
}
impl AiPracticeFavorites {
    pub fn new(repository: Arc<dyn AiPracticeFavoritesPort>) -> Self {
        Self { repository }
    }
    pub async fn set(
        &self,
        id: &AiPracticeQuestionId,
        favorite: bool,
    ) -> Result<(), AiPracticeError> {
        if !id.is_valid() {
            return Err(AiPracticeError::Invalid);
        }
        self.repository.set(id, favorite).await
    }
    pub async fn list(&self) -> Result<Vec<AiPracticeQuestionId>, AiPracticeError> {
        self.repository.list().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ai_practice::AiPracticeModule;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Fake(Mutex<Vec<AiPracticeQuestionId>>);
    #[async_trait::async_trait]
    impl AiPracticeFavoritesPort for Fake {
        async fn set(
            &self,
            id: &AiPracticeQuestionId,
            favorite: bool,
        ) -> Result<(), AiPracticeError> {
            let mut entries = self.0.lock().unwrap();
            entries.retain(|entry| entry != id);
            if favorite {
                entries.push(id.clone());
            }
            Ok(())
        }
        async fn list(&self) -> Result<Vec<AiPracticeQuestionId>, AiPracticeError> {
            Ok(self.0.lock().unwrap().clone())
        }
    }
    #[tokio::test]
    async fn favorite_tag_is_shared_by_session_and_question() {
        let app = AiPracticeFavorites::new(Arc::new(Fake::default()));
        let id = AiPracticeQuestionId {
            module: AiPracticeModule::LogicJudgment,
            session_id: "019a31b7-8c42-7c16-a6d1-0c1f0f34c821".into(),
            question: 1,
        };
        app.set(&id, true).await.unwrap();
        app.set(&id, true).await.unwrap();
        assert_eq!(app.list().await.unwrap(), vec![id.clone()]);
        app.set(&id, false).await.unwrap();
        assert!(app.list().await.unwrap().is_empty());
    }
}

#[async_trait::async_trait]
pub trait AiPracticeAgentPort: Send + Sync {
    async fn state(
        &self,
        id: &AiPracticeQuestionId,
    ) -> Result<crate::domain::ai_practice::AiPracticeAgentState, AiPracticeError>;
    async fn attach(
        &self,
        id: &AiPracticeQuestionId,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<String, AiPracticeError>;
    async fn send(
        &self,
        id: &AiPracticeQuestionId,
        message: &str,
        attachments: &[String],
    ) -> Result<(), AiPracticeError>;
    async fn stop(&self, id: &AiPracticeQuestionId) -> Result<(), AiPracticeError>;
}

pub struct AiPracticeAgent {
    port: Arc<dyn AiPracticeAgentPort>,
}
impl AiPracticeAgent {
    pub fn new(port: Arc<dyn AiPracticeAgentPort>) -> Self {
        Self { port }
    }
    pub async fn state(
        &self,
        id: &AiPracticeQuestionId,
    ) -> Result<crate::domain::ai_practice::AiPracticeAgentState, AiPracticeError> {
        if !id.is_valid() {
            return Err(AiPracticeError::Invalid);
        }
        self.port.state(id).await
    }
    pub async fn attach(
        &self,
        id: &AiPracticeQuestionId,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<String, AiPracticeError> {
        if !id.is_valid() || !matches!(media_type, "image/png" | "image/jpeg") {
            return Err(AiPracticeError::Invalid);
        }
        self.port.attach(id, media_type, bytes).await
    }
    pub async fn send(
        &self,
        id: &AiPracticeQuestionId,
        message: &str,
        attachments: &[String],
    ) -> Result<(), AiPracticeError> {
        let message = message.trim();
        if !id.is_valid()
            || (message.is_empty() && attachments.is_empty())
            || message.len() > 8000
            || attachments.len() > 3
        {
            return Err(AiPracticeError::Invalid);
        }
        let message = if message.is_empty() {
            "请解析图片并说明思路"
        } else {
            message
        };
        self.port.send(id, message, attachments).await
    }
    pub async fn stop(&self, id: &AiPracticeQuestionId) -> Result<(), AiPracticeError> {
        if !id.is_valid() {
            return Err(AiPracticeError::Invalid);
        }
        self.port.stop(id).await
    }
}

#[cfg(test)]
mod agent_tests {
    use super::*;
    use crate::domain::ai_practice::{AiPracticeAgentState, AiPracticeMessage, AiPracticeModule};
    use std::sync::Mutex;
    #[derive(Default)]
    struct Fake(Mutex<Vec<(AiPracticeQuestionId, String)>>);
    #[async_trait::async_trait]
    impl AiPracticeAgentPort for Fake {
        async fn state(
            &self,
            id: &AiPracticeQuestionId,
        ) -> Result<AiPracticeAgentState, AiPracticeError> {
            let messages = self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|(key, _)| key == id)
                .map(|(_, content)| AiPracticeMessage {
                    role: "user".into(),
                    content: content.clone(),
                })
                .collect();
            Ok(AiPracticeAgentState {
                status: "idle".into(),
                messages,
                partial: String::new(),
                error: None,
            })
        }
        async fn attach(
            &self,
            _: &AiPracticeQuestionId,
            _: &str,
            _: &[u8],
        ) -> Result<String, AiPracticeError> {
            Ok("attachment".into())
        }
        async fn send(
            &self,
            id: &AiPracticeQuestionId,
            message: &str,
            _: &[String],
        ) -> Result<(), AiPracticeError> {
            self.0.lock().unwrap().push((id.clone(), message.into()));
            Ok(())
        }
        async fn stop(&self, _: &AiPracticeQuestionId) -> Result<(), AiPracticeError> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn agent_messages_are_scoped_to_session_and_question() {
        let app = AiPracticeAgent::new(Arc::new(Fake::default()));
        let id = AiPracticeQuestionId {
            module: AiPracticeModule::DataAnalysis,
            session_id: "019a31b7-8c42-7c16-a6d1-0c1f0f34c821".into(),
            question: 0,
        };
        app.send(&id, "请解题", &[]).await.unwrap();
        assert_eq!(app.state(&id).await.unwrap().messages[0].content, "请解题");
        let mut other = id.clone();
        other.question = 1;
        assert!(app.state(&other).await.unwrap().messages.is_empty());
    }
    #[tokio::test]
    async fn agent_accepts_an_image_without_text() {
        let app = AiPracticeAgent::new(Arc::new(Fake::default()));
        let id = AiPracticeQuestionId {
            module: AiPracticeModule::DataAnalysis,
            session_id: "019a31b7-8c42-7c16-a6d1-0c1f0f34c821".into(),
            question: 0,
        };
        app.send(&id, "", &["attachment".into()]).await.unwrap();
        assert_eq!(
            app.state(&id).await.unwrap().messages[0].content,
            "请解析图片并说明思路"
        );
    }
}
