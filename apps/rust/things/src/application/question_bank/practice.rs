//! Question Bank practice use cases. Never return answer keys before submission.
use super::{
    document::valid_id, BankError, DrillPaperRow, IndexedPaper, PaperContent, PracticeContent,
    QuestionBankApplication, StudyAnswerDay,
};
use crate::domain::question_bank::{
    primary_module, CodexLimitWindow, CodexLimits, CreatePracticeRequest, DrillCollection,
    DrillPaper, FavoriteState, ModulePracticeFilters, PaperModule, PaperModules,
    PracticeBrowseItem, PracticeCatalog, PracticeItem, PracticeMode, PracticeQuestion,
    PracticeQuestionDetail, PracticeReview, PracticeScore, PracticeSession, PracticeWorkspace,
    ReviewQuestion, StudyPlan, StudyPlanForecast, StudyPlanForecastItem,
};
use chrono::{Datelike, Duration, NaiveDate};
use std::collections::{BTreeMap, BTreeSet};

#[async_trait::async_trait]
pub trait AiReviewBackend: Send + Sync {
    async fn rate_limits(&self) -> Result<CodexLimits, String> {
        Err("Codex 限额不可用".into())
    }

    async fn connect(&self, previous_thread: Option<&str>)
        -> Result<Box<dyn AiReviewTurn>, String>;
}

/// Extract only the standard Codex bucket; unknown windows stay unavailable.
pub fn codex_limits_from_response(value: &serde_json::Value) -> CodexLimits {
    let bucket = value
        .pointer("/rateLimitsByLimitId/codex")
        .or_else(|| value.get("rateLimits"));
    let mut limits = CodexLimits::default();
    if let Some(bucket) = bucket {
        for name in ["primary", "secondary"] {
            let Some(window) = bucket.get(name) else {
                continue;
            };
            let Some(duration) = window.get("windowDurationMins").and_then(|v| v.as_u64()) else {
                continue;
            };
            let Some(used) = window
                .get("usedPercent")
                .and_then(|v| v.as_f64())
                .filter(|v| v.is_finite() && (0.0..=100.0).contains(v))
            else {
                continue;
            };
            let limit = CodexLimitWindow {
                remaining_percent: (100.0 - used).floor() as u8,
                resets_at: window
                    .get("resetsAt")
                    .and_then(|v| v.as_i64())
                    .filter(|v| *v > 0),
            };
            match duration {
                300 => limits.five_hour = Some(limit),
                10080 => limits.week = Some(limit),
                _ => {}
            }
        }
    }
    limits
}

#[cfg(test)]
mod codex_limits_tests {
    use super::*;
    #[test]
    fn reads_standard_bucket_by_window_duration() {
        let response = serde_json::json!({"rateLimitsByLimitId":{"codex":{
            "primary":{"usedPercent":62,"windowDurationMins":10080,"resetsAt":2000},
            "secondary":{"usedPercent":28,"windowDurationMins":300,"resetsAt":1000}
        }}});
        let result = codex_limits_from_response(&response);
        assert_eq!(result.five_hour.unwrap().remaining_percent, 72);
        assert_eq!(result.week.unwrap().remaining_percent, 38);
    }
    #[test]
    fn rejects_unknown_and_invalid_windows() {
        let response = serde_json::json!({"rateLimits":{
            "primary":{"usedPercent":101,"windowDurationMins":300},
            "secondary":{"usedPercent":10,"windowDurationMins":15}
        }});
        assert_eq!(
            codex_limits_from_response(&response),
            CodexLimits::default()
        );
    }
}

#[async_trait::async_trait]
pub trait AiReviewTurn: Send {
    fn thread_id(&self) -> &str;
    async fn turn(
        &mut self,
        prompt: &str,
        images: &[String],
        cancellation: tokio::sync::watch::Sender<bool>,
    ) -> Result<String, String>;
}

