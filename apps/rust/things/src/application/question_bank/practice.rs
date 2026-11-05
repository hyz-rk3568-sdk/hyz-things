//! Question Bank practice use cases. Never return answer keys before submission.
use super::{
    document::valid_id, BankError, IndexedPaper, PaperContent, PracticeContent,
    QuestionBankApplication,
};
use crate::domain::question_bank::{
    primary_module, CreatePracticeRequest, FavoriteState, ModulePracticeFilters, PaperModule,
    PaperModules, PracticeBrowseItem, PracticeCatalog, PracticeItem, PracticeMode,
    PracticeQuestion, PracticeQuestionDetail, PracticeReview, PracticeScore, PracticeSession,
    PracticeWorkspace, ReviewQuestion,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiReviewLocator {
    pub session_id: String,
    pub question_id: String,
    pub paper_id: String,
    pub material_ids: Vec<String>,
    pub source_hash: String,
    pub user_answer: Vec<String>,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn validate_filters(filters: &ModulePracticeFilters) -> Result<(), BankError> {
    if filters
        .year_min
        .is_some_and(|year| !(1900..=2100).contains(&year))
        || filters
            .year_max
            .is_some_and(|year| !(1900..=2100).contains(&year))
        || matches!((filters.year_min, filters.year_max), (Some(min), Some(max)) if min > max)
        || filters.count.is_some_and(|count| count == 0 || count > 20)
        || filters.regions.len() > 64
        || filters.regions.iter().any(|region| {
            region.trim() != region
                || region.is_empty()
                || region.len() > 100
                || region.chars().any(char::is_control)
        })
        || filters.regions.iter().collect::<BTreeSet<_>>().len() != filters.regions.len()
    {
        return Err(BankError::Invalid);
    }
    Ok(())
}

impl QuestionBankApplication {
    pub(super) async fn primary_indexed(&self, id: &str) -> Result<IndexedPaper, BankError> {
        if !valid_id(id) {
            return Err(BankError::Invalid);
        }
        let paper = self.repository.indexed_paper(id).await?;
        if paper.summary.role != "primary" {
            return Err(BankError::NotFound);
        }
        Ok(paper)
    }

    pub(super) async fn indexed_content(
        &self,
        paper: &IndexedPaper,
    ) -> Result<PaperContent, BankError> {
        let content = self.repository.paper_content(&paper.summary.id).await?;
        if content.paper_id != paper.summary.id || content.hash != paper.hash {
            return Err(BankError::Stale);
        }
        if content.questions.len() != paper.summary.questions {
            return Err(BankError::Unavailable);
        }
        Ok(content)
    }

    pub async fn modules(&self, paper_id: &str) -> Result<PaperModules, BankError> {
        let indexed = self.primary_indexed(paper_id).await?;
        let counts = self
            .repository
            .paper_module_counts(&indexed.summary.id, &indexed.hash)
            .await?;
        let mut modules: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut unclassified = 0;
        let mut total = 0;
        for (module, gradable, ungradable) in counts {
            total += gradable + ungradable;
            let Some(module) = primary_module(module.as_deref()) else {
                unclassified += gradable;
                continue;
            };
            let count = modules.entry(module.into()).or_default();
            count.0 += gradable;
            count.1 += ungradable;
        }
        if total != indexed.summary.questions {
            return Err(BankError::Unavailable);
        }
        Ok(PaperModules {
            modules: modules
                .into_iter()
                .map(|(name, (count, ungradable))| PaperModule {
                    name,
                    count,
                    ungradable,
                })
                .collect(),
            unclassified,
        })
    }

    pub async fn practice_catalog(
        &self,
        filters: &ModulePracticeFilters,
    ) -> Result<PracticeCatalog, BankError> {
        validate_filters(filters)?;
        self.repository.practice_catalog(filters).await
    }

    pub async fn practice_browse(
        &self,
        mode: PracticeMode,
        paper_id: Option<&str>,
        module: Option<&str>,
        category: Option<&str>,
        filters: &ModulePracticeFilters,
    ) -> Result<Vec<PracticeBrowseItem>, BankError> {
        validate_filters(filters)?;
        if matches!(mode, PracticeMode::PaperModule)
            || matches!(mode, PracticeMode::Paper)
                && (paper_id.is_none() || module.is_some() || category.is_some())
            || matches!(mode, PracticeMode::Module) && (paper_id.is_some() || module.is_none())
            || paper_id.is_some_and(|id| !valid_id(id))
            || module.is_some_and(|m| primary_module(Some(m)) != Some(m))
            || category.is_some_and(|c| {
                module.is_none()
                    || c.is_empty()
                    || c.trim() != c
                    || c.len() > 128
                    || c.chars().any(char::is_control)
            })
        {
            return Err(BankError::Invalid);
        }
        self.repository
            .practice_browse(mode, paper_id, module, category, filters)
            .await
    }

    pub async fn create_practice(
        &self,
        request: CreatePracticeRequest,
    ) -> Result<PracticeSession, BankError> {
        validate_filters(&request.filters)?;
        if matches!(
            request.mode,
            PracticeMode::Paper | PracticeMode::PaperModule
        ) && request.filters != ModulePracticeFilters::default()
        {
            return Err(BankError::Invalid);
        }
        let module = match request.mode {
            PracticeMode::Paper => {
                if request.module.is_some()
                    || request.paper_id.is_none()
                    || request.category.is_some()
                {
                    return Err(BankError::Invalid);
                }
                None
            }
            PracticeMode::PaperModule => {
                let raw = request.module.as_deref().ok_or(BankError::Invalid)?;
                let normalized = primary_module(Some(raw)).ok_or(BankError::Invalid)?;
                if request.paper_id.is_none() || raw != normalized || request.category.is_some() {
                    return Err(BankError::Invalid);
                }
                Some(normalized.to_string())
            }
            PracticeMode::Module => {
                let raw = request.module.as_deref().ok_or(BankError::Invalid)?;
                let normalized = primary_module(Some(raw)).ok_or(BankError::Invalid)?;
                if request.paper_id.is_some() || raw != normalized {
                    return Err(BankError::Invalid);
                }
                if request.category.as_ref().is_some_and(|category| {
                    category.trim() != category
                        || category.is_empty()
                        || category.len() > 128
                        || category.chars().any(char::is_control)
                }) {
                    return Err(BankError::Invalid);
                }
                Some(normalized.to_string())
            }
            PracticeMode::Wrong | PracticeMode::Favorite => {
                if request.paper_id.as_ref().is_some_and(|id| !valid_id(id))
                    || request.category.as_ref().is_some_and(|category| {
                        request.module.is_none()
                            || category.trim() != category
                            || category.is_empty()
                            || category.len() > 128
                            || category.chars().any(char::is_control)
                    })
                {
                    return Err(BankError::Invalid);
                }
                match request.module.as_deref() {
                    Some(raw) if primary_module(Some(raw)) == Some(raw) => Some(raw.to_string()),
                    Some(_) => return Err(BankError::Invalid),
                    None => None,
                }
            }
        };
        let selected_order = match request.mode {
            PracticeMode::Module => {
                let count = request.filters.count.unwrap_or(20);
                // Ask for extra groups so the caller can fill the requested
                // question count even when groups have different sizes.
                let groups = self
                    .repository
                    .practice_module_groups(
                        module.as_deref().ok_or(BankError::Invalid)?,
                        request.category.as_deref(),
                        &request.filters,
                        count.saturating_mul(4).max(8),
                    )
                    .await?;
                let mut selected = Vec::new();
                for group in groups {
                    if selected.len() >= count {
                        break;
                    }
                    selected.extend(group);
                }
                selected
            }
            PracticeMode::Wrong | PracticeMode::Favorite => self
                .repository
                .practice_browse(
                    request.mode.clone(),
                    request.paper_id.as_deref(),
                    module.as_deref(),
                    request.category.as_deref(),
                    &request.filters,
                )
                .await?
                .into_iter()
                .take(request.filters.count.unwrap_or(20))
                .map(|item| item.question_id)
                .collect(),
            _ => Vec::new(),
        };
        let mut items = Vec::new();
        if matches!(
            request.mode,
            PracticeMode::Module | PracticeMode::Wrong | PracticeMode::Favorite
        ) {
            let contents = self
                .repository
                .question_contents(&selected_order)
                .await?
                .into_iter()
                .map(|content| (content.question.id.clone(), content))
                .collect::<BTreeMap<_, _>>();
            for id in &selected_order {
                let content = contents
                    .get(id)
                    .filter(|content| content.active)
                    .ok_or(BankError::Unavailable)?;
                if !content.question.gradable {
                    continue;
                }
                items.push(practice_item(
                    &content.question,
                    &content.paper_id,
                    &content.source_hash,
                ));
            }
        } else {
            let paper = self
                .primary_indexed(request.paper_id.as_deref().ok_or(BankError::Invalid)?)
                .await?;
            let content = self.indexed_content(&paper).await?;
            for question in &content.questions {
                if !question.gradable {
                    continue;
                }
                if module.as_ref().is_some_and(|module| {
                    primary_module(question.module.as_deref()) != Some(module.as_str())
                }) {
                    continue;
                }
                items.push(practice_item(question, &paper.summary.id, &paper.hash));
            }
        }
        if items.is_empty() {
            return Err(BankError::NotFound);
        }
        let timestamp = now();
        let session = PracticeSession {
            id: uuid::Uuid::now_v7().to_string(),
            mode: request.mode,
            paper_id: request.paper_id,
            module,
            category: request.category,
            status: "active".into(),
            created_at: timestamp,
            updated_at: timestamp,
            elapsed_seconds: 0,
            paused: false,
            revision: 0,
            items,
            score: None,
        };
        self.repository.practice_create(session.clone()).await?;
        Ok(session)
    }

    pub async fn practice_history(&self) -> Result<Vec<PracticeSession>, BankError> {
        self.repository.practice_history().await
    }

    pub async fn practice_question_detail(
        &self,
        question_id: &str,
    ) -> Result<PracticeQuestionDetail, BankError> {
        if !valid_id(question_id) {
            return Err(BankError::Invalid);
        }
        let content = self
            .repository
            .question_contents(&[question_id.into()])
            .await?
            .into_iter()
            .find(|content| content.question.id == question_id && content.active)
            .ok_or(BankError::NotFound)?;
        let indexed = self.primary_indexed(&content.paper_id).await?;
        if indexed.hash != content.source_hash {
            return Err(BankError::Stale);
        }
        let question = content.question;
        let item = PracticeBrowseItem {
            question_id: question.id.clone(),
            paper_title: content.paper_title,
            paper_id: indexed.summary.id,
            year: indexed.summary.year,
            region: indexed.summary.region,
            number: question.number,
            module: question.module.clone(),
            category: question.category.clone(),
        };
        let (correct_count, wrong_count) = self
            .repository
            .practice_attempt_counts(&[question_id.to_owned()])
            .await?
            .into_iter()
            .find(|(id, _, _)| id == question_id)
            .map(|(_, correct, wrong)| (correct, wrong))
            .unwrap_or((0, 0));
        Ok(PracticeQuestionDetail {
            question: question.view(),
            correct_answer: question.correct_answer,
            explanation: question.explanation,
            materials: content.materials,
            item,
            correct_count,
            wrong_count,
        })
    }

    async fn session_contents(
        &self,
        session: &PracticeSession,
    ) -> Result<BTreeMap<String, PracticeContent>, BankError> {
        let ids = session
            .items
            .iter()
            .map(|item| item.question_id.clone())
            .collect::<Vec<_>>();
        Ok(self
            .repository
            .question_contents(&ids)
            .await?
            .into_iter()
            .map(|content| (content.question.id.clone(), content))
            .collect())
    }

    pub async fn practice_workspace(&self, id: &str) -> Result<PracticeWorkspace, BankError> {
        if !valid_id(id) {
            return Err(BankError::Invalid);
        }
        let session = self.repository.practice_load(id).await?;
        let contents = self.session_contents(&session).await?;
        let questions = session
            .items
            .iter()
            .map(|item| practice_question(item, contents.get(&item.question_id)))
            .collect();
        Ok(PracticeWorkspace { session, questions })
    }

    pub async fn save_practice_answer(
        &self,
        session_id: &str,
        question_id: &str,
        answer: Vec<String>,
    ) -> Result<PracticeSession, BankError> {
        if !valid_id(session_id)
            || !valid_id(question_id)
            || answer.len() > 12
            || answer
                .iter()
                .any(|a| a.len() != 1 || !a.as_bytes()[0].is_ascii_uppercase())
            || answer.iter().collect::<BTreeSet<_>>().len() != answer.len()
        {
            return Err(BankError::Invalid);
        }
        let mut session = self.repository.practice_load(session_id).await?;
        if session.status != "active" || session.paused {
            return Err(BankError::Conflict);
        }
        let item = session
            .items
            .iter_mut()
            .find(|q| q.question_id == question_id)
            .ok_or(BankError::NotFound)?;
        let contents = self
            .repository
            .question_contents(&[question_id.to_string()])
            .await?;
        let content = contents
            .first()
            .filter(|content| content.question.id == question_id)
            .ok_or(BankError::Unavailable)?;
        if !content.active || matching_content(item, Some(content)).is_none() {
            return Err(BankError::Stale);
        }
        let labels = super::document::option_labels(&content.question.options_markdown);
        if answer.iter().any(|choice| !labels.contains(choice)) {
            return Err(BankError::Invalid);
        }
        item.answer = answer;
        let previous_revision = session.revision;
        session.revision += 1;
        session.updated_at = now();
        self.repository
            .practice_save_answer(session.clone(), question_id, previous_revision)
            .await?;
        Ok(session)
    }

    pub async fn pause_practice(
        &self,
        id: &str,
        paused: bool,
        elapsed_seconds: u64,
    ) -> Result<PracticeSession, BankError> {
        if !valid_id(id) || elapsed_seconds > 60 * 60 * 24 * 7 {
            return Err(BankError::Invalid);
        }
        let mut session = self.repository.practice_load(id).await?;
        if session.status != "active" {
            return Err(BankError::Conflict);
        }
        session.paused = paused;
        session.elapsed_seconds = session.elapsed_seconds.max(elapsed_seconds);
        let previous_revision = session.revision;
        session.revision += 1;
        session.updated_at = now();
        self.repository
            .practice_update(session.clone(), previous_revision)
            .await?;
        Ok(session)
    }

    pub async fn submit_practice(&self, id: &str) -> Result<PracticeSession, BankError> {
        if !valid_id(id) {
            return Err(BankError::Invalid);
        }
        let mut session = self.repository.practice_load(id).await?;
        if session.status == "completed" {
            return Ok(session);
        }
        if session.status != "active" {
            return Err(BankError::Conflict);
        }
        let contents = self.session_contents(&session).await?;
        let mut score = PracticeScore {
            total: session.items.len(),
            correct: 0,
            wrong: 0,
            unanswered: 0,
            ungradable: 0,
        };
        for item in &mut session.items {
            let key = matching_content(item, contents.get(&item.question_id))
                .map(|content| &content.question);
            let grade = if !item.gradable || !key.is_some_and(|key| key.gradable) {
                score.ungradable += 1;
                "ungradable"
            } else if item.answer.is_empty() {
                score.unanswered += 1;
                "unanswered"
            } else {
                let mut answer = item.answer.clone();
                answer.sort();
                if key.is_some_and(|key| key.correct_answer == answer) {
                    score.correct += 1;
                    "correct"
                } else {
                    score.wrong += 1;
                    "wrong"
                }
            };
            item.grade = Some(grade.into());
        }
        session.status = "completed".into();
        session.paused = true;
        session.score = Some(score);
        session.updated_at = now();
        let previous_revision = session.revision;
        session.revision += 1;
        match self
            .repository
            .practice_update(session.clone(), previous_revision)
            .await
        {
            Ok(()) => Ok(session),
            Err(BankError::Conflict) => {
                let current = self.repository.practice_load(id).await?;
                if current.status == "completed" {
                    Ok(current)
                } else {
                    Err(BankError::Conflict)
                }
            }
            Err(error) => Err(error),
        }
    }

    pub async fn repeat_practice(&self, id: &str) -> Result<PracticeSession, BankError> {
        if !valid_id(id) {
            return Err(BankError::Invalid);
        }
        let source = self.repository.practice_load(id).await?;
        if source.status != "completed" {
            return Err(BankError::Conflict);
        }
        let ids = source
            .items
            .iter()
            .map(|item| item.question_id.clone())
            .collect::<Vec<_>>();
        let contents = self
            .repository
            .question_contents(&ids)
            .await?
            .into_iter()
            .map(|content| (content.question.id.clone(), content))
            .collect::<BTreeMap<_, _>>();
        let items = source
            .items
            .iter()
            .map(|item| {
                let content = contents
                    .get(&item.question_id)
                    .filter(|content| {
                        content.active && matching_content(item, Some(content)).is_some()
                    })
                    .ok_or(BankError::Unavailable)?;
                Ok(PracticeItem {
                    question_id: item.question_id.clone(),
                    paper_id: item.paper_id.clone(),
                    source_hash: item.source_hash.clone(),
                    number: item.number,
                    answer: Vec::new(),
                    grade: None,
                    gradable: content.question.gradable,
                })
            })
            .collect::<Result<Vec<_>, BankError>>()?;
        let timestamp = now();
        let session = PracticeSession {
            id: uuid::Uuid::now_v7().to_string(),
            mode: source.mode,
            paper_id: source.paper_id,
            module: source.module,
            category: source.category,
            status: "active".into(),
            created_at: timestamp,
            updated_at: timestamp,
            elapsed_seconds: 0,
            paused: false,
            revision: 0,
            items,
            score: None,
        };
        self.repository.practice_create(session.clone()).await?;
        Ok(session)
    }

    /// Server-only Codex locator: never returns the source answer or explanation.
    pub async fn ai_review_locator(
        &self,
        session_id: &str,
        question_id: &str,
    ) -> Result<AiReviewLocator, BankError> {
        if !valid_id(question_id) {
            return Err(BankError::Invalid);
        }
        let review = self.practice_review(session_id).await?;
        let entry = review
            .questions
            .iter()
            .find(|entry| entry.question.item.question_id == question_id)
            .ok_or(BankError::NotFound)?;
        if entry.question.content_changed {
            return Err(BankError::Stale);
        }
        let item = &entry.question.item;
        let indexed = self.primary_indexed(&item.paper_id).await?;
        if indexed.hash != item.source_hash {
            return Err(BankError::Stale);
        }
        let question = entry.question.question.as_ref().ok_or(BankError::Stale)?;
        Ok(AiReviewLocator {
            session_id: session_id.to_owned(),
            question_id: question_id.to_owned(),
            paper_id: item.paper_id.clone(),
            material_ids: question.materials.clone(),
            source_hash: item.source_hash.clone(),
            user_answer: item.answer.clone(),
        })
    }

    pub async fn practice_review(&self, id: &str) -> Result<PracticeReview, BankError> {
        if !valid_id(id) {
            return Err(BankError::Invalid);
        }
        let session = self.repository.practice_load(id).await?;
        if session.status != "completed" {
            return Err(BankError::Conflict);
        }
        let contents = self.session_contents(&session).await?;
        let ids = session
            .items
            .iter()
            .map(|item| item.question_id.clone())
            .collect::<Vec<_>>();
        let counts = self
            .repository
            .practice_attempt_counts(&ids)
            .await?
            .into_iter()
            .map(|(id, correct, wrong)| (id, (correct, wrong)))
            .collect::<BTreeMap<_, _>>();
        let questions = session
            .items
            .iter()
            .map(|item| {
                let content = contents.get(&item.question_id);
                let key = matching_content(item, content).map(|content| &content.question);
                let (correct_count, wrong_count) =
                    counts.get(&item.question_id).copied().unwrap_or((0, 0));
                ReviewQuestion {
                    question: practice_question(item, content),
                    correct_answer: key
                        .map(|key| key.correct_answer.clone())
                        .unwrap_or_default(),
                    explanation: key.map(|key| key.explanation.clone()).unwrap_or_default(),
                    correct_count,
                    wrong_count,
                }
            })
            .collect();
        Ok(PracticeReview { session, questions })
    }

    pub async fn set_favorite(
        &self,
        question_id: &str,
        favorite: bool,
    ) -> Result<FavoriteState, BankError> {
        if !valid_id(question_id) {
            return Err(BankError::Invalid);
        }
        self.repository
            .practice_favorite(question_id, favorite)
            .await?;
        Ok(FavoriteState {
            question_id: question_id.into(),
            favorite,
        })
    }

    pub async fn favorite(&self, question_id: &str) -> Result<FavoriteState, BankError> {
        if !valid_id(question_id) {
            return Err(BankError::Invalid);
        }
        Ok(FavoriteState {
            question_id: question_id.into(),
            favorite: self.repository.practice_is_favorite(question_id).await?,
        })
    }
}

fn matching_content<'a>(
    item: &PracticeItem,
    content: Option<&'a PracticeContent>,
) -> Option<&'a PracticeContent> {
    content.filter(|content| {
        content.paper_id == item.paper_id && content.source_hash == item.source_hash
    })
}

