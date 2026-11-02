//! Question Bank practice use cases. Never return answer keys before submission.
use super::{
    document::valid_id, document::Document, BankError, IndexedPaper, QuestionBankApplication,
};
use crate::domain::question_bank::{
    CreatePracticeRequest, FavoriteState, ModulePracticeFilters, PaperModule, PaperModules, PracticeCatalog, PracticeItem,
    PracticeMode, PracticeQuestion, PracticeReview, PracticeScore, PracticeSession,
    PracticeWorkspace, ReviewQuestion,
};
use std::collections::{BTreeMap, BTreeSet};

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Only one-way, unambiguous mappings are allowed; don't infer from category/topics.
pub fn primary_module(name: Option<&str>) -> Option<&'static str> {
    match name?.trim() {
        "政治理论" => Some("政治理论"),
        "常识判断" => Some("常识判断"),
        "言语理解与表达" | "言语理解" | "言语" => Some("言语理解与表达"),
        "判断推理" | "图形推理" | "定义判断" | "类比推理" | "逻辑判断" | "形式推理" => {
            Some("判断推理")
        }
        "数量关系" | "数量" => Some("数量关系"),
        "资料分析" | "资料" => Some("资料分析"),
        _ => None,
    }
}

fn validate_filters(filters: &ModulePracticeFilters) -> Result<(), BankError> {
    if filters.year_min.is_some_and(|year| !(1900..=2100).contains(&year))
        || filters.year_max.is_some_and(|year| !(1900..=2100).contains(&year))
        || matches!((filters.year_min, filters.year_max), (Some(min), Some(max)) if min > max)
        || filters.count.is_some_and(|count| count == 0 || count > 20)
        || filters.regions.len() > 64
        || filters.regions.iter().any(|region| {
            region.trim() != region || region.is_empty() || region.len() > 100
                || region.chars().any(char::is_control)
        })
        || filters.regions.iter().collect::<BTreeSet<_>>().len() != filters.regions.len()
    {
        return Err(BankError::Invalid);
    }
    Ok(())
}

impl QuestionBankApplication {
    async fn primary_indexed(&self, id: &str) -> Result<IndexedPaper, BankError> {
        if !valid_id(id) {
            return Err(BankError::Invalid);
        }
        self.repository
            .list()
            .await?
            .into_iter()
            .find(|paper| paper.summary.id == id && paper.summary.role == "primary")
            .ok_or(BankError::NotFound)
    }

    async fn primary_document(&self, id: &str) -> Result<(IndexedPaper, Document), BankError> {
        let indexed = self.primary_indexed(id).await?;
        let doc = self.source.load(indexed.clone()).await?;
        if doc.summary().id != indexed.summary.id {
            return Err(BankError::Conflict);
        }
        Ok((indexed, doc))
    }