/// Forwards validated temporary images to the PC-side receiver. The returned
/// path is an absolute path on the Codex host, never a browser-supplied value.
#[async_trait::async_trait]
pub trait AiAttachmentRelay: Send + Sync {
    async fn upload(&self, media_type: &str, bytes: &[u8]) -> Result<String, String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuestionAiLocator {
    pub question_id: String,
    pub paper_id: String,
    pub paper_path: String,
    pub material_ids: Vec<String>,
    pub stem_fingerprint: String,
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

fn normalize_study_plan(mut plan: StudyPlan) -> Result<StudyPlan, BankError> {
    if plan.items.len() > 100 {
        return Err(BankError::Invalid);
    }
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();
    for item in plan.items {
        if primary_module(Some(&item.module)) != Some(item.module.as_str()) {
            return Err(BankError::Invalid);
        }
        if let Some(category) = &item.category {
            if category.is_empty()
                || category.trim() != category
                || category.len() > 128
                || category.chars().any(char::is_control)
            {
                return Err(BankError::Invalid);
            }
        }
        if seen.insert((item.module.clone(), item.category.clone())) {
            normalized.push(item);
        }
    }
    let parents: BTreeSet<String> = normalized
        .iter()
        .filter(|item| item.category.is_none())
        .map(|item| item.module.clone())
        .collect();
    normalized.retain(|item| item.category.is_none() || !parents.contains(&item.module));
    normalized.sort_by(|a, b| {
        a.module
            .cmp(&b.module)
            .then_with(|| a.category.cmp(&b.category))
    });
    plan.items = normalized;
    Ok(plan)
}

fn valid_collection_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn beijing_day_start_ms(date: NaiveDate) -> i64 {
    let offset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    date.and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(offset)
        .single()
        .unwrap()
        .timestamp_millis()
}

fn estimate_days(remaining: i64, answered: i64, window: i64) -> Option<i64> {
    if remaining <= 0 {
        return Some(0);
    }
    if answered <= 0 {
        return None;
    }
    Some((remaining.saturating_mul(window) + answered - 1) / answered)
}

fn finish_date(today: NaiveDate, days: Option<i64>) -> Option<String> {
    days.map(|days| {
        let date = today + Duration::days(days);
        format!("{}/{}", date.month(), date.day())
    })
}

fn study_filter_label(filters: &ModulePracticeFilters) -> String {
    let years = match (filters.year_min, filters.year_max) {
        (Some(min), Some(max)) if min == max => format!("{min} 年"),
        (Some(min), Some(max)) => format!("{min}–{max} 年"),
        (Some(min), None) => format!("{min} 年及以后"),
        (None, Some(max)) => format!("{max} 年及以前"),
        (None, None) => "不限年份".into(),
    };
    let regions = if filters.regions.is_empty() {
        "不限地区".into()
    } else {
        format!(
            "{}：{}",
            if filters.exclude_regions {
                "排除"
            } else {
                "仅限"
            },
            filters.regions.join("、")
        )
    };
    format!("{years} · {regions}")
}

fn build_study_plan_forecast(
    plan: &StudyPlan,
    catalog: &PracticeCatalog,
    answers: &[StudyAnswerDay],
    filters: &ModulePracticeFilters,
    today: NaiveDate,
) -> StudyPlanForecast {
    let day_today = today.format("%Y-%m-%d").to_string();
    let day_yesterday = (today - Duration::days(1)).format("%Y-%m-%d").to_string();
    let day_7 = (today - Duration::days(7)).format("%Y-%m-%d").to_string();
    let mut items = Vec::new();
    for planned in &plan.items {
        let module = catalog
            .modules
            .iter()
            .find(|module| module.name == planned.module);
        let (total, done) = match (&planned.category, module) {
            (Some(category), Some(module)) => module
                .categories
                .iter()
                .find(|entry| &entry.name == category)
                .map(|entry| (entry.count as i64, entry.done as i64))
                .unwrap_or((0, 0)),
            (None, Some(module)) => (module.count as i64, module.done as i64),
            _ => (0, 0),
        };
        let remaining = (total - done).max(0);
        let mut answered_today = 0;
        let mut answered_yesterday = 0;
        let mut answered_7 = 0;
        let mut active_days = BTreeSet::new();
        for row in answers {
            if row.module != planned.module {
                continue;
            }
            if planned
                .category
                .as_ref()
                .is_some_and(|category| &row.category != category)
            {
                continue;
            }
            // The averages use complete days, so today never dilutes them.
            if row.day >= day_7 && row.day < day_today {
                answered_7 += row.answered;
                if row.answered > 0 {
                    active_days.insert(row.day.clone());
                }
            }
            if row.day == day_yesterday {
                answered_yesterday += row.answered;
            }
            if row.day == day_today {
                answered_today += row.answered;
            }
        }
        let low_confidence = answered_7 < 20 || active_days.len() < 3;
        let days_yesterday = estimate_days(remaining, answered_yesterday, 1);
        let days_7 = estimate_days(remaining, answered_7, 7);
        items.push(StudyPlanForecastItem {
            module: planned.module.clone(),
            category: planned.category.clone(),
            total,
            done,
            remaining,
            answered_today,
            answered_yesterday,
            answered_7,
            days_yesterday,
            finish_yesterday: finish_date(today, days_yesterday),
            days_7,
            finish_7: finish_date(today, days_7),
            low_confidence,
        });
    }
    let total_remaining: i64 = items.iter().map(|item| item.remaining).sum();
    let answered_yesterday: i64 = items.iter().map(|item| item.answered_yesterday).sum();
    let answered_7: i64 = items.iter().map(|item| item.answered_7).sum();
    let stalled_yesterday = items
        .iter()
        .any(|item| item.remaining > 0 && item.answered_yesterday == 0);
    let stalled_7 = items
        .iter()
        .any(|item| item.remaining > 0 && item.answered_7 == 0);
    let days_yesterday = if total_remaining == 0 {
        Some(0)
    } else if stalled_yesterday {
        None
    } else {
        estimate_days(total_remaining, answered_yesterday, 1)
    };
    let days_7 = if total_remaining == 0 {
        Some(0)
    } else if stalled_7 {
        None
    } else {
        estimate_days(total_remaining, answered_7, 7)
    };
    StudyPlanForecast {
        prediction_enabled: plan.prediction_enabled,
        filter_label: study_filter_label(filters),
        low_confidence: items.iter().any(|item| item.low_confidence),
        finish_yesterday: finish_date(today, days_yesterday),
        finish_7: finish_date(today, days_7),
        items,
        total_remaining,
        days_yesterday,
        days_7,
    }
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

    /// Drill collections are separate source libraries. Their progress is
    /// reported independently from the exam forecast so lecture questions
    /// never change the remaining exam workload.
    pub async fn drill_collections(&self) -> Result<Vec<DrillCollection>, BankError> {
        let rows = self.repository.drill_paper_stats().await?;
        let mut grouped = BTreeMap::<String, Vec<DrillPaperRow>>::new();
        for row in rows {
            grouped
                .entry(row.collection_id.clone())
                .or_default()
                .push(row);
        }
        Ok(grouped
            .into_iter()
            .filter_map(|(id, rows)| {
                let first = rows.first()?;
                let title = first.collection_title.clone();
                let year = first.year;
                let questions = rows.iter().map(|row| row.questions).sum::<usize>();
                let gradable = rows.iter().map(|row| row.gradable).sum::<usize>();
                let done = rows.iter().map(|row| row.done).sum::<usize>();
                let module = rows.iter().find_map(|row| row.module.clone());
                Some(DrillCollection {
                    id,
                    title,
                    module,
                    year,
                    questions,
                    gradable,
                    done,
                    papers: rows
                        .into_iter()
                        .map(|row| DrillPaper {
                            paper_id: row.paper_id,
                            title: row.title,
                            questions: row.questions,
                            gradable: row.gradable,
                            done: row.done,
                        })
                        .collect(),
                })
            })
            .collect())
    }

    pub async fn module_settings(&self) -> Result<ModulePracticeFilters, BankError> {
        self.repository.module_settings().await
    }

    pub async fn save_module_settings(
        &self,
        filters: ModulePracticeFilters,
    ) -> Result<ModulePracticeFilters, BankError> {
        validate_filters(&filters)?;
        self.repository.save_module_settings(&filters).await?;
        Ok(filters)
    }

    pub async fn study_plan(&self) -> Result<StudyPlan, BankError> {
        self.repository.study_plan().await
    }

    pub async fn save_study_plan(&self, plan: StudyPlan) -> Result<StudyPlan, BankError> {
        let plan = normalize_study_plan(plan)?;
        self.repository.save_study_plan(&plan).await?;
        Ok(plan)
    }

    pub async fn study_plan_forecast(
        &self,
        filters: &ModulePracticeFilters,
    ) -> Result<StudyPlanForecast, BankError> {
        validate_filters(filters)?;
        let plan = self.repository.study_plan().await?;
        let catalog = self.repository.practice_catalog(filters).await?;
        let offset = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let today = chrono::Utc::now().with_timezone(&offset).date_naive();
        let start = beijing_day_start_ms(today - Duration::days(7));
        let end = beijing_day_start_ms(today + Duration::days(1));
        let answers = self
            .repository
            .study_answered_days(filters, start, end)
            .await?;
        Ok(build_study_plan_forecast(
            &plan, &catalog, &answers, filters, today,
        ))
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
        if matches!(mode, PracticeMode::PaperModule | PracticeMode::Drill)
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
            PracticeMode::Drill => {
                let collection = request.collection_id.as_deref().ok_or(BankError::Invalid)?;
                if !valid_collection_id(collection)
                    || request.paper_id.as_ref().is_some_and(|id| !valid_id(id))
                    || request.module.is_some()
                    || request.category.is_some()
                    || request.filters != ModulePracticeFilters::default()
                {
                    return Err(BankError::Invalid);
                }
                None
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
            PracticeMode::Drill => {
                self.repository
                    .drill_question_ids(
                        request.collection_id.as_deref().ok_or(BankError::Invalid)?,
                        request.paper_id.as_deref(),
                    )
                    .await?
            }
            _ => Vec::new(),
        };
        let mut items = Vec::new();
        if matches!(
            request.mode,
            PracticeMode::Module
                | PracticeMode::Wrong
                | PracticeMode::Favorite
                | PracticeMode::Drill
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
                if !content.question.gradable
                    && !matches!(request.mode, PracticeMode::Drill | PracticeMode::Favorite)
                {
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
        let (collection_id, collection_title, drill_module) = if request.mode == PracticeMode::Drill
        {
            let collection = request.collection_id.clone();
            let rows = self.repository.drill_paper_stats().await?;
            let row = rows
                .iter()
                .find(|row| {
                    Some(row.collection_id.as_str()) == collection.as_deref()
                        && request
                            .paper_id
                            .as_deref()
                            .is_none_or(|paper_id| row.paper_id == paper_id)
                })
                .ok_or(BankError::NotFound)?;
            (
                collection,
                Some(row.collection_title.clone()),
                row.module.clone(),
            )
        } else {
            (None, None, None)
        };
        let timestamp = now();
        let session = PracticeSession {
            id: uuid::Uuid::now_v7().to_string(),
            mode: request.mode,
            paper_id: request.paper_id,
            module: module.or(drill_module),
            category: request.category,
            collection_id,
            collection_title,
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

    async fn active_question_content(
        &self,
        question_id: &str,
    ) -> Result<(PracticeContent, IndexedPaper), BankError> {
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
        Ok((content, indexed))
    }

    pub async fn practice_question_detail(
        &self,
        question_id: &str,
    ) -> Result<PracticeQuestionDetail, BankError> {
        let (content, indexed) = self.active_question_content(question_id).await?;
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
            collection_id: source.collection_id,
            collection_title: source.collection_title,
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
    pub async fn question_ai_locator(
        &self,
        question_id: &str,
    ) -> Result<QuestionAiLocator, BankError> {
        let (content, indexed) = self.active_question_content(question_id).await?;
        let stem_fingerprint = self
            .stem_fingerprint(&indexed.path, &content.question.stem)
            .await?
            .ok_or(BankError::Stale)?;
        Ok(QuestionAiLocator {
            question_id: question_id.to_owned(),
            paper_id: content.paper_id,
            paper_path: indexed.path,
            material_ids: content.question.materials,
            stem_fingerprint,
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
    use crate::domain::question_bank::{
        PaperSummary, PracticeCategory, PracticeModule, StudyPlanItem,
    };
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
                subject: "xingce".into(),
                role: "primary".into(),
                kind: "exam".into(),
                collection_id: None,
                collection_title: None,
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
            content_fingerprints: Default::default(),
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

    fn plan_catalog() -> PracticeCatalog {
        PracticeCatalog {
            modules: vec![PracticeModule {
                name: "资料分析".into(),
                count: 100,
                done: 40,
                uncategorized: 0,
                categories: vec![
                    PracticeCategory {
                        name: "比重".into(),
                        count: 60,
                        done: 30,
                    },
                    PracticeCategory {
                        name: "增长".into(),
                        count: 40,
                        done: 10,
                    },
                ],
            }],
            unclassified: 0,
        }
    }

    fn plan_answers(today: NaiveDate) -> Vec<StudyAnswerDay> {
        let day = |offset| {
            (today + Duration::days(offset))
                .format("%Y-%m-%d")
                .to_string()
        };
        vec![
            StudyAnswerDay {
                day: day(-3),
                module: "资料分析".into(),
                category: "比重".into(),
                answered: 10,
            },
            StudyAnswerDay {
                day: day(-3),
                module: "资料分析".into(),
                category: "增长".into(),
                answered: 5,
            },
            StudyAnswerDay {
                day: day(-2),
                module: "资料分析".into(),
                category: "比重".into(),
                answered: 10,
            },
            StudyAnswerDay {
                day: day(-1),
                module: "资料分析".into(),
                category: "比重".into(),
                answered: 10,
            },
            StudyAnswerDay {
                day: day(0),
                module: "资料分析".into(),
                category: "比重".into(),
                answered: 10,
            },
            StudyAnswerDay {
                day: day(-2),
                module: "资料分析".into(),
                category: "增长".into(),
                answered: 5,
            },
            StudyAnswerDay {
                day: day(-1),
                module: "资料分析".into(),
                category: "增长".into(),
                answered: 5,
            },
            StudyAnswerDay {
                day: day(0),
                module: "资料分析".into(),
                category: "增长".into(),
                answered: 5,
            },
        ]
    }

    #[test]
    fn parent_plan_aggregates_categories_and_estimates_days() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 10).unwrap();
        let plan = StudyPlan {
            prediction_enabled: true,
            items: vec![StudyPlanItem {
                module: "资料分析".into(),
                category: None,
            }],
        };
        let forecast = build_study_plan_forecast(
            &plan,
            &plan_catalog(),
            &plan_answers(today),
            &ModulePracticeFilters::default(),
            today,
        );
        let item = &forecast.items[0];
        assert_eq!((item.total, item.done, item.remaining), (100, 40, 60));
        assert_eq!((item.answered_today, item.answered_yesterday), (15, 15));
        assert_eq!(item.answered_7, 45);
        assert_eq!(item.days_yesterday, Some(4));
        assert_eq!(item.days_7, Some(10));
        assert_eq!(item.finish_yesterday.as_deref(), Some("1/14"));
        assert_eq!(item.finish_7.as_deref(), Some("1/20"));
        assert!(!item.low_confidence);
        assert_eq!(forecast.total_remaining, 60);
        assert_eq!(forecast.days_yesterday, Some(4));
        assert_eq!(forecast.days_7, Some(10));
    }

    #[test]
    fn child_plan_marks_small_samples_as_low_confidence() {
        let today = NaiveDate::from_ymd_opt(2026, 1, 10).unwrap();
        let plan = StudyPlan {
            prediction_enabled: true,
            items: vec![StudyPlanItem {
                module: "资料分析".into(),
                category: Some("比重".into()),
            }],
        };
        let answers = vec![
            StudyAnswerDay {
                day: (today - Duration::days(1)).format("%Y-%m-%d").to_string(),
                module: "资料分析".into(),
                category: "比重".into(),
                answered: 10,
            },
            StudyAnswerDay {
                day: today.format("%Y-%m-%d").to_string(),
                module: "资料分析".into(),
                category: "比重".into(),
                answered: 10,
            },
        ];
        let forecast = build_study_plan_forecast(
            &plan,
            &plan_catalog(),
            &answers,
            &ModulePracticeFilters::default(),
            today,
        );
        let item = &forecast.items[0];
        assert_eq!((item.remaining, item.days_7), (30, Some(21)));
        assert_eq!((item.answered_today, item.answered_yesterday), (10, 10));
        assert_eq!(item.days_yesterday, Some(3));
        assert!(item.low_confidence);
        assert!(forecast.low_confidence);
    }

    #[test]
    fn parent_plan_supersedes_child_items_and_keeps_toggle() {
        let plan = StudyPlan {
            prediction_enabled: false,
            items: vec![
                StudyPlanItem {
                    module: "资料分析".into(),
                    category: Some("比重".into()),
                },
                StudyPlanItem {
                    module: "资料分析".into(),
                    category: None,
                },
                StudyPlanItem {
                    module: "资料分析".into(),
                    category: Some("增长".into()),
                },
            ],
        };
        let normalized = normalize_study_plan(plan).unwrap();
        assert_eq!(normalized.items.len(), 1);
        assert!(normalized.items[0].category.is_none());
        assert!(!normalized.prediction_enabled);
    }

    #[test]
    fn filter_label_names_years_and_regions() {
        let filters = ModulePracticeFilters {
            year_min: Some(2017),
            year_max: Some(2026),
            regions: vec!["国考".into()],
            exclude_regions: false,
            count: None,
        };
        assert_eq!(study_filter_label(&filters), "2017–2026 年 · 仅限：国考");
    }
}