fn practice_question(item: &PracticeItem, content: Option<&PracticeContent>) -> PracticeQuestion {
    let matching = matching_content(item, content);
    PracticeQuestion {
        item: item.clone(),
        paper_title: content
            .map(|content| content.paper_title.clone())
            .unwrap_or_else(|| "原试卷".into()),
        question: matching.map(|content| content.question.view()),
        materials: matching
            .map(|content| content.materials.clone())
            .unwrap_or_default(),
        content_changed: matching.is_none(),
    }
}

fn practice_item(question: &super::StoredQuestion, paper_id: &str, hash: &str) -> PracticeItem {
    PracticeItem {
        question_id: question.id.clone(),
        paper_id: paper_id.into(),
        source_hash: hash.into(),
        number: question.number,
        answer: Vec::new(),
        grade: None,
        gradable: question.gradable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::question_bank::document::Document;
    use crate::application::question_bank::{
        BankAsset, BankSnapshot, QuestionBankRepositoryPort, QuestionBankSourcePort,
    };
    use crate::domain::question_bank::PaperSummary;
    use async_trait::async_trait;
    use std::sync::Arc;

    struct NoSource;
    #[async_trait]
    impl QuestionBankSourcePort for NoSource {
        async fn scan(&self, _: Vec<IndexedPaper>) -> Result<BankSnapshot, BankError> {
            unimplemented!()
        }
        async fn load(&self, _: IndexedPaper) -> Result<Document, BankError> {
            unimplemented!()
        }
        async fn asset(&self, _: String) -> Result<BankAsset, BankError> {
            unimplemented!()
        }
    }

    struct CountsOnly(IndexedPaper);
    #[async_trait]
    impl QuestionBankRepositoryPort for CountsOnly {
        async fn list(&self) -> Result<Vec<IndexedPaper>, BankError> {
            Ok(vec![self.0.clone()])
        }
        async fn apply(&self, _: Vec<IndexedPaper>) -> Result<(), BankError> {
            unimplemented!()
        }
        async fn indexed_paper(&self, id: &str) -> Result<IndexedPaper, BankError> {
            (self.0.summary.id == id)
                .then(|| self.0.clone())
                .ok_or(BankError::NotFound)
        }
        async fn paper_content(&self, _: &str) -> Result<PaperContent, BankError> {
            panic!("module counts must aggregate without loading question content")
        }
        async fn paper_module_counts(
            &self,
            _: &str,
            _: &str,
        ) -> Result<Vec<(Option<String>, usize, usize)>, BankError> {
            Ok(vec![(Some("资料分析".into()), 2, 1)])
        }
    }

    #[tokio::test]
    async fn modules_read_aggregate_counts_without_loading_question_content() {
        let paper = IndexedPaper {
            summary: PaperSummary {
                id: "019a3d8c-7f23-7b6e-927d-3c951851d3a4".into(),
                title: "样卷".into(),
                year: 2020,
                region: "国考".into(),
                variant: "统考".into(),
                role: "primary".into(),
                questions: 3,
            },
            path: "papers/a.md".into(),
            hash: "hash".into(),
            ids: Vec::new(),
            question_ids: Vec::new(),
            material_ids: Vec::new(),
            question_numbers: Vec::new(),
            question_modules: Vec::new(),
            question_categories: Vec::new(),
            content: PaperContent::default(),
        };
        let app = QuestionBankApplication::new(Arc::new(NoSource), Arc::new(CountsOnly(paper)));
        let modules = app
            .modules("019a3d8c-7f23-7b6e-927d-3c951851d3a4")
            .await
            .unwrap();
        assert_eq!(modules.modules.len(), 1);
        assert_eq!(modules.modules[0].name, "资料分析");
        assert_eq!(modules.modules[0].count, 2);
        assert_eq!(modules.modules[0].ungradable, 1);
    }
}