    pub async fn modules(&self, paper_id: &str) -> Result<PaperModules, BankError> {
        let (_, doc) = self.primary_document(paper_id).await?;
        let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut unclassified = 0;
        for q in doc.view().questions {
            if let Some(module) = primary_module(q.module.as_deref()) {
                let count = counts.entry(module.into()).or_default();
                count.0 += 1;
                if !doc.key(&q.id).is_some_and(|key| key.valid) {
                    count.1 += 1;
                }
            } else {
                unclassified += 1;
            }
        }
        Ok(PaperModules {
            modules: counts
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

    pub async fn practice_catalog(&self, filters: &ModulePracticeFilters) -> Result<PracticeCatalog, BankError> {
        validate_filters(filters)?;
        self.repository.practice_catalog(filters).await
    }

    pub async fn create_practice(
        &self,
        request: CreatePracticeRequest,
    ) -> Result<PracticeSession, BankError> {
        validate_filters(&request.filters)?;
        if request.mode != PracticeMode::Module && request.filters != ModulePracticeFilters::default() {
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
                if request.module.is_some()
                    || request.paper_id.is_some()
                    || request.category.is_some()
                {
                    return Err(BankError::Invalid);
                }
                None
            }
        };
        let selected_order = match request.mode {
            PracticeMode::Module => {
                self.repository
                    .practice_module_candidates(
                        module.as_deref().ok_or(BankError::Invalid)?,
                        request.category.as_deref(),
                        &request.filters,
                        request.filters.count.unwrap_or(20),
                    )
                    .await?
            }
            PracticeMode::Wrong | PracticeMode::Favorite => {
                self.repository
                    .practice_candidates(request.mode.clone())
                    .await?
            }
            _ => Vec::new(),
        };
        let selected = match request.mode {
            PracticeMode::Module | PracticeMode::Wrong | PracticeMode::Favorite => {
                Some(selected_order.iter().cloned().collect::<BTreeSet<_>>())
            }
            _ => None,
        };
        let indexed = match request.paper_id.as_deref() {
            Some(id) => vec![self.primary_indexed(id).await?],
            None => self
                .repository
                .list()
                .await?
                .into_iter()
                .filter(|p| p.summary.role == "primary")
                .collect(),
        };
        let mut items = Vec::new();
        for paper in indexed {
            if selected
                .as_ref()
                .is_some_and(|ids| !paper.question_ids.iter().any(|id| ids.contains(id)))
            {
                continue;
            }
            let doc = self.source.load(paper.clone()).await?;
            if doc.summary().id != paper.summary.id {
                return Err(BankError::Conflict);
            }
            for question in doc.view().questions {
                if let Some(module) = module.as_ref() {
                    if primary_module(question.module.as_deref()) != Some(module.as_str()) {
                        continue;
                    }
                }
                if selected
                    .as_ref()
                    .is_some_and(|ids| !ids.contains(&question.id))
                {
                    continue;
                }
                items.push(PracticeItem {
                    question_id: question.id.clone(),
                    paper_id: paper.summary.id.clone(),
                    source_hash: paper.hash.clone(),
                    number: question.number,
                    answer: Vec::new(),
                    grade: None,
                    gradable: doc.key(&question.id).is_some_and(|key| key.valid),
                });
            }
        }
        if items.is_empty() {
            return Err(BankError::NotFound);
        }
        if request.mode == PracticeMode::Module {
            // Keep the repository's unpracticed-first, deterministic twenty-question order.
            items.sort_by_key(|item| {
                selected_order
                    .iter()
                    .position(|id| id == &item.question_id)
                    .unwrap_or(usize::MAX)
            });
        }
        // Original-paper and original-paper-module modes never sample or truncate.
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

    pub async fn practice_workspace(&self, id: &str) -> Result<PracticeWorkspace, BankError> {
        if !valid_id(id) {
            return Err(BankError::Invalid);
        }
        let session = self.repository.practice_load(id).await?;
        let indexed = self.repository.known().await?;
        let mut cache: BTreeMap<String, Option<Document>> = BTreeMap::new();
        let mut questions = Vec::new();
        for item in &session.items {
            let paper = indexed.iter().find(|p| p.summary.id == item.paper_id);
            let title = paper
                .map(|p| p.summary.title.clone())
                .unwrap_or_else(|| "原试卷".into());
            let matching = paper.filter(|p| p.hash == item.source_hash);
            if let Some(paper) = matching {
                if !cache.contains_key(&item.paper_id) {
                    let doc = self
                        .source
                        .load(paper.clone())
                        .await
                        .ok()
                        .filter(|doc| doc.summary().id == item.paper_id);
                    cache.insert(item.paper_id.clone(), doc);
                }
            }
            let doc = cache
                .get(&item.paper_id)
                .and_then(|v| v.as_ref())
                .filter(|_| matching.is_some());
            let question = doc.and_then(|doc| {
                doc.view()
                    .questions
                    .into_iter()
                    .find(|q| q.id == item.question_id)
            });
            let materials = doc
                .map(|doc| {
                    doc.view()
                        .materials
                        .into_iter()
                        .filter(|m| {
                            question
                                .as_ref()
                                .is_some_and(|q| q.materials.contains(&m.id))
                        })
                        .collect()
                })
                .unwrap_or_default();
            questions.push(PracticeQuestion {
                item: item.clone(),
                paper_title: title,
                content_changed: question.is_none(),
                question,
                materials,
            });
        }
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
        let indexed = self.primary_indexed(&item.paper_id).await?;
        if indexed.hash != item.source_hash {
            return Err(BankError::Stale);
        }
        let doc = self.source.load(indexed).await?;
        let question = doc
            .view()
            .questions
            .into_iter()
            .find(|q| q.id == question_id)
            .ok_or(BankError::NotFound)?;
        let labels = super::document::option_labels(&question.options_markdown);
        if answer.iter().any(|choice| !labels.contains(choice)) {
            return Err(BankError::Invalid);
        }
        item.answer = answer;
        let previous_revision = session.revision;
        session.revision += 1;
        session.updated_at = now();
        self.repository
            .practice_update(session.clone(), previous_revision)
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
        let indexed = self.repository.known().await?;
        let mut docs: BTreeMap<String, Option<Document>> = BTreeMap::new();
        let mut score = PracticeScore {
            total: session.items.len(),
            correct: 0,
            wrong: 0,
            unanswered: 0,
            ungradable: 0,
        };
        for item in &mut session.items {
            if !docs.contains_key(&item.paper_id) {
                let doc = match indexed
                    .iter()
                    .find(|p| p.summary.id == item.paper_id && p.hash == item.source_hash)
                {
                    Some(p) => Some(self.source.load(p.clone()).await?),
                    None => None,
                };
                docs.insert(item.paper_id.clone(), doc);
            }
            let key = docs
                .get(&item.paper_id)
                .and_then(|v| v.as_ref())
                .and_then(|doc| doc.key(&item.question_id));
            let grade = if !item.gradable || !key.is_some_and(|k| k.valid) {
                score.ungradable += 1;
                "ungradable"
            } else if item.answer.is_empty() {
                score.unanswered += 1;
                "unanswered"
            } else {
                let mut answer = item.answer.clone();
                answer.sort();
                if key.is_some_and(|key| key.choices == answer) {
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

    pub async fn practice_review(&self, id: &str) -> Result<PracticeReview, BankError> {
        let workspace = self.practice_workspace(id).await?;
        if workspace.session.status != "completed" {
            return Err(BankError::Conflict);
        }
        let indexed = self.repository.known().await?;
        let mut cache: BTreeMap<String, Option<Document>> = BTreeMap::new();
        let mut questions = Vec::new();
        for q in workspace.questions {
            if !cache.contains_key(&q.item.paper_id) {
                let doc = match indexed
                    .iter()
                    .find(|p| p.summary.id == q.item.paper_id && p.hash == q.item.source_hash)
                {
                    Some(p) => self.source.load(p.clone()).await.ok(),
                    None => None,
                };
                cache.insert(q.item.paper_id.clone(), doc);
            }
            let key = cache
                .get(&q.item.paper_id)
                .and_then(|v| v.as_ref())
                .filter(|_| !q.content_changed)
                .and_then(|doc| doc.key(&q.item.question_id));
            questions.push(ReviewQuestion {
                question: q,
                correct_answer: key.map(|k| k.choices.clone()).unwrap_or_default(),
                explanation: key.map(|k| k.explanation.clone()).unwrap_or_default(),
            });
        }
        Ok(PracticeReview {
            session: workspace.session,
            questions,
        })
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
