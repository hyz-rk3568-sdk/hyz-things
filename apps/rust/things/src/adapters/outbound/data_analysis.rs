//! Native Codex App Server question preparation from exam-inspired simulated topics.
//! Codex owns the answer; the application stores each freshly prepared practice attempt.
use super::codex_app_server::{
    validate_token, CodexSession, CANCELLED_MESSAGE, UNCONFIRMED_MESSAGE,
};
use crate::domain::data_analysis::{
    select_material_batch, topic_scope, Material, RelatedCardSnapshot, VerifiedQuestion,
    QUIZ_GROUPS, QUIZ_PER_GROUP, QUIZ_SECONDS, TOPICS,
};
use crate::domain::vcnt::{delta_vcnt, CardSchedule};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    future::Future,
    os::unix::fs::MetadataExt,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{watch, Mutex},
    task::JoinSet,
};
use uuid::Uuid;

const ENV_PATH: &str = "/userdata/hyz-things/.env";
const RECENT_THEME_PAPERS: usize = 4;
const MAX_GENERATION_CONCURRENCY: usize = 2;

// User-owned style and difficulty policy. Hard limits, selected categories,
// JSON shape and sources remain server-controlled even if this text changes.
pub const DEFAULT_USER_PROMPT: &str = r#"请生成符合国考、省考资料分析风格的原创模拟题。

材料：400～1000汉字，分2～4段，统计指标、单位、年份和同比关系自然且自洽；数字接近公报精度，避免刻意凑整。只陈述统计事实，不给公式、找数路径、方法提示或教学说明。

命题：优先考查对应 Flashcard 的审题、速算、易错点及变式，避免机械换数字或重复基础题。题干仅提出所求年份、主体和指标，不提示计算步骤。不出“下列说法正确的是”“下列说法错误的是”“根据资料可以推出的是”等需要逐项核查多个独立陈述的综合判断题。每题围绕一个明确的数量关系、指标或比较目标命题；即使选中了“综合分析与审题”类别，也优先将其转化为具体计算或比较题。

选项：每题四个合理干扰项，适合行测近似速算；答案唯一，材料、选项与解析相符。解析简洁，优先使用 hyz Flashcard 方法。

先自行检查数值关系和选项正确性，不得照搬真题原文或数字。"#;

#[derive(Clone, Serialize)]
pub struct UserPromptSettings {
    pub user_prompt: String,
    pub customized: bool,
}

#[derive(Clone)]
pub struct CodexConfig {
    // Fixed-address LAN authentication only; this is NOT the ChatGPT OAuth token.
    token: String,
    cwd: String,
    #[cfg(test)]
    test_server: Option<std::net::SocketAddr>,
}

impl CodexConfig {
    #[cfg(test)]
    pub(crate) fn for_test_server(address: std::net::SocketAddr) -> Self {
        Self {
            token: "a".repeat(64),
            cwd: "/home/hyz/hyz_things".into(),
            test_server: Some(address),
        }
    }

    pub(crate) async fn connect(&self) -> Result<CodexSession, String> {
        #[cfg(test)]
        if let Some(address) = self.test_server {
            return CodexSession::connect_test_server(address, &self.token, &self.cwd).await;
        }
        tokio::time::timeout(
            Duration::from_secs(12),
            CodexSession::connect(&self.token, &self.cwd),
        )
        .await
        .map_err(|_| "连接 Codex App Server 超时".to_owned())?
    }
}

/// Root-only .env file. Ignore unrelated configuration rather than accepting
/// user-supplied URLs, model IDs, or legacy OpenAI-compatible credentials.
pub fn load_codex_env() -> Result<Option<CodexConfig>, &'static str> {
    let meta = match fs::symlink_metadata(ENV_PATH) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("Codex .env cannot be read"),
    };
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.uid() != 0
        || meta.mode() & 0o077 != 0
        || meta.len() > 16 * 1024
    {
        return Err("Codex .env must be a root-owned 0600 regular file");
    }
    let input = fs::read_to_string(ENV_PATH).map_err(|_| "Codex .env cannot be read")?;
    parse_env(&input)
}

fn valid_remote_repo_path(value: &str) -> bool {
    // The path is sent to the PC, and must NOT be resolved on RK3568.
    // Never accept shell expansions, parent traversal or browser-selected paths.
    let path = std::path::Path::new(value);
    !value.is_empty()
        && value.len() <= 512
        && path.is_absolute()
        && path.components().all(|part| {
            matches!(
                part,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
        && !value.chars().any(|c| c.is_control())
}

fn parse_env(input: &str) -> Result<Option<CodexConfig>, &'static str> {
    let mut token = None;
    let mut cwd = None;
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or("Invalid .env entry")?;
        let mut value = value.trim();
        if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
            value = &value[1..value.len() - 1];
        }
        match key.trim() {
            "HYZ_THINGS_CODEX_WS_TOKEN" => {
                if token.is_some() {
                    return Err("Duplicate Codex Token");
                }
                validate_token(value).map_err(|_| "Codex WS Token has invalid characters")?;
                token = Some(value.to_owned());
            }
            "HYZ_THINGS_CODEX_CWD" => {
                if cwd.is_some() {
                    return Err("Duplicate Codex CWD");
                }
                if !valid_remote_repo_path(value) {
                    return Err("Codex CWD must be a safe absolute path on the PC");
                }
                cwd = Some(value.to_owned());
            }
            _ => {}
        }
    }
    match (token, cwd) {
        (Some(token), Some(cwd)) => Ok(Some(CodexConfig {
            token,
            cwd,
            #[cfg(test)]
            test_server: None,
        })),
        (None, None) => Ok(None),
        _ => Err("Codex needs both HYZ_THINGS_CODEX_WS_TOKEN and HYZ_THINGS_CODEX_CWD"),
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct QuestionGroup {
    #[serde(default)]
    pub version: u32,
    pub material: Material,
    pub questions: Vec<VerifiedQuestion>,
}

#[derive(Clone, Serialize)]
pub struct PublicQuestion {
    pub question: String,
    pub options: [String; 4],
}
#[derive(Clone, Serialize)]
pub struct PublicGroup {
    pub material: Material,
    pub questions: Vec<PublicQuestion>,
}
#[derive(Clone, Serialize)]
pub struct RunView {
    pub id: String,
    pub status: String,
    pub started_at_ms: Option<i64>,
    pub deadline_at_ms: Option<i64>,
    pub paused_remaining_ms: Option<i64>,
    /// Authoritative server clock sampled for this response; clients must
    /// not compare a device-local clock directly with deadline_at_ms.
    pub server_now_ms: i64,
    pub duration_seconds: u64,
    pub groups: Vec<PublicGroup>,
    pub answers: Vec<Option<usize>>,
    pub current_question: usize,
}
#[derive(Clone, Serialize)]
pub struct ClaimedRun {
    pub run: RunView,
    pub control_token: String,
}
#[derive(Clone, Serialize)]
pub struct UnfinishedRun {
    pub id: String,
    pub status: String,
    pub answered: usize,
    pub current_question: usize,
    pub deadline_at_ms: Option<i64>,
    pub paused_remaining_ms: Option<i64>,
    pub created_at_ms: i64,
}
#[derive(Clone, Serialize)]
pub struct GradedQuestion {
    pub number: usize,
    pub question: String,
    pub options: [String; 4],
    pub chosen: Option<usize>,
    pub correct_index: usize,
    pub answer: String,
    pub explanation: String,
    pub topic: String,
    pub skill: String,
    pub evidence: String,
    pub card_path: Option<String>,
    pub card_id: Option<String>,
    pub related_card: Option<RelatedCardSnapshot>,
}
#[derive(Clone, Serialize)]
pub struct RunResult {
    pub id: String,
    pub correct: usize,
    pub total: usize,
    pub questions: Vec<GradedQuestion>,
    pub materials: Vec<Material>,
}

#[derive(Clone, Serialize)]
pub struct ComprehensiveHistoryEntry {
    pub id: String,
    pub attempt_no: i64,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub elapsed_ms: i64,
    pub correct: usize,
    pub total: usize,
    pub topics: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct AnswerSummary {
    pub answered_count: i64,
    pub correct_count: i64,
}

#[derive(Clone, Serialize)]
pub struct CardPerformance {
    pub card_id: String,
    pub category: String,
    pub source_file: String,
    pub front_markdown: String,
    pub back_markdown: String,
    pub answered_count: i64,
    pub correct_count: i64,
}

#[derive(Clone, Serialize)]
pub struct PrepareProgress {
    pub state: String,
    pub verified: usize,
    pub message: String,
    pub run_id: Option<String>,
}

#[derive(Clone)]
pub struct DataAnalysisService {
    pool: SqlitePool,
    config: Option<CodexConfig>,
    jobs: Arc<Mutex<HashMap<String, PrepareProgress>>>,
    gate: Arc<tokio::sync::Semaphore>,
}

/// Deadline failures are fatal to the entire paper. Content/structure errors
/// may still be retried with a different theme.
fn is_generation_timeout(error: &str) -> bool {
    error == "连接 Codex App Server 超时" || error.starts_with("Codex 出题超时")
}

/// Run one bounded wave. Each task establishes its own Codex WS session and
/// ephemeral Thread, avoiding response-ID multiplexing and mixed conversations.
/// Results retain their source slot even if the network finishes out of order.
async fn run_generation_wave<F, Fut>(
    candidates: Vec<(usize, Material)>,
    producer: F,
) -> Result<Vec<(usize, Result<QuestionGroup, String>)>, String>
where
    F: Fn(Material, usize, watch::Sender<bool>) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = Result<QuestionGroup, String>> + Send + 'static,
{
    debug_assert!(candidates.len() <= QUIZ_GROUPS);
    let mut running = JoinSet::new();
    let (cancellation, _) = watch::channel(false);
    let permits = Arc::new(tokio::sync::Semaphore::new(MAX_GENERATION_CONCURRENCY));
    for (index, material) in candidates {
        let produce = producer.clone();
        let permits = Arc::clone(&permits);
        let cancellation = cancellation.clone();
        running.spawn(async move {
            let _permit = permits
                .acquire_owned()
                .await
                .expect("generation semaphore open");
            if *cancellation.borrow() {
                return (index, Err(CANCELLED_MESSAGE.into()));
            }
            let result = produce(material, index, cancellation.clone()).await;
            if result
                .as_ref()
                .err()
                .is_some_and(|error| is_generation_timeout(error))
            {
                // Publish before releasing the permit so queued groups cannot start.
                cancellation.send_replace(true);
            }
            (index, result)
        });
    }
    let mut results = Vec::new();
    let mut timeout_error = None;
    let mut cleanup_unconfirmed = false;
    while let Some(outcome) = running.join_next().await {
        match outcome {
            Ok(result) => {
                if let Err(error) = &result.1 {
                    cleanup_unconfirmed |= error.contains(UNCONFIRMED_MESSAGE);
                    if is_generation_timeout(error) {
                        cancellation.send_replace(true);
                        timeout_error.get_or_insert_with(|| error.clone());
                    }
                }
                results.push(result);
            }
            Err(_) => {
                cleanup_unconfirmed = true;
                results.push((usize::MAX, Err("Codex 并行出题任务意外中断".to_owned())));
            }
        }
    }
    if let Some(mut error) = timeout_error {
        if cleanup_unconfirmed && !error.contains(UNCONFIRMED_MESSAGE) {
            error.push_str(&format!("；其他会话{UNCONFIRMED_MESSAGE}，请检查电脑服务"));
        }
        return Err(error);
    }
    Ok(results)
}

// The SQLite flashcards table is the single selection snapshot. A category is
// shared across wrong/knowledge directories; each card ID is unique per paper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceKind {
    Wrong,
    Knowledge,
}
impl SourceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Wrong => "wrong",
            Self::Knowledge => "knowledge",
        }
    }
}

#[derive(Clone, Debug)]
struct CardCandidate {
    id: String,
    card_path: String,
    source_hash: String,
    category: String,
    source_kind: SourceKind,
    front_markdown: String,
    back_markdown: String,
}

#[derive(Clone, Debug)]
struct CardTarget {
    target_id: String,
    card: CardCandidate,
    vcnt_increment: i64,
    expected_vcnt: i64,
}

#[derive(Clone, Debug)]
struct QuestionPlan {
    groups: [Vec<CardTarget>; QUIZ_GROUPS],
}

struct CardRng(u64);
impl CardRng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.next() as usize % (i + 1);
            items.swap(i, j);
        }
    }
}

fn card_from_db(
    id: String,
    source_file: String,
    deck: String,
    front: String,
    back: String,
    hash: String,
) -> Option<CardCandidate> {
    // Never accept paths from unrelated modules or nested subdecks.
    let wrong_source = wrong_question_source(&deck);
    let (kind, category) = if let Some((name, _)) = wrong_source {
        (SourceKind::Wrong, name)
    } else {
        (SourceKind::Knowledge, knowledge_card_category(&deck)?)
    };
    if category == "错题"
        || !valid_wrong_category(category)
        || crate::domain::flashcards::FlashcardId::new(id.clone()).is_err()
        || hash.is_empty()
        || front.trim().is_empty()
        || back.trim().is_empty()
    {
        return None;
    }
    let leaf = std::path::Path::new(&source_file)
        .file_name()
        .and_then(|name| name.to_str())?;
    if !matches!(leaf, "card.md" | "cards.md") {
        return None;
    }
    // The canonical display path is reconstructed from a verified deck, not
    // trusted from an arbitrary source_file path.
    let card_path = match (kind, wrong_source) {
        (SourceKind::Wrong, Some((_, true))) => {
            format!("flashcards/资料/{category}/错题/{leaf}")
        }
        (SourceKind::Wrong, _) => format!("flashcards/资料/错题/{category}/{leaf}"),
        (SourceKind::Knowledge, _) => format!("flashcards/资料/{category}/{leaf}"),
    };
    Some(CardCandidate {
        id,
        card_path,
        source_hash: hash,
        category: category.to_owned(),
        source_kind: kind,
        front_markdown: front,
        back_markdown: back,
    })
}

// Integral min-cost flow. Category -> passage edges enforce one question per
// category in each passage; source -> category marginal edges encode historical
// fair_count plus the cost of additional exposure in this paper.
struct FlowEdge {
    to: usize,
    reverse: usize,
    capacity: i32,
    cost: i64,
}
struct FlowNetwork {
    edges: Vec<Vec<FlowEdge>>,
}
impl FlowNetwork {
    fn new(nodes: usize) -> Self {
        Self {
            edges: (0..nodes).map(|_| Vec::new()).collect(),
        }
    }
    fn add(&mut self, from: usize, to: usize, capacity: i32, cost: i64) {
        let reverse_to = self.edges[to].len();
        let reverse_from = self.edges[from].len();
        self.edges[from].push(FlowEdge {
            to,
            reverse: reverse_to,
            capacity,
            cost,
        });
        self.edges[to].push(FlowEdge {
            to: from,
            reverse: reverse_from,
            capacity: 0,
            cost: -cost,
        });
    }
    fn min_cost_flow(&mut self, source: usize, sink: usize) -> usize {
        let mut total = 0;
        loop {
            // The residual network has negative reverse costs, so BFS alone
            // would NOT implement fair scheduling. Bellman-Ford finds the
            // least-cost augmenting path, including reassignments.
            let n = self.edges.len();
            let mut distance = vec![i64::MAX / 4; n];
            let mut parent = vec![None::<(usize, usize)>; n];
            distance[source] = 0;
            for _ in 0..n - 1 {
                let mut changed = false;
                for from in 0..n {
                    if distance[from] == i64::MAX / 4 {
                        continue;
                    }
                    for (edge_idx, edge) in self.edges[from].iter().enumerate() {
                        if edge.capacity > 0 {
                            let candidate = distance[from].saturating_add(edge.cost);
                            if candidate < distance[edge.to] {
                                distance[edge.to] = candidate;
                                parent[edge.to] = Some((from, edge_idx));
                                changed = true;
                            }
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
            if parent[sink].is_none() {
                return total;
            }
            let mut node = sink;
            while node != source {
                let (from, edge_idx) = parent[node].expect("min-cost augmenting path");
                let reverse = self.edges[from][edge_idx].reverse;
                self.edges[from][edge_idx].capacity -= 1;
                self.edges[node][reverse].capacity += 1;
                node = from;
            }
            total += 1;
        }
    }
}

fn plan_question_targets(
    cards: &[CardCandidate],
    seed: u64,
    schedule: &CardSchedule,
) -> Result<QuestionPlan, String> {
    let mut rng = CardRng::new(seed);
    let mut queues = BTreeMap::<String, Vec<CardCandidate>>::new();
    for card in cards {
        queues
            .entry(card.category.clone())
            .or_default()
            .push(card.clone());
    }
    if queues.len() < QUIZ_PER_GROUP {
        return Err("资料分析至少需要5个有效题型".into());
    }
    if cards.len() < QUIZ_GROUPS * QUIZ_PER_GROUP {
        return Err("资料分析至少需要20张不同卡片".into());
    }
    for queue in queues.values_mut() {
        rng.shuffle(queue);
        queue.sort_by_key(|card| schedule.card(&card.id));
        queue.truncate(QUIZ_GROUPS);
    }
    // Each source-to-type marginal edge costs one real card's vcnt.
    // The network chooses counts and material placement jointly.
    let mut types: Vec<_> = queues.into_iter().collect();
    rng.shuffle(&mut types);
    let group_base = 1 + types.len();
    let sink = group_base + QUIZ_GROUPS;
    let mut net = FlowNetwork::new(sink + 1);
    for (index, (_, queue)) in types.iter().enumerate() {
        for card in queue {
            net.add(0, 1 + index, 1, schedule.card(&card.id));
        }
        let mut slots: Vec<_> = (0..QUIZ_GROUPS).collect();
        rng.shuffle(&mut slots);
        for slot in slots {
            net.add(1 + index, group_base + slot, 1, 0);
        }
    }
    for slot in 0..QUIZ_GROUPS {
        net.add(group_base + slot, sink, QUIZ_PER_GROUP as i32, 0);
    }
    if net.min_cost_flow(0, sink) != QUIZ_GROUPS * QUIZ_PER_GROUP {
        return Err("卡片池无法满足每篇5种不同题型及整卷20张不同卡片".into());
    }
    let mut groups: [Vec<CardCandidate>; QUIZ_GROUPS] = std::array::from_fn(|_| Vec::new());
    for (index, (_, queue)) in types.iter().enumerate() {
        let mut slots = Vec::new();
        for edge in &net.edges[1 + index] {
            if (group_base..sink).contains(&edge.to) && edge.capacity == 0 {
                slots.push(edge.to - group_base);
            }
        }
        // Nondecreasing marginal costs select the cheapest prefix.
        for (card, slot) in queue.iter().zip(slots) {
            groups[slot].push(card.clone());
        }
    }
    let plan = QuestionPlan {
        groups: std::array::from_fn(|slot| {
            rng.shuffle(&mut groups[slot]);
            groups[slot]
                .iter()
                .enumerate()
                .map(|(index, card)| CardTarget {
                    target_id: format!("g{}-q{}", slot + 1, index + 1),
                    card: card.clone(),
                    vcnt_increment: schedule.increment(&card.id),
                    expected_vcnt: schedule.card(&card.id),
                })
                .collect()
        }),
    };
    validate_question_plan(&plan)?;
    Ok(plan)
}

fn validate_question_plan(plan: &QuestionPlan) -> Result<(), String> {
    let mut ids = HashSet::new();
    for group in &plan.groups {
        if group.len() != QUIZ_PER_GROUP {
            return Err("QuestionPlan 某篇材料不是5个题位".into());
        }
        let mut categories = HashSet::new();
        for target in group {
            if !ids.insert(target.card.id.as_str()) {
                return Err("QuestionPlan 整卷存在重复 card_id".into());
            }
            if !categories.insert(target.card.category.as_str()) {
                return Err("QuestionPlan 同篇出现重复知识点分类".into());
            }
        }
    }
    Ok(())
}

fn target_ids_match(expected: &[CardTarget], returned: &[String]) -> bool {
    if returned.len() != expected.len() {
        return false;
    }
    let planned: HashSet<_> = expected.iter().map(|t| t.target_id.as_str()).collect();
    let received: HashSet<_> = returned.iter().map(String::as_str).collect();
    received.len() == expected.len() && received == planned
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
fn as_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

async fn restore_related_cards(
    connection: &mut sqlx::SqliteConnection,
    groups: &mut [QuestionGroup],
) -> Result<(), String> {
    for question in groups.iter_mut().flat_map(|group| &mut group.questions) {
        if question.related_card.is_some() || question.source_hash.is_none() {
            continue;
        }
        let Some(id) = question.card_id.as_deref() else {
            continue;
        };
        let row = sqlx::query(
            "SELECT source_hash,front_markdown,back_markdown FROM flashcards WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(as_error)?;
        if let Some(row) = row {
            let hash: String = row.try_get("source_hash").map_err(as_error)?;
            question.related_card = question.related_card_for(
                id,
                &hash,
                RelatedCardSnapshot {
                    front: row.try_get("front_markdown").map_err(as_error)?,
                    back: row.try_get("back_markdown").map_err(as_error)?,
                },
            );
        }
    }
    Ok(())
}

/// Build a graded view from saved answers without changing the run or requiring
/// the editing lease. Callers must check submission status before history reads.
fn grade_run(
    id: &str,
    groups: Vec<QuestionGroup>,
    answers: Vec<Option<usize>>,
) -> Result<RunResult, String> {
    if groups.len() != QUIZ_GROUPS
        || groups.iter().any(|g| g.questions.len() != QUIZ_PER_GROUP)
        || answers.len() != QUIZ_GROUPS * QUIZ_PER_GROUP
    {
        return Err("训练题量错误，不能交卷".into());
    }
    let mut questions = Vec::with_capacity(QUIZ_GROUPS * QUIZ_PER_GROUP);
    for (group_index, group) in groups.iter().enumerate() {
        for (local_index, q) in group.questions.iter().enumerate() {
            let index = group_index * QUIZ_PER_GROUP + local_index;
            questions.push(GradedQuestion {
                number: index + 1,
                question: q.question.clone(),
                options: q.options.clone(),
                chosen: answers[index],
                correct_index: q.correct_index,
                answer: q.answer.clone(),
                explanation: q.explanation.clone(),
                topic: group.material.topic.clone(),
                skill: q.skill.clone(),
                evidence: q.evidence.clone(),
                card_path: q.card_path.clone(),
                card_id: q.card_id.clone(),
                related_card: q.related_card.clone(),
            });
        }
    }
    let correct = questions
        .iter()
        .filter(|q| q.chosen == Some(q.correct_index))
        .count();
    Ok(RunResult {
        id: id.into(),
        correct,
        total: QUIZ_GROUPS * QUIZ_PER_GROUP,
        questions,
        materials: groups.into_iter().map(|g| g.material).collect(),
    })
}

impl DataAnalysisService {
    pub fn new(pool: SqlitePool, config: Option<CodexConfig>) -> Result<Self, String> {
        Ok(Self {
            pool,
            config,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            gate: Arc::new(tokio::sync::Semaphore::new(1)),
        })
    }
    pub fn configured_model(&self) -> Option<String> {
        self.config.as_ref().map(|_| "Codex App Server".to_owned())
    }
    pub async fn counts(&self) -> Result<(i64, i64), String> {
        let sources = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM data_analysis_sources")
            .fetch_one(&self.pool)
            .await
            .map_err(as_error)?;
        let groups = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM data_analysis_groups")
            .fetch_one(&self.pool)
            .await
            .map_err(as_error)?;
        Ok((sources, groups))
    }
    /// Read only newly generated papers, newest first. A replay copies its
    /// parent topics and must never count as an independent theme selection.
    /// Both unfinished and submitted originals count as recently used.
    /// Aggregate submitted, actually answered questions (including replays).
    /// Return every ACTIVE card with persisted scheduling state, ordered by
    /// its actual vcnt. Scheduling counts never leave the server.
    pub async fn card_performance(&self) -> Result<(AnswerSummary, Vec<CardPerformance>), String> {
        let mut tx = self.pool.begin().await.map_err(as_error)?;
        let summary_row = sqlx::query(
            "SELECT COUNT(*) AS n, COALESCE(SUM(r.is_correct),0) AS c
             FROM data_analysis_results r
             JOIN data_analysis_runs run ON run.id=r.run_id
             WHERE run.status='submitted' AND r.was_answered=1",
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(as_error)?;
        let summary = AnswerSummary {
            answered_count: summary_row.try_get("n").map_err(as_error)?,
            correct_count: summary_row.try_get("c").map_err(as_error)?,
        };
        let rows = sqlx::query(
            "SELECT u.card_id,u.category,f.source_file,f.front_markdown,f.back_markdown,
                    COALESCE(s.n,0) AS answered_count,
                    COALESCE(s.c,0) AS correct_count
             FROM data_analysis_card_usage u
             JOIN flashcards f ON f.id=u.card_id AND f.active=1
             LEFT JOIN (
                SELECT r.card_id,COUNT(*) AS n,COALESCE(SUM(r.is_correct),0) AS c
                FROM data_analysis_results r
                JOIN data_analysis_runs run ON run.id=r.run_id
                WHERE run.status='submitted' AND r.was_answered=1
                  AND r.card_id IS NOT NULL
                GROUP BY r.card_id
             ) s ON s.card_id=u.card_id
             ORDER BY u.vcnt ASC,u.card_id ASC",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(as_error)?;
        let cards = rows
            .into_iter()
            .map(|row| {
                Ok(CardPerformance {
                    card_id: row.try_get("card_id").map_err(as_error)?,
                    category: row.try_get("category").map_err(as_error)?,
                    source_file: row.try_get("source_file").map_err(as_error)?,
                    front_markdown: row.try_get("front_markdown").map_err(as_error)?,
                    back_markdown: row.try_get("back_markdown").map_err(as_error)?,
                    answered_count: row.try_get("answered_count").map_err(as_error)?,
                    correct_count: row.try_get("correct_count").map_err(as_error)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        tx.commit().await.map_err(as_error)?;
        Ok((summary, cards))
    }

    async fn recent_paper_themes(&self) -> Result<Vec<Vec<String>>, String> {
        let rows = sqlx::query(
            "SELECT groups_json FROM data_analysis_runs
             WHERE origin_run_id IS NULL
             ORDER BY created_at_ms DESC, id DESC LIMIT ?",
        )
        .bind(RECENT_THEME_PAPERS as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(as_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let json: String = row.try_get("groups_json").ok()?;
                let groups: Vec<QuestionGroup> = serde_json::from_str(&json).ok()?;
                (groups.len() == QUIZ_GROUPS).then(|| {
                    groups
                        .into_iter()
                        .map(|group| group.material.topic)
                        .collect::<Vec<_>>()
                })
            })
            .collect())
    }

    /// Register new cards with the global minimum and snapshot weights.
    async fn load_card_schedule(&self, cards: &[CardCandidate]) -> Result<CardSchedule, String> {
        let mut tx = self.pool.begin().await.map_err(as_error)?;
        for card in cards {
            sqlx::query(
                "UPDATE data_analysis_card_usage SET category=? WHERE card_id=? AND category<>?",
            )
            .bind(&card.category)
            .bind(&card.id)
            .bind(&card.category)
            .execute(&mut *tx)
            .await
            .map_err(as_error)?;
            sqlx::query(
                "INSERT INTO data_analysis_card_usage(card_id,category,selected_count,fair_count,vcnt)
                 SELECT ?, ?, 0, 0, COALESCE((SELECT MIN(vcnt) FROM data_analysis_card_usage), 0)
                 ON CONFLICT(card_id) DO NOTHING"
            )
            .bind(&card.id).bind(&card.category)
            .execute(&mut *tx).await.map_err(as_error)?;
        }
        let mut schedule = CardSchedule::default();
        for row in sqlx::query("SELECT card_id,vcnt FROM data_analysis_card_usage")
            .fetch_all(&mut *tx)
            .await
            .map_err(as_error)?
        {
            schedule.cards.insert(
                row.try_get("card_id").map_err(as_error)?,
                row.try_get("vcnt").map_err(as_error)?,
            );
        }
        for row in sqlx::query(
            "SELECT r.card_id, COUNT(*) AS n, COALESCE(SUM(r.is_correct),0) AS c
             FROM data_analysis_results r
             JOIN data_analysis_runs run ON run.id=r.run_id
             WHERE run.status='submitted' AND r.was_answered=1 AND r.card_id IS NOT NULL
             GROUP BY r.card_id",
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(as_error)?
        {
            let id: String = row.try_get("card_id").map_err(as_error)?;
            let n: i64 = row.try_get("n").map_err(as_error)?;
            let c: i64 = row.try_get("c").map_err(as_error)?;
            schedule
                .increments
                .insert(id, delta_vcnt(c as u64, n as u64));
        }
        tx.commit().await.map_err(as_error)?;
        Ok(schedule)
    }

    /// Only an entirely prepared ORIGINAL paper consumes scheduling credits.
    /// Run creation, 20 ledger entries and both counter updates commit atomically.
    async fn persist_original_run(
        &self,
        id: &str,
        groups: &[QuestionGroup],
        plan: &QuestionPlan,
    ) -> Result<(), String> {
        if groups.len() != QUIZ_GROUPS
            || groups
                .iter()
                .any(|group| group.questions.len() != QUIZ_PER_GROUP)
        {
            return Err("完整20题生成前不得累计调度次数".into());
        }
        validate_question_plan(plan)?;
        let mut tx = self.pool.begin().await.map_err(as_error)?;
        let created_at = now_ms();
        sqlx::query(
            "INSERT INTO data_analysis_runs(id,groups_json,answers_json,status,created_at_ms)
             VALUES(?,?,?,'ready',?)",
        )
        .bind(id)
        .bind(serde_json::to_string(groups).map_err(as_error)?)
        .bind(
            serde_json::to_string(&vec![None::<usize>; QUIZ_GROUPS * QUIZ_PER_GROUP])
                .map_err(as_error)?,
        )
        .bind(created_at)
        .execute(&mut *tx)
        .await
        .map_err(as_error)?;

        for target in plan.groups.iter().flatten() {
            sqlx::query(
                "INSERT INTO data_analysis_card_selection_log(run_id,card_id,category,selected_at_ms)
                 VALUES(?,?,?,?)",
            )
            .bind(id)
            .bind(&target.card.id)
            .bind(&target.card.category)
            .bind(created_at)
            .execute(&mut *tx)
            .await
            .map_err(as_error)?;
            let updated = sqlx::query(
                "UPDATE data_analysis_card_usage
                 SET selected_count=selected_count+1,vcnt=vcnt+?
                 WHERE card_id=? AND category=? AND vcnt=? AND vcnt<=?",
            )
            .bind(target.vcnt_increment)
            .bind(&target.card.id)
            .bind(&target.card.category)
            .bind(target.expected_vcnt)
            .bind(i64::MAX - target.vcnt_increment)
            .execute(&mut *tx)
            .await
            .map_err(as_error)?;
            if updated.rows_affected() != 1 {
                return Err("卡片虚拟计数变更失败或溢出，整卷入库已回滚".into());
            }
        }
        tx.commit().await.map_err(as_error)?;
        Ok(())
    }

    async fn card_candidates(&self) -> Result<Vec<CardCandidate>, String> {
        let rows = sqlx::query(
            "SELECT id,source_file,deck_path,front_markdown,back_markdown,source_hash \
             FROM flashcards WHERE active=1 ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(as_error)?;
        let cards: Vec<_> = rows
            .into_iter()
            .filter_map(|row| {
                card_from_db(
                    row.try_get("id").ok()?,
                    row.try_get("source_file").ok()?,
                    row.try_get("deck_path").ok()?,
                    row.try_get("front_markdown").ok()?,
                    row.try_get("back_markdown").ok()?,
                    row.try_get("source_hash").ok()?,
                )
            })
            .collect();
        if cards.is_empty() {
            return Err("SQLite 尚无有效资料分析 Flashcard，请先同步卡片".into());
        }
        Ok(cards)
    }

    pub async fn user_prompt_settings(&self) -> Result<UserPromptSettings, String> {
        let saved = sqlx::query_scalar::<_, String>(
            "SELECT user_prompt FROM data_analysis_prompt_settings WHERE id=1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(as_error)?;
        Ok(match saved {
            Some(user_prompt) => UserPromptSettings {
                user_prompt,
                customized: true,
            },
            None => UserPromptSettings {
                user_prompt: DEFAULT_USER_PROMPT.to_owned(),
                customized: false,
            },
        })
    }

    /// None restores the default. User input can never replace system constraints.
    pub async fn save_user_prompt(&self, text: Option<&str>) -> Result<UserPromptSettings, String> {
        if let Some(prompt) = text {
            if prompt.trim().is_empty()
                || prompt.len() > 8000
                || prompt
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
            {
                return Err("出题要求不能为空，且不能超过8000字节".into());
            }
            sqlx::query(
                "INSERT INTO data_analysis_prompt_settings(id,user_prompt,updated_at_ms)
                 VALUES(1,?,?)
                 ON CONFLICT(id) DO UPDATE SET
                     user_prompt=excluded.user_prompt,
                     updated_at_ms=excluded.updated_at_ms",
            )
            .bind(prompt.trim())
            .bind(now_ms())
            .execute(&self.pool)
            .await
            .map_err(as_error)?;
        } else {
            sqlx::query("DELETE FROM data_analysis_prompt_settings WHERE id=1")
                .execute(&self.pool)
                .await
                .map_err(as_error)?;
        }
        self.user_prompt_settings().await
    }

    /// Current running comprehensive preparation, including progress across page changes.
    pub async fn preparing_job(&self) -> Option<String> {
        self.jobs
            .lock()
            .await
            .iter()
            .find(|(_, progress)| !matches!(progress.state.as_str(), "ready" | "failed"))
            .map(|(id, _)| id.clone())
    }

    pub async fn prepare(&self) -> Result<String, String> {
        if self.config.is_none() {
            return Err(
                "请在 RK3568 的 /userdata/hyz-things/.env 配置 Codex WebSocket Token".into(),
            );
        }
        // Freeze the current user instructions for one entire 20-question job.
        let user_prompt = self.user_prompt_settings().await?.user_prompt;
        if let Some(id) = self.preparing_job().await {
            return Ok(id);
        }
        // Reserve the exclusive AI slot *before* publishing a job ID. A second
        // click or another tab must not queue an unintended second paper.
        let guard = self
            .gate
            .clone()
            .try_acquire_owned()
            .map_err(|_| "已有其他出题任务正在执行，请等当前任务完成".to_owned())?;
        let id = Uuid::now_v7().to_string();
        self.jobs.lock().await.insert(
            id.clone(),
            PrepareProgress {
                state: "pending".into(),
                verified: 0,
                message: "正在准备资料".into(),
                run_id: None,
            },
        );
        let worker = self.clone();
        let job = id.clone();
        tokio::spawn(async move {
            let result = worker.generate(&job, &user_prompt).await;
            drop(guard);
            match result {
                Ok(run_id) => {
                    worker
                        .set_progress(
                            &job,
                            "ready",
                            20,
                            "Codex 已交付20道题，可以开始训练",
                            Some(run_id),
                        )
                        .await
                }
                Err(msg) => worker.set_progress(&job, "failed", 0, &msg, None).await,
            }
        });
        Ok(id)
    }
    async fn set_progress(
        &self,
        id: &str,
        state: &str,
        verified: usize,
        message: &str,
        run_id: Option<String>,
    ) {
        let mut jobs = self.jobs.lock().await;
        if let Some(job) = jobs.get_mut(id) {
            job.state = state.to_owned();
            job.verified = verified;
            job.message = message.to_owned();
            job.run_id = run_id;
        }
    }
    pub async fn job(&self, id: &str) -> Option<PrepareProgress> {
        self.jobs.lock().await.get(id).cloned()
    }
    async fn generate(&self, job: &str, user_prompt: &str) -> Result<String, String> {
        self.set_progress(job, "selecting", 0, "正在规划参考真题的统计主题", None)
            .await;
        // Keep a single randomized, balanced quota plan for the entire job,
        // including retries after a Codex passage fails validation.
        let seed = Uuid::parse_str(job).map_err(as_error)?.as_u128() as u64;
        let cards = self.card_candidates().await?;
        let schedule = self.load_card_schedule(&cards).await?;
        let plan = Arc::new(plan_question_targets(&cards, seed, &schedule)?);
        let recent_papers = self.recent_paper_themes().await?;
        let theme_seed =
            Uuid::parse_str(job).map_err(as_error)?.as_u128() as u64 ^ 0x9e3779b97f4a7c15;
        let ordered = order_exam_themes(exam_theme_materials(), &recent_papers, theme_seed);
        // Keep the original four slots even if only some parallel tasks succeed.
        // Retried slots must retain their assigned wrong/knowledge quotas.
        let mut ready = vec![None::<QuestionGroup>; QUIZ_GROUPS];
        let mut attempted = HashSet::<String>::new();
        let mut last_error = String::new();
        let mut attempts = 0;
        loop {
            let missing = ready
                .iter()
                .enumerate()
                .filter_map(|(slot, group)| group.is_none().then_some(slot))
                .collect::<Vec<_>>();
            if missing.is_empty() {
                break;
            }
            let completed: Vec<Material> = ready
                .iter()
                .flatten()
                .map(|group| group.material.clone())
                .collect();
            let wave = select_material_batch(&ordered, &completed, &attempted, missing.len());
            if wave.is_empty() {
                break;
            }

            let mut ordered_results = vec![None::<QuestionGroup>; QUIZ_GROUPS];
            let mut requests = Vec::new();
            for (slot, material) in missing.into_iter().zip(wave.iter()) {
                attempted.insert(material.topic.clone());
                if attempts < 12 {
                    attempts += 1;
                    requests.push((slot, material.clone()));
                }
            }
            if requests.is_empty() {
                break;
            }

            self.set_progress(
                job,
                "generating",
                ready.iter().filter(|group| group.is_some()).count() * QUIZ_PER_GROUP,
                "最多2个 Codex Thread 正在生成错题强化与资料知识点混合试卷",
                None,
            )
            .await;
            let worker = self.clone();
            let plan = Arc::clone(&plan);
            let user_prompt = Arc::new(user_prompt.to_owned());
            for (slot, outcome) in
                run_generation_wave(requests, move |material, slot, cancellation| {
                    let worker = worker.clone();
                    let plan = Arc::clone(&plan);
                    let user_prompt = Arc::clone(&user_prompt);
                    async move {
                        worker
                            .generate_group(&material, &plan, &user_prompt, slot, cancellation)
                            .await
                    }
                })
                .await?
            {
                match outcome {
                    Ok(group) if slot < QUIZ_GROUPS => {
                        ordered_results[slot] = Some(group);
                    }
                    Ok(_) => last_error = "Codex 并行任务返回异常序号".into(),
                    Err(error) => last_error = error,
                }
            }

            // Only the coordinator persists groups in the assigned slot.
            for (slot, group) in ordered_results.into_iter().enumerate() {
                let Some(group) = group else { continue };
                sqlx::query("INSERT INTO data_analysis_groups(id,topic,group_json,created_at_ms) VALUES (?,?,?,?)")
                    .bind(Uuid::now_v7().to_string())
                    .bind(&group.material.topic)
                    .bind(serde_json::to_string(&group).map_err(as_error)?)
                    .bind(now_ms())
                    .execute(&self.pool).await.map_err(as_error)?;
                ready[slot] = Some(group);
                self.set_progress(
                    job,
                    "generating",
                    ready.iter().filter(|group| group.is_some()).count() * QUIZ_PER_GROUP,
                    "已接收 Codex 完成的模拟试题并保存",
                    None,
                )
                .await;
            }
        }
        let count = ready.iter().filter(|group| group.is_some()).count();
        if count != QUIZ_GROUPS {
            return Err(format!(
                "已获得{}/20题，无法组成整卷：{}",
                count * QUIZ_PER_GROUP,
                last_error
            ));
        }
        let groups = ready.into_iter().flatten().collect::<Vec<_>>();
        let id = Uuid::now_v7().to_string();
        self.persist_original_run(&id, &groups, &plan).await?;
        Ok(id)
    }
    /// Read a small set of hyz's actual wrong questions as question-form
    /// patterns (up to twenty). Historical numbers are masked before reaching the AI provider.
    /// These are never treated as official statistical evidence.
    /// Codex reads the current hyz Skill/Flashcards from the PC repository.
    /// Its final answers/options are accepted as delivered; the server only
    /// decodes the contract the UI needs and never recomputes the solution.
    async fn generate_group(
        &self,
        source: &Material,
        plan: &QuestionPlan,
        user_prompt: &str,
        ordinal: usize,
        cancellation: watch::Sender<bool>,
    ) -> Result<QuestionGroup, String> {
        let config = self.config.as_ref().ok_or("Codex Token 未配置")?;
        let context = source.clone();
        // Codex App Server accepts a single turn text; these sections are
        // application-side boundaries, NOT an actual privileged system role.
        // Output schema and source quotas are enforced outside the model.
        let targets = &plan.groups[ordinal];
        let targets_text = targets.iter().map(|target| format!(
            "【目标 {target_id}】\n来源：{kind}\n分类：{category}\n卡片ID：{id}\n卡片路径：{path}\n### Front\n{front}\n### Back\n{back}\n",
            target_id = target.target_id,
            kind = target.card.source_kind.as_str(),
            category = target.card.category,
            id = target.card.id,
            path = target.card.card_path,
            front = target.card.front_markdown,
            back = target.card.back_markdown,
        )).collect::<Vec<_>>().join("\n");
        let prompt = format!(
            r#"【固定规则 / 应用端约束】
你是 hyz 的公务员行测资料分析命题 Agent。先读取仓库 .agents/skills/行测做题/SKILL.md 及其资料分析参考，再严格使用下方五张已指定的 Flashcard 内容。
生成一篇自洽统计材料和恰好5道四选一题，唯一正确，答案与解析一致，材料只包含统计事实。
必须使用 Codex 输出 Schema；每道题对应一个不同的指定 target_id，skill、source_kind 必须与该卡片一致；不得自行选卡、替换卡片或引用额外卡片补位。
联网参考可验证来源的国考、省考真题材料风格，不照搬原题与数字。

【本轮动态上下文】
本次是4篇×5题的第 {group_no} 篇；统计主题「{topic}」，参考指标范围：「{topic_scope}」。
材料应围绕此主题展开；参考指标只限定选材方向，无须全部涵盖，不得拼凑无关指标。
这5张卡片已由后端从 SQLite 选定，其中可能同时包含错题卡与普通卡。每张卡片恰好出1道题，允许调整题目顺序。
先根据这5个目标构造共享统计材料，再设计题目；自己核对计算、正确答案和干扰项。

【本篇固定 Flashcard】
{targets_text}

【用户出题要求 / 自定义】
{user_prompt}

只输出 Schema 对应的 JSON，不输出解释性前言。"#,
            group_no = ordinal + 1,
            topic = context.topic,
            topic_scope = topic_scope(&context.topic).unwrap_or("该主题相关统计指标"),
            targets_text = targets_text,
        );
        let mut cancelled = cancellation.subscribe();
        let mut session = tokio::select! {
            result = tokio::time::timeout(
                Duration::from_secs(12),
                CodexSession::connect(&config.token, &config.cwd),
            ) => result.map_err(|_| "连接 Codex App Server 超时".to_owned())??,
            _ = cancelled.wait_for(|value| *value) => {
                return Err(CANCELLED_MESSAGE.into());
            }
        };
        let output = session.turn_cancellable(&prompt, cancellation).await?;
        #[derive(Deserialize)]
        struct GeneratedQuestion {
            #[serde(flatten)]
            question: VerifiedQuestion,
            source_kind: String,
            target_id: String,
        }
        #[derive(Deserialize)]
        struct Batch {
            material_text: String,
            questions: Vec<GeneratedQuestion>,
        }
        let batch: Batch =
            serde_json::from_str(&output).map_err(|_| "Codex 返回的试卷 JSON 与展示格式不匹配")?;
        // Structural safety only: protect fixed UI indexing. No math/content
        // verification and no answer rewriting by hyz-things.
        if batch.questions.len() != QUIZ_PER_GROUP
            || batch
                .questions
                .iter()
                .any(|q| q.question.correct_index >= 4)
        {
            return Err("Codex 返回的试卷结构不完整（需要5题，每题4个选项）".into());
        }
        let returned: Vec<_> = batch
            .questions
            .iter()
            .map(|q| q.target_id.clone())
            .collect();
        if !target_ids_match(targets, &returned) {
            return Err("Codex 返回的 target_id 与本篇5张指定 Flashcard 不一致，请重试".into());
        }
        let mut questions = Vec::with_capacity(QUIZ_PER_GROUP);
        for generated in batch.questions {
            let target = targets
                .iter()
                .find(|t| t.target_id == generated.target_id)
                .expect("target IDs checked above");
            if generated.source_kind != target.card.source_kind.as_str()
                || generated.question.skill != target.card.category
            {
                return Err("Codex 返回的卡片分类或来源与指定目标不符，请重试".into());
            }
            let mut question = generated.question;
            // Trusted provenance comes from the frozen SQLite selection, not AI.
            question.skill = target.card.category.clone();
            question.card_id = Some(target.card.id.clone());
            question.card_path = Some(target.card.card_path.clone());
            question.source_kind = Some(target.card.source_kind.as_str().to_owned());
            question.source_hash = Some(target.card.source_hash.clone());
            question.related_card = Some(RelatedCardSnapshot {
                front: target.card.front_markdown.clone(),
                back: target.card.back_markdown.clone(),
            });
            questions.push(question);
        }
        let mut material = context;
        material.text = batch.material_text;
        Ok(QuestionGroup {
            version: 5,
            material,
            questions,
        })
    }
    async fn fetch_row(
        &self,
        id: &str,
    ) -> Result<(Vec<QuestionGroup>, Vec<Option<usize>>, String, Option<i64>), String> {
        let row=sqlx::query("SELECT groups_json,answers_json,status,started_at_ms FROM data_analysis_runs WHERE id=?")
            .bind(id).fetch_optional(&self.pool).await.map_err(as_error)?.ok_or("训练不存在")?;
        let groups =
            serde_json::from_str(&row.try_get::<String, _>("groups_json").map_err(as_error)?)
                .map_err(as_error)?;
        let answers =
            serde_json::from_str(&row.try_get::<String, _>("answers_json").map_err(as_error)?)
                .map_err(as_error)?;
        Ok((
            groups,
            answers,
            row.try_get("status").map_err(as_error)?,
            row.try_get("started_at_ms").map_err(as_error)?,
        ))
    }
    /// Create a fresh, untimed replay of a completed paper. Never touch the
    /// source record or invoke Codex, even when replaying a previous replay.
    pub async fn retry(&self, source_id: &str) -> Result<String, String> {
        let mut tx = self.pool.begin().await.map_err(as_error)?;
        let row = sqlx::query(
            "SELECT groups_json, answers_json, status, COALESCE(origin_run_id, id) AS root_id
             FROM data_analysis_runs WHERE id=?",
        )
        .bind(source_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(as_error)?
        .ok_or("历史训练不存在")?;
        let status: String = row.try_get("status").map_err(as_error)?;
        if status != "submitted" {
            return Err("只有已交卷的训练可以重做".into());
        }
        let groups_json: String = row.try_get("groups_json").map_err(as_error)?;
        let groups: Vec<QuestionGroup> = serde_json::from_str(&groups_json).map_err(as_error)?;
        if groups.len() != QUIZ_GROUPS || groups.iter().any(|g| g.questions.len() != QUIZ_PER_GROUP)
        {
            return Err("历史试卷题目不完整，无法重做".into());
        }
        let original_answers: Vec<Option<usize>> =
            serde_json::from_str(&row.try_get::<String, _>("answers_json").map_err(as_error)?)
                .map_err(as_error)?;
        if original_answers.len() != QUIZ_GROUPS * QUIZ_PER_GROUP {
            return Err("历史答题记录不完整，无法重做".into());
        }
        let root_id: String = row.try_get("root_id").map_err(as_error)?;
        let attempt_no: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(attempt_no), 1) + 1
             FROM data_analysis_runs WHERE id=? OR origin_run_id=?",
        )
        .bind(&root_id)
        .bind(&root_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(as_error)?;
        let id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO data_analysis_runs
             (id,groups_json,answers_json,status,created_at_ms,origin_run_id,attempt_no)
             VALUES (?,?,?,'ready',?,?,?)",
        )
        .bind(&id)
        // Keep old questions verbatim, but do not carry legacy Material fields
        // into a new attempt's persisted JSON.
        .bind(serde_json::to_string(&groups).map_err(as_error)?)
        .bind(
            serde_json::to_string(&vec![None::<usize>; QUIZ_GROUPS * QUIZ_PER_GROUP])
                .map_err(as_error)?,
        )
        .bind(now_ms())
        .bind(root_id)
        .bind(attempt_no)
        .execute(&mut *tx)
        .await
        .map_err(as_error)?;
        tx.commit().await.map_err(as_error)?;
        Ok(id)
    }

    /// One server-owned live position and a single random editing lease.
    /// Claim is an explicit takeover: all older tokens stop authorizing writes.
    pub async fn claim(&self, id: &str) -> Result<ClaimedRun, String> {
        let token = Uuid::now_v7().to_string();
        let updated = sqlx::query(
            "UPDATE data_analysis_runs
             SET controller_token=?, status='active',
                 started_at_ms=COALESCE(started_at_ms, ?)
             WHERE id=? AND status IN ('ready','active')",
        )
        .bind(&token)
        .bind(now_ms())
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(as_error)?;
        if updated.rows_affected() != 1 {
            return Err("训练已交卷或不存在，无法接管".into());
        }
        Ok(ClaimedRun {
            run: self.run(id).await?,
            control_token: token,
        })
    }

    /// Server discovery works across browsers/devices; never expose answer keys
    /// or control tokens. Ready runs stay untimed until explicitly claimed.
    pub async fn unfinished(&self) -> Result<Vec<UnfinishedRun>, String> {
        let rows = sqlx::query(
            "SELECT id,status,answers_json,current_question,started_at_ms,
                    paused_at_ms,paused_total_ms,created_at_ms
             FROM data_analysis_runs WHERE status IN ('ready','active')
             ORDER BY created_at_ms DESC LIMIT 30",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(as_error)?;
        let mut items = Vec::with_capacity(rows.len());
        for row in rows {
            let answers: Vec<Option<usize>> =
                serde_json::from_str(&row.try_get::<String, _>("answers_json").map_err(as_error)?)
                    .map_err(as_error)?;
            if answers.len() != QUIZ_GROUPS * QUIZ_PER_GROUP {
                continue;
            }
            let status: String = row.try_get("status").map_err(as_error)?;
            let start: Option<i64> = row.try_get("started_at_ms").map_err(as_error)?;
            let paused_at: Option<i64> = row.try_get("paused_at_ms").map_err(as_error)?;
            let paused_total: i64 = row.try_get("paused_total_ms").map_err(as_error)?;
            let deadline = start.map(|at| at + QUIZ_SECONDS as i64 * 1000 + paused_total);
            items.push(UnfinishedRun {
                id: row.try_get("id").map_err(as_error)?,
                status,
                answered: answers.iter().filter(|a| a.is_some()).count(),
                current_question: row
                    .try_get::<i64, _>("current_question")
                    .map_err(as_error)?
                    .clamp(0, 19) as usize,
                deadline_at_ms: if paused_at.is_none() { deadline } else { None },
                paused_remaining_ms: paused_at
                    .zip(deadline)
                    .map(|(since, end)| (end - since).max(0)),
                created_at_ms: row.try_get("created_at_ms").map_err(as_error)?,
            });
        }
        Ok(items)
    }

    async fn check_control(&self, id: &str, token: Option<&str>) -> Result<(), String> {
        let stored: Option<String> =
            sqlx::query_scalar("SELECT controller_token FROM data_analysis_runs WHERE id=?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await
                .map_err(as_error)?
                .ok_or("训练不存在")?;
        if stored.as_deref() != token {
            return Err("本次训练已在其他设备接管，请重新进入".into());
        }
        Ok(())
    }

    /// A position change cannot alter answers or active timer.
    pub async fn save_position(
        &self,
        id: &str,
        question: usize,
        token: &str,
    ) -> Result<(), String> {
        if question >= QUIZ_GROUPS * QUIZ_PER_GROUP {
            return Err("题号无效".into());
        }
        let updated = sqlx::query(
            "UPDATE data_analysis_runs SET current_question=?
             WHERE id=? AND status='active' AND controller_token=?",
        )
        .bind(question as i64)
        .bind(id)
        .bind(token)
        .execute(&self.pool)
        .await
        .map_err(as_error)?;
        if updated.rows_affected() == 0 {
            return Err("本次训练已在其他设备接管，请重新进入".into());
        }
        Ok(())
    }

    pub async fn run(&self, id: &str) -> Result<RunView, String> {
        sqlx::query("UPDATE data_analysis_runs SET status='active',started_at_ms=? WHERE id=? AND status='ready'")
            .bind(now_ms()).bind(id).execute(&self.pool).await.map_err(as_error)?;
        let (groups, answers, status, started_at_ms) = self.fetch_row(id).await?;
        let timing =
            sqlx::query("SELECT paused_at_ms,paused_total_ms FROM data_analysis_runs WHERE id=?")
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .map_err(as_error)?;
        let paused_at: Option<i64> = timing.try_get("paused_at_ms").map_err(as_error)?;
        let paused_total: i64 = timing.try_get("paused_total_ms").map_err(as_error)?;
        let deadline = started_at_ms.map(|ms| ms + QUIZ_SECONDS as i64 * 1000 + paused_total);
        let paused_remaining_ms = paused_at
            .zip(deadline)
            .map(|(since, until)| (until - since).max(0));
        Ok(RunView {
            id: id.into(),
            status,
            started_at_ms,
            deadline_at_ms: if paused_at.is_some() { None } else { deadline },
            paused_remaining_ms,
            server_now_ms: now_ms(),
            duration_seconds: QUIZ_SECONDS,
            groups: groups
                .into_iter()
                .map(|g| PublicGroup {
                    material: g.material,
                    questions: g
                        .questions
                        .into_iter()
                        .map(|q| PublicQuestion {
                            question: q.question,
                            options: q.options,
                        })
                        .collect(),
                })
                .collect(),
            answers,
            current_question: sqlx::query_scalar::<_, i64>(
                "SELECT current_question FROM data_analysis_runs WHERE id=?",
            )
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(as_error)?
            .clamp(0, 19) as usize,
        })
    }
    pub async fn answer(&self, id: &str, number: usize, choice: usize) -> Result<(), String> {
        self.answer_controlled(id, number, choice, None).await
    }

    pub async fn answer_controlled(
        &self,
        id: &str,
        number: usize,
        choice: usize,
        token: Option<&str>,
    ) -> Result<(), String> {
        if !(1..=QUIZ_GROUPS * QUIZ_PER_GROUP).contains(&number) || choice > 3 {
            return Err("题号或选项无效".into());
        }
        // A single SQL JSON update avoids lost answers from concurrent requests.
        // The token predicate also rejects a write racing with a new claim.
        let updated = sqlx::query(
            "UPDATE data_analysis_runs
             SET answers_json=json_set(answers_json, ?, ?)
             WHERE id=? AND status='active' AND paused_at_ms IS NULL
               AND started_at_ms IS NOT NULL
               AND (? < started_at_ms + ? + paused_total_ms)
               AND ((controller_token IS NULL AND ? IS NULL) OR controller_token=?)",
        )
        .bind(format!("$[{}]", number - 1))
        .bind(choice as i64)
        .bind(id)
        .bind(now_ms())
        .bind(QUIZ_SECONDS as i64 * 1000)
        .bind(token)
        .bind(token)
        .execute(&self.pool)
        .await
        .map_err(as_error)?;
        if updated.rows_affected() == 0 {
            return Err("训练已暂停、超时或控制权已移交".into());
        }
        Ok(())
    }

    pub async fn submit(&self, id: &str) -> Result<RunResult, String> {
        self.submit_controlled(id, None).await
    }
    pub async fn submit_controlled(
        &self,
        id: &str,
        token: Option<&str>,
    ) -> Result<RunResult, String> {
        // Serialize with answer writes; only answers frozen in this transaction
        // can contribute to card-level accuracy.
        let mut tx = self.pool.begin().await.map_err(as_error)?;
        // Acquire a write reservation before reading; prevents a concurrently
        // saved answer from being graded against a stale read snapshot.
        sqlx::query("UPDATE data_analysis_runs SET status=status WHERE id=?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(as_error)?;
        let row = sqlx::query(
            "SELECT groups_json,status,controller_token FROM data_analysis_runs WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(as_error)?
        .ok_or("训练不存在")?;
        let current_token: Option<String> = row.try_get("controller_token").map_err(as_error)?;
        if current_token.as_deref() != token {
            return Err("训练已由其他设备接管".into());
        }
        let status: String = row.try_get("status").map_err(as_error)?;
        let mut groups: Vec<QuestionGroup> =
            serde_json::from_str(&row.try_get::<String, _>("groups_json").map_err(as_error)?)
                .map_err(as_error)?;
        if status != "submitted" {
            if status != "active" {
                return Err("试卷尚未开始".into());
            }
            let updated = sqlx::query(
                "UPDATE data_analysis_runs SET status='submitted',ended_at_ms=?
                 WHERE id=? AND status='active'
                   AND ((controller_token IS NULL AND ? IS NULL) OR controller_token=?)",
            )
            .bind(now_ms())
            .bind(id)
            .bind(token)
            .bind(token)
            .execute(&mut *tx)
            .await
            .map_err(as_error)?;
            if updated.rows_affected() != 1 {
                return Err("训练已由其他设备接管或已交卷".into());
            }
        }
        let raw: String =
            sqlx::query_scalar("SELECT answers_json FROM data_analysis_runs WHERE id=?")
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .map_err(as_error)?;
        let answers: Vec<Option<usize>> = serde_json::from_str(&raw).map_err(as_error)?;
        restore_related_cards(&mut tx, &mut groups).await?;
        let result = grade_run(id, groups, answers)?;
        if status != "submitted" {
            for q in &result.questions {
                sqlx::query(
                    "INSERT INTO data_analysis_results
                     (run_id,question_no,topic,skill,is_correct,elapsed_ms,card_id,was_answered)
                     VALUES (?,?,?,?,?,?,?,?)",
                )
                .bind(id)
                .bind(q.number as i64)
                .bind(&q.topic)
                .bind(&q.skill)
                .bind((q.chosen == Some(q.correct_index)) as i64)
                .bind(0i64)
                .bind(&q.card_id)
                .bind(q.chosen.is_some() as i64)
                .execute(&mut *tx)
                .await
                .map_err(as_error)?;
            }
        }
        tx.commit().await.map_err(as_error)?;
        Ok(result)
    }

    pub async fn pause_toggle(&self, id: &str) -> Result<RunView, String> {
        self.pause_toggle_controlled(id, None).await
    }
    pub async fn pause_toggle_controlled(
        &self,
        id: &str,
        token: Option<&str>,
    ) -> Result<RunView, String> {
        self.check_control(id, token).await?;
        let row=sqlx::query("SELECT status,started_at_ms,paused_at_ms,paused_total_ms FROM data_analysis_runs WHERE id=?")
            .bind(id).fetch_optional(&self.pool).await.map_err(as_error)?.ok_or("训练不存在")?;
        let status: String = row.try_get("status").map_err(as_error)?;
        let started: Option<i64> = row.try_get("started_at_ms").map_err(as_error)?;
        let paused_at: Option<i64> = row.try_get("paused_at_ms").map_err(as_error)?;
        let paused_total: i64 = row.try_get("paused_total_ms").map_err(as_error)?;
        if status != "active" {
            return Err("只有进行中的训练可以暂停".into());
        }
        let now = now_ms();
        let result = if let Some(paused_at) = paused_at {
            sqlx::query("UPDATE data_analysis_runs SET paused_at_ms=NULL,paused_total_ms=paused_total_ms+? WHERE id=? AND status='active' AND paused_at_ms=? AND ((controller_token IS NULL AND ? IS NULL) OR controller_token=?)")
                .bind((now-paused_at).max(0)).bind(id).bind(paused_at).bind(token).bind(token)
                .execute(&self.pool).await.map_err(as_error)?
        } else {
            if started.is_none_or(|ms| now >= ms + QUIZ_SECONDS as i64 * 1000 + paused_total) {
                return Err("训练计时已结束，不能暂停".into());
            }
            sqlx::query("UPDATE data_analysis_runs SET paused_at_ms=? WHERE id=? AND status='active' AND paused_at_ms IS NULL AND ((controller_token IS NULL AND ? IS NULL) OR controller_token=?)")
                .bind(now).bind(id).bind(token).bind(token).execute(&self.pool).await.map_err(as_error)?
        };
        if result.rows_affected() != 1 {
            return Err("计时状态已变化，请刷新".into());
        }
        self.run(id).await
    }

    pub async fn result(&self, id: &str) -> Result<RunResult, String> {
        let (mut groups, answers, status, _started) = self.fetch_row(id).await?;
        if status != "submitted" {
            return Err("训练尚未交卷".into());
        }
        let mut connection = self.pool.acquire().await.map_err(as_error)?;
        restore_related_cards(&mut connection, &mut groups).await?;
        grade_run(id, groups, answers)
    }

    pub async fn history(&self) -> Result<Vec<ComprehensiveHistoryEntry>, String> {
        let rows=sqlx::query(
            "SELECT id,groups_json,answers_json,started_at_ms,ended_at_ms,paused_total_ms,attempt_no FROM data_analysis_runs WHERE status='submitted' AND ended_at_ms IS NOT NULL ORDER BY ended_at_ms DESC LIMIT 100"
        ).fetch_all(&self.pool).await.map_err(as_error)?;
        let mut result = Vec::new();
        for row in rows {
            let groups: Vec<QuestionGroup> =
                serde_json::from_str(&row.try_get::<String, _>("groups_json").map_err(as_error)?)
                    .map_err(as_error)?;
            let answers: Vec<Option<usize>> =
                serde_json::from_str(&row.try_get::<String, _>("answers_json").map_err(as_error)?)
                    .map_err(as_error)?;
            let total: usize = groups.iter().map(|g| g.questions.len()).sum();
            if answers.len() != total {
                continue;
            }
            let correct = groups
                .iter()
                .flat_map(|g| g.questions.iter())
                .zip(answers.iter())
                .filter(|(q, a)| **a == Some(q.correct_index))
                .count();
            let started: i64 = row
                .try_get::<Option<i64>, _>("started_at_ms")
                .map_err(as_error)?
                .unwrap_or(0);
            let ended: i64 = row.try_get("ended_at_ms").map_err(as_error)?;
            let paused_total: i64 = row.try_get("paused_total_ms").map_err(as_error)?;
            result.push(ComprehensiveHistoryEntry {
                id: row.try_get("id").map_err(as_error)?,
                attempt_no: row.try_get("attempt_no").map_err(as_error)?,
                started_at_ms: started,
                ended_at_ms: ended,
                elapsed_ms: (ended - started - paused_total).max(0),
                correct,
                total,
                topics: groups.iter().map(|g| g.material.topic.clone()).collect(),
            });
        }
        Ok(result)
    }
}

/// Shuffle equal-priority themes per newly created paper, while strongly
/// preferring topics not seen in the previous four original papers. Historical
/// replays are excluded by the server query. Sorting stays stable after shuffle
/// so equal reuse penalties retain a randomized order.
fn order_exam_themes(
    mut themes: Vec<Material>,
    recent_papers: &[Vec<String>],
    seed: u64,
) -> Vec<Material> {
    let mut state = seed | 1;
    for i in (1..themes.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        themes.swap(i, state as usize % (i + 1));
    }

    // Newest papers are penalized most. When four unseen themes exist, none
    // of the recently used topics can enter the initial four assignments.
    let mut penalties: HashMap<&str, usize> = HashMap::new();
    for (age, topics) in recent_papers.iter().take(RECENT_THEME_PAPERS).enumerate() {
        let weight = 1 << (RECENT_THEME_PAPERS - 1 - age);
        for topic in topics.iter().map(String::as_str).collect::<HashSet<_>>() {
            *penalties.entry(topic).or_default() += weight;
        }
    }
    themes.sort_by_key(|material| penalties.get(material.topic.as_str()).copied().unwrap_or(0));
    themes
}

/// Lightweight theme seeds only. No National Bureau of Statistics fetch is
/// needed: Codex researches actual exam-style passages before inventing a new
/// self-contained statistical dataset for each theme.
fn exam_theme_materials() -> Vec<Material> {
    TOPICS
        .iter()
        .map(|(topic, _)| Material {
            topic: (*topic).to_owned(),
            text: String::new(),
        })
        .collect()
}

fn knowledge_card_category(deck: &str) -> Option<&str> {
    let parts: Vec<_> = deck.split('/').filter(|s| !s.is_empty()).collect();
    let category = match parts.as_slice() {
        [skill] | ["资料", skill] | ["flashcards", "资料", skill] => *skill,
        _ => return None,
    };
    valid_wrong_category(category).then_some(category)
}

fn valid_wrong_category(category: &str) -> bool {
    let count = category.chars().count();
    count > 0
        && count <= 60
        && category != "."
        && category != ".."
        && !category.chars().any(char::is_control)
}

/// Accept the legacy 资料/错题/<skill> layout and the current
/// 资料/<skill>/错题 layout. Only verified product roots and skill segments pass.
fn wrong_question_source(deck: &str) -> Option<(&str, bool)> {
    let parts = deck
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let index = parts.iter().position(|part| *part == "错题")?;
    let prefix = &parts[..index];
    let (category, skill_precedes_wrong_folder) = match prefix {
        [] | ["资料"] | ["flashcards", "资料"] => (*parts.get(index + 1)?, false),
        ["资料", category] | ["flashcards", "资料", category] => (*category, true),
        _ => return None,
    };
    valid_wrong_category(category).then_some((category, skill_precedes_wrong_folder))
}

#[cfg(test)]
fn wrong_question_category(deck: &str) -> Option<&str> {
    wrong_question_source(deck).map(|(category, _)| category)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_exact_wrong_question_subdirectories_as_skills() {
        assert_eq!(
            wrong_question_category("资料/错题/乘积增长率"),
            Some("乘积增长率")
        );
        assert_eq!(
            wrong_question_category("资料/乘除变化/错题"),
            Some("乘除变化")
        );
        assert_eq!(
            wrong_question_category("flashcards/资料/线段混合/错题"),
            Some("线段混合")
        );
        assert_eq!(
            wrong_question_category("错题/混合增长率"),
            Some("混合增长率")
        );
        assert_eq!(
            wrong_question_category("flashcards/资料/错题/综合分析与审题/小结"),
            Some("综合分析与审题")
        );
        assert_eq!(
            wrong_question_category("资料/错题/平均数增量"),
            Some("平均数增量")
        );
        assert_eq!(wrong_question_category("资料/错题"), None);
        assert_eq!(wrong_question_category("资料/线段混合"), None);
        assert_eq!(wrong_question_category("其他/错题/乘积增长率"), None);
    }

    #[test]
    fn candidates_keep_the_verified_asset_path_for_each_wrong_question_layout() {
        let current = card_from_db(
            "00000000-0000-7000-8000-000000000001".into(),
            "/import/资料/乘除变化/错题/cards.md".into(),
            "资料/乘除变化/错题".into(),
            "Front".into(),
            "Back".into(),
            "hash".into(),
        )
        .unwrap();
        assert_eq!(current.category, "乘除变化");
        assert_eq!(current.card_path, "flashcards/资料/乘除变化/错题/cards.md");
        let legacy = card_from_db(
            "00000000-0000-7000-8000-000000000002".into(),
            "/import/资料/错题/乘除变化/cards.md".into(),
            "资料/错题/乘除变化".into(),
            "Front".into(),
            "Back".into(),
            "hash".into(),
        )
        .unwrap();
        assert_eq!(legacy.card_path, "flashcards/资料/错题/乘除变化/cards.md");
    }

    #[test]
    fn current_wrong_question_layout_can_fill_all_four_passages() {
        let data_categories = [
            ("乘除变化", 6, 9),
            ("假设代入思想", 1, 2),
            ("分数比较大小", 2, 4),
            ("增长贡献率和拉动增长率", 3, 1),
            ("年均变化", 7, 1),
            ("线段混合", 1, 2),
            ("计算方法及误差分析", 2, 9),
        ];
        let mut serial = 1;
        let mut cards = Vec::new();
        for (category, knowledge_count, wrong_count) in data_categories {
            for (deck, count) in [
                (format!("资料/{category}"), knowledge_count),
                (format!("资料/{category}/错题"), wrong_count),
            ] {
                for _ in 0..count {
                    cards.push(
                        card_from_db(
                            format!("00000000-0000-7000-8000-{serial:012x}"),
                            format!("/import/{deck}/cards.md"),
                            deck.clone(),
                            "Front".into(),
                            "Back".into(),
                            format!("hash-{serial}"),
                        )
                        .unwrap(),
                    );
                    serial += 1;
                }
            }
        }
        assert_eq!(cards.len(), 50);
        let plan = plan_question_targets(&cards, 1, &CardSchedule::default()).unwrap();
        assert!(plan
            .groups
            .iter()
            .all(|group| group.len() == QUIZ_PER_GROUP));
        assert!(plan.groups.iter().all(|group| {
            group
                .iter()
                .map(|target| target.card.category.as_str())
                .collect::<HashSet<_>>()
                .len()
                == QUIZ_PER_GROUP
        }));
    }

    fn candidate(category: &str, id: usize, source_kind: SourceKind) -> CardCandidate {
        CardCandidate {
            id: format!("{category}-{id}"),
            card_path: "flashcards/资料/test/card.md".into(),
            source_hash: "hash".into(),
            category: category.into(),
            source_kind,
            front_markdown: "Question".into(),
            back_markdown: "Answer".into(),
        }
    }
    fn ample_pool() -> Vec<CardCandidate> {
        (0..12)
            .flat_map(|i| {
                let cat = format!("category{i}");
                (0..5).map(move |n| {
                    candidate(
                        &cat,
                        n,
                        if n < 2 {
                            SourceKind::Wrong
                        } else {
                            SourceKind::Knowledge
                        },
                    )
                })
            })
            .collect()
    }
    #[test]
    fn global_schedule_selects_lowest_card_cost_without_type_quotas() {
        let cards = ample_pool();
        let mut schedule = CardSchedule::default();
        for card in &cards {
            schedule.cards.insert(card.id.clone(), 10_000);
        }
        for n in 0..4 {
            schedule.cards.insert(format!("category0-{n}"), n as i64);
        }
        let a = plan_question_targets(&cards, 123, &schedule).unwrap();
        let b = plan_question_targets(&cards, 123, &schedule).unwrap();
        validate_question_plan(&a).unwrap();
        assert_eq!(
            a.groups
                .iter()
                .flatten()
                .map(|t| &t.card.id)
                .collect::<Vec<_>>(),
            b.groups
                .iter()
                .flatten()
                .map(|t| &t.card.id)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            a.groups
                .iter()
                .flatten()
                .filter(|t| t.card.category == "category0")
                .count(),
            4
        );
        assert_eq!(
            a.groups
                .iter()
                .flatten()
                .filter(
                    |t| t.card.category == "category0" && t.card.source_kind == SourceKind::Wrong
                )
                .count(),
            2
        );
    }
    #[test]
    fn global_schedule_rejects_infeasible_pools() {
        let cards = (0..4)
            .flat_map(|i| {
                (0..10).map(move |n| candidate(&format!("cat{i}"), n, SourceKind::Knowledge))
            })
            .collect::<Vec<_>>();
        assert!(plan_question_targets(&cards, 1, &CardSchedule::default()).is_err());
        let cards = ample_pool();
        let plan = plan_question_targets(&cards, 123, &CardSchedule::default()).unwrap();
        assert_eq!(plan.groups.iter().map(Vec::len).sum::<usize>(), 20);
        assert!(plan.groups.iter().all(|g| g
            .iter()
            .map(|t| &t.card.category)
            .collect::<HashSet<_>>()
            .len()
            == 5));
    }
    #[test]
    fn weighted_vcnt_favors_weak_cards() {
        assert_eq!(delta_vcnt(0, 0), 1024);
        assert_eq!(delta_vcnt(0, 5), 838);
        assert_eq!(delta_vcnt(5, 5), 1317);
    }

    fn test_prepared_groups(plan: &QuestionPlan) -> Vec<QuestionGroup> {
        plan.groups
            .iter()
            .enumerate()
            .map(|(slot, targets)| QuestionGroup {
                version: 5,
                material: Material {
                    topic: format!("t{slot}"),
                    text: "统计材料".into(),
                },
                questions: targets
                    .iter()
                    .map(|target| VerifiedQuestion {
                        question: "模拟题".into(),
                        options: ["A".into(), "B".into(), "C".into(), "D".into()],
                        correct_index: 0,
                        answer: "A".into(),
                        explanation: "解析".into(),
                        skill: target.card.category.clone(),
                        evidence: "统计材料".into(),
                        card_id: Some(target.card.id.clone()),
                        card_path: Some(target.card.card_path.clone()),
                        source_kind: Some(target.card.source_kind.as_str().into()),
                        source_hash: Some(target.card.source_hash.clone()),
                        related_card: None,
                    })
                    .collect(),
            })
            .collect()
    }

    #[tokio::test]
    async fn original_paper_increases_vcnt_but_replay_does_not() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let service = DataAnalysisService::new(pool.clone(), None).unwrap();
        let cards = ample_pool();
        let schedule = service.load_card_schedule(&cards).await.unwrap();
        let plan = plan_question_targets(&cards, 9, &schedule).unwrap();
        let id = Uuid::now_v7().to_string();
        service
            .persist_original_run(&id, &test_prepared_groups(&plan), &plan)
            .await
            .unwrap();
        let chosen = &plan.groups[0][0].card.id;
        let row: (i64, i64) = sqlx::query_as(
            "SELECT selected_count,vcnt FROM data_analysis_card_usage WHERE card_id=?",
        )
        .bind(chosen)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row, (1, 1024));
        // A concurrently committed paper invalidates the frozen vcnt snapshot.
        let stale_run = Uuid::now_v7().to_string();
        assert!(service
            .persist_original_run(&stale_run, &test_prepared_groups(&plan), &plan)
            .await
            .is_err());

        sqlx::query("UPDATE data_analysis_runs SET status='submitted',ended_at_ms=? WHERE id=?")
            .bind(now_ms())
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
        let replay = service.retry(&id).await.unwrap();
        assert_ne!(replay, id);
        let after: (i64, i64) = sqlx::query_as(
            "SELECT selected_count,vcnt FROM data_analysis_card_usage WHERE card_id=?",
        )
        .bind(chosen)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(after, row);
        let mut bad = plan.clone();
        bad.groups[3][1].card.id = "missing-card".into();
        let other = Uuid::now_v7().to_string();
        assert!(service
            .persist_original_run(&other, &test_prepared_groups(&bad), &bad)
            .await
            .is_err());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM data_analysis_runs WHERE id=?")
            .bind(other)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
    #[tokio::test]
    async fn new_cards_inherit_global_minimum_and_keep_vcnt_after_type_move() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let service = DataAnalysisService::new(pool.clone(), None).unwrap();
        let mut cards = vec![
            candidate("a", 0, SourceKind::Knowledge),
            candidate("b", 0, SourceKind::Wrong),
        ];
        service.load_card_schedule(&cards).await.unwrap();
        sqlx::query("UPDATE data_analysis_card_usage SET vcnt=5000 WHERE card_id='a-0'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE data_analysis_card_usage SET vcnt=6000 WHERE card_id='b-0'")
            .execute(&pool)
            .await
            .unwrap();
        cards.push(candidate("c", 0, SourceKind::Knowledge));
        cards[0].category = "c".into();
        let snapshot = service.load_card_schedule(&cards).await.unwrap();
        assert_eq!(snapshot.card("c-0"), 5000);
        assert_eq!(snapshot.card("a-0"), 5000);
    }
    #[tokio::test]
    async fn only_answered_submitted_questions_affect_card_weight() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let service = DataAnalysisService::new(pool.clone(), None).unwrap();
        let cards = ample_pool();
        service.load_card_schedule(&cards).await.unwrap();
        let plan = plan_question_targets(&cards, 9, &CardSchedule::default()).unwrap();
        let groups = test_prepared_groups(&plan);
        let id = Uuid::now_v7().to_string();
        service
            .persist_original_run(&id, &groups, &plan)
            .await
            .unwrap();
        sqlx::query("UPDATE data_analysis_runs SET status='active',answers_json=json_set(answers_json,'$[0]',1) WHERE id=?")
            .bind(&id).execute(&pool).await.unwrap();
        service.submit(&id).await.unwrap();
        service.submit(&id).await.unwrap();
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM data_analysis_results WHERE run_id=? AND was_answered=1",
        )
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows, 1);
        let changed = service.load_card_schedule(&cards).await.unwrap();
        assert_eq!(
            changed.increment(&plan.groups[0][0].card.id),
            delta_vcnt(0, 1)
        );
        assert_eq!(changed.increment(&plan.groups[0][1].card.id), 1024);
        let replay = service.retry(&id).await.unwrap();
        sqlx::query("UPDATE data_analysis_runs SET status='active',answers_json=json_set(answers_json,'$[0]',1) WHERE id=?")
            .bind(&replay).execute(&pool).await.unwrap();
        service.submit(&replay).await.unwrap();
        let after = service.load_card_schedule(&cards).await.unwrap();
        assert_eq!(
            after.increment(&plan.groups[0][0].card.id),
            delta_vcnt(0, 2)
        );
    }

    #[tokio::test]
    async fn all_card_statistics_use_real_vcnt_and_include_unanswered() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let service = DataAnalysisService::new(pool.clone(), None).unwrap();
        let cards = ample_pool();
        for card in &cards {
            sqlx::query(
                "INSERT INTO flashcards
                 (id,source_file,source_order,deck_path,front_markdown,back_markdown,
                  source_hash,active,created_at_ms,updated_at_ms)
                 VALUES (?,?,0,?,?,?,?,1,0,0)",
            )
            .bind(&card.id)
            .bind(&card.card_path)
            .bind(&card.category)
            .bind(&card.front_markdown)
            .bind(&card.back_markdown)
            .bind(&card.source_hash)
            .execute(&pool)
            .await
            .unwrap();
        }
        let schedule = service.load_card_schedule(&cards).await.unwrap();
        let plan = plan_question_targets(&cards, 33, &schedule).unwrap();
        let run_id = Uuid::now_v7().to_string();
        service
            .persist_original_run(&run_id, &test_prepared_groups(&plan), &plan)
            .await
            .unwrap();
        // Twelve answers: two correct, ten wrong, eight blank.
        let mut answers = vec![None::<usize>; 20];
        for (index, answer) in answers.iter_mut().enumerate().take(12) {
            *answer = Some(if index < 2 { 0 } else { 1 });
        }
        sqlx::query("UPDATE data_analysis_runs SET status='active', answers_json=? WHERE id=?")
            .bind(serde_json::to_string(&answers).unwrap())
            .bind(&run_id)
            .execute(&pool)
            .await
            .unwrap();
        service.submit(&run_id).await.unwrap();
        let (summary, ordered) = service.card_performance().await.unwrap();
        assert_eq!(summary.answered_count, 12);
        assert_eq!(summary.correct_count, 2);
        // All 60 enrolled cards, including the 40 never selected, are present.
        assert_eq!(ordered.len(), cards.len());
        assert_eq!(
            ordered
                .iter()
                .filter(|card| card.answered_count == 0)
                .count(),
            48
        );
        assert_eq!(
            ordered
                .iter()
                .filter(|card| card.answered_count == 1)
                .count(),
            12
        );
        assert!(ordered.iter().all(|card| !card.front_markdown.is_empty()));
        // 40 cards still have vcnt=0, while 20 selected cards have vcnt=1024.
        // Order must reflect the actual counter, NOT the accuracy or category.
        let actual_vcnt: Vec<i64> = sqlx::query_scalar(
            "SELECT vcnt FROM data_analysis_card_usage ORDER BY vcnt ASC,card_id ASC",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(actual_vcnt.len(), cards.len());
        assert_eq!(actual_vcnt[..40], [0; 40]);
        assert_eq!(actual_vcnt[40..], [1024; 20]);
        let expected_ids: Vec<String> = sqlx::query_scalar(
            "SELECT card_id FROM data_analysis_card_usage ORDER BY vcnt ASC,card_id ASC",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            ordered
                .iter()
                .map(|card| card.card_id.as_str())
                .collect::<Vec<_>>(),
            expected_ids.iter().map(String::as_str).collect::<Vec<_>>()
        );
        let replay_id = service.retry(&run_id).await.unwrap();
        let mut replay_answers = vec![None::<usize>; 20];
        replay_answers[0] = Some(1);
        sqlx::query("UPDATE data_analysis_runs SET status='active', answers_json=? WHERE id=?")
            .bind(serde_json::to_string(&replay_answers).unwrap())
            .bind(&replay_id)
            .execute(&pool)
            .await
            .unwrap();
        service.submit(&replay_id).await.unwrap();
        let (replay_summary, _) = service.card_performance().await.unwrap();
        assert_eq!(replay_summary.answered_count, 13);
        assert_eq!(replay_summary.correct_count, 2);
    }

    #[test]
    fn knowledge_decks_exclude_wrong_questions_and_unrelated_modules() {
        assert_eq!(knowledge_card_category("资料/年均变化"), Some("年均变化"));
        assert_eq!(
            knowledge_card_category("flashcards/资料/线段混合"),
            Some("线段混合")
        );
        assert_eq!(knowledge_card_category("乘除变化"), Some("乘除变化"));
        assert_eq!(knowledge_card_category("资料/错题/年均变化"), None);
        assert_eq!(knowledge_card_category("判断/论证推理"), None);
        assert_eq!(knowledge_card_category("资料/images"), Some("images"));
    }

    #[test]
    fn env_accepts_non_hex_token_without_a_fixed_length() {
        let value = "short-token-12";
        let config = parse_env(&format!(
            "HYZ_THINGS_CODEX_WS_TOKEN={value}\nHYZ_THINGS_CODEX_CWD=/home/hyz/hyz_things"
        ))
        .unwrap()
        .unwrap();
        assert_eq!(config.token, value);
    }

    #[test]
    fn default_user_prompt_contains_optional_judgment_question_preference() {
        // An opinionated default, not an immutable source, quota, or schema rule.
        assert!(DEFAULT_USER_PROMPT.contains("下列说法正确的是"));
        assert!(DEFAULT_USER_PROMPT.contains("根据资料可以推出的是"));
        assert!(DEFAULT_USER_PROMPT.contains("综合分析与审题"));
        assert!(DEFAULT_USER_PROMPT.contains("不出"));
    }

    #[test]
    fn final_codex_answers_deserialize_without_backend_recalculation() {
        // Deliberately mathematically arbitrary. The app consumes the model's
        // selected option rather than a Rust formula/answer builder.
        let item = serde_json::json!({
            "question":"2024年比2023年增长约多少？",
            "options":["17%","19%","21%","23%"],
            "correct_index":2,
            "answer":"21%",
            "explanation":"按照本题对应 hyz 卡片的速算策略判断。",
            "skill":"间隔增长率",
            "evidence":"2023年增长10%，2024年增长10%"
        });
        let q: VerifiedQuestion = serde_json::from_value(item).unwrap();
        assert_eq!(q.correct_index, 2);
        assert_eq!(q.answer, "21%");
        assert_eq!(q.skill, "间隔增长率");
    }

    #[test]
    fn token_is_the_only_supported_model_credential() {
        let sample = format!(
            "HYZ_THINGS_AI_BASE_URL=https://api.deepseek.com\nHYZ_THINGS_AI_API_KEY=legacy\nHYZ_THINGS_AI_MODEL=legacy\nHYZ_THINGS_CODEX_WS_TOKEN={}\nHYZ_THINGS_CODEX_CWD=/home/hyz/hyz_things\n",
            "a".repeat(64)
        );
        let config = parse_env(&sample).unwrap().unwrap();
        assert_eq!(config.token.len(), 64);
        assert_eq!(config.cwd, "/home/hyz/hyz_things");
        assert!(parse_env("HYZ_THINGS_AI_BASE_URL=https://api.deepseek.com")
            .unwrap()
            .is_none());
        assert!(parse_env(
            "HYZ_THINGS_CODEX_WS_TOKEN=bad token\nHYZ_THINGS_CODEX_CWD=/home/user/repo"
        )
        .is_err());
        assert!(parse_env("HYZ_THINGS_CODEX_WS_TOKEN=short-token-12").is_err());
        assert!(parse_env("HYZ_THINGS_CODEX_CWD=/home/hyz/hyz_things").is_err());
        assert!(!valid_remote_repo_path("hyz-things"));
        assert!(!valid_remote_repo_path("/home/../etc"));
        assert!(!valid_remote_repo_path("/home/hyz\nhead"));
        assert!(valid_remote_repo_path("/home/hyz/hyz_things"));
        assert!(parse_env(&format!(
            "HYZ_THINGS_CODEX_WS_TOKEN={0}\nHYZ_THINGS_CODEX_WS_TOKEN={0}\nHYZ_THINGS_CODEX_CWD=/home/hyz/hyz_things",
            "a".repeat(64)
        ))
        .is_err());
    }
    #[test]
    fn timeout_detection_only_marks_deadline_errors_as_fatal() {
        assert!(is_generation_timeout("连接 Codex App Server 超时"));
        assert!(is_generation_timeout(
            "Codex 出题超时，请检查电脑服务与账号额度"
        ));
        assert!(!is_generation_timeout("Codex 返回的试卷结构不完整"));
        assert!(!is_generation_timeout(
            "Codex 生成失败或中断（可能登录失效或额度不足）"
        ));
    }

    fn generation_candidates(count: usize) -> Vec<(usize, Material)> {
        (0..count)
            .map(|slot| {
                (
                    slot,
                    Material {
                        topic: format!("topic-{slot}"),
                        text: "test".into(),
                    },
                )
            })
            .collect()
    }

    #[tokio::test]
    async fn one_group_timeout_cancels_peers_before_returning() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::sync::Barrier;

        struct DropMarker(Arc<AtomicUsize>);
        impl Drop for DropMarker {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let dropped = Arc::new(AtomicUsize::new(0));
        let cancelled = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(Barrier::new(MAX_GENERATION_CONCURRENCY));
        let candidates = generation_candidates(QUIZ_GROUPS);
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            run_generation_wave(candidates, {
                let dropped = Arc::clone(&dropped);
                let cancelled = Arc::clone(&cancelled);
                let started = Arc::clone(&started);
                move |_material, slot, cancellation| {
                    let dropped = Arc::clone(&dropped);
                    let cancelled = Arc::clone(&cancelled);
                    let started = Arc::clone(&started);
                    async move {
                        let _marker = DropMarker(dropped);
                        started.wait().await;
                        if slot == 0 {
                            Err("Codex 出题超时，请检查电脑服务与账号额度".into())
                        } else {
                            let mut signal = cancellation.subscribe();
                            let _ = signal.wait_for(|value| *value).await;
                            cancelled.fetch_add(1, Ordering::SeqCst);
                            Err("Codex 出题已取消".into())
                        }
                    }
                }
            }),
        )
        .await
        .expect("a timeout should end the wave without waiting for other workers");
        assert!(matches!(result, Err(ref error) if error.contains("超时")));
        assert_eq!(
            cancelled.load(Ordering::SeqCst),
            1,
            "peer cleanup must finish"
        );
        assert_eq!(dropped.load(Ordering::SeqCst), MAX_GENERATION_CONCURRENCY);
    }

    #[tokio::test]
    async fn batch_timeout_reports_unconfirmed_peer_cleanup() {
        for timeout in [
            "连接 Codex App Server 超时",
            "Codex 出题超时，请检查电脑服务与账号额度",
        ] {
            let started = Arc::new(tokio::sync::Barrier::new(2));
            let candidates = generation_candidates(2);
            let error = run_generation_wave(candidates, move |_material, slot, cancellation| {
                let started = Arc::clone(&started);
                async move {
                    started.wait().await;
                    if slot == 0 {
                        Err(timeout.into())
                    } else {
                        let mut signal = cancellation.subscribe();
                        let _ = signal.wait_for(|value| *value).await;
                        Err(format!(
                            "{CANCELLED_MESSAGE}；未确认 Codex 远端已停止，请检查电脑服务"
                        ))
                    }
                }
            })
            .await
            .err()
            .expect("batch should time out");
            assert!(error.starts_with(timeout));
            assert!(error.contains("未确认 Codex 远端已停止"), "{error}");
        }
    }

    #[tokio::test]
    async fn parallel_wave_runs_at_most_two_sessions_and_keeps_source_slots() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::sync::Barrier;

        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(MAX_GENERATION_CONCURRENCY + 1));
        let candidates = generation_candidates(QUIZ_GROUPS);
        let task = tokio::spawn(run_generation_wave(candidates, {
            let active = Arc::clone(&active);
            let peak = Arc::clone(&peak);
            let barrier = Arc::clone(&barrier);
            move |material, _ordinal, _cancellation| {
                let active = Arc::clone(&active);
                let peak = Arc::clone(&peak);
                let barrier = Arc::clone(&barrier);
                async move {
                    let running = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(running, Ordering::SeqCst);
                    barrier.wait().await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    if material.topic == "topic-2" {
                        Err("simulate invalid Codex response".into())
                    } else {
                        Ok(QuestionGroup {
                            version: 2,
                            material,
                            questions: Vec::new(),
                        })
                    }
                }
            }
        }));
        for _ in (0..QUIZ_GROUPS).step_by(MAX_GENERATION_CONCURRENCY) {
            tokio::time::timeout(Duration::from_secs(4), barrier.wait())
                .await
                .expect("each pair of workers should run concurrently");
        }
        let mut outcomes = tokio::time::timeout(Duration::from_secs(4), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        outcomes.sort_by_key(|(slot, _)| *slot);
        assert_eq!(peak.load(Ordering::SeqCst), 2);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(outcomes.len(), QUIZ_GROUPS);
        for (slot, item) in outcomes {
            if slot == 2 {
                assert!(item.is_err());
            } else {
                assert_eq!(item.unwrap().material.topic, format!("topic-{slot}"));
            }
        }
    }

    #[test]
    fn all_knowledge_cards_are_valid_global_candidates() {
        let cards = (0..5)
            .flat_map(|i| {
                (0..4).map(move |n| candidate(&format!("cat{i}"), n, SourceKind::Knowledge))
            })
            .collect::<Vec<_>>();
        let plan = plan_question_targets(&cards, 4123, &CardSchedule::default()).unwrap();
        validate_question_plan(&plan).unwrap();
    }

    #[tokio::test]
    async fn user_prompt_is_shared_by_sqlite_and_default_is_restorable() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let ipad = DataAnalysisService::new(pool.clone(), None).unwrap();
        let phone = DataAnalysisService::new(pool, None).unwrap();
        let initial = ipad.user_prompt_settings().await.unwrap();
        assert!(!initial.customized);
        assert_eq!(initial.user_prompt, DEFAULT_USER_PROMPT);
        let customized = ipad
            .save_user_prompt(Some("只出带合理干扰项的真实模拟数据题。"))
            .await
            .unwrap();
        assert!(customized.customized);
        assert_eq!(
            phone.user_prompt_settings().await.unwrap().user_prompt,
            "只出带合理干扰项的真实模拟数据题。"
        );
        assert!(phone.save_user_prompt(Some(" ")).await.is_err());
        assert!(phone
            .save_user_prompt(Some(&"x".repeat(8001)))
            .await
            .is_err());
        assert!(phone.save_user_prompt(Some("提示\0注入")).await.is_err());
        assert_eq!(
            phone.user_prompt_settings().await.unwrap().user_prompt,
            "只出带合理干扰项的真实模拟数据题。"
        );
        let restored = phone.save_user_prompt(None).await.unwrap();
        assert!(!restored.customized);
        assert_eq!(
            ipad.user_prompt_settings().await.unwrap().user_prompt,
            DEFAULT_USER_PROMPT
        );
    }

    #[tokio::test]
    async fn submitted_claimed_paper_history_is_read_only_without_controller_token() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let group = QuestionGroup {
            version: 4,
            material: Material {
                topic: "测试主题".into(),
                text: "测试统计材料".into(),
            },
            questions: (0..QUIZ_PER_GROUP)
                .map(|index| VerifiedQuestion {
                    question: format!("第{}题", index + 1),
                    options: ["A".into(), "B".into(), "C".into(), "D".into()],
                    correct_index: 1,
                    answer: "B".into(),
                    explanation: "历史解析".into(),
                    skill: "基期量".into(),
                    evidence: "原始统计数据".into(),
                    card_id: None,
                    card_path: None,
                    source_kind: None,
                    source_hash: None,
                    related_card: None,
                })
                .collect(),
        };
        let id = Uuid::now_v7().to_string();
        // Simulate JSON created before section/subtopic/source_url removal.
        let mut archived = serde_json::to_value(vec![group; QUIZ_GROUPS]).unwrap();
        for group in archived.as_array_mut().unwrap() {
            group["material"]["section"] = serde_json::json!("测试章节");
            group["material"]["subtopic"] = serde_json::json!("测试子主题");
            group["material"]["source_url"] = serde_json::json!("practice-test");
        }
        sqlx::query(
            "INSERT INTO data_analysis_runs (id,groups_json,answers_json,status,created_at_ms)
             VALUES (?,?,?,'ready',?)",
        )
        .bind(&id)
        .bind(archived.to_string())
        .bind(serde_json::to_string(&vec![None::<usize>; QUIZ_GROUPS * QUIZ_PER_GROUP]).unwrap())
        .bind(now_ms())
        .execute(&pool)
        .await
        .unwrap();

        let service = DataAnalysisService::new(pool.clone(), None).unwrap();
        let claimed = service.claim(&id).await.unwrap();
        assert_eq!(claimed.run.groups[0].material.topic, "测试主题");
        assert_eq!(claimed.run.groups[0].material.text, "测试统计材料");
        assert_eq!(
            serde_json::to_value(&claimed.run.groups[0].material)
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            2
        );
        assert!(
            service.result(&id).await.is_err(),
            "active answers stay hidden"
        );
        service
            .answer_controlled(&id, 1, 1, Some(&claimed.control_token))
            .await
            .unwrap();
        let submitted = service
            .submit_controlled(&id, Some(&claimed.control_token))
            .await
            .unwrap();
        assert_eq!(submitted.correct, 1);
        let before: (String, String, i64) = sqlx::query_as(
            "SELECT status,controller_token,ended_at_ms FROM data_analysis_runs WHERE id=?",
        )
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let history = service.history().await.unwrap();
        assert!(history.iter().any(|entry| entry.id == id));
        let reviewed = service.result(&id).await.expect("read submitted history");
        assert_eq!(reviewed.correct, submitted.correct);
        assert_eq!(reviewed.questions.len(), QUIZ_GROUPS * QUIZ_PER_GROUP);
        assert_eq!(reviewed.questions[0].explanation, "历史解析");
        let after: (String, String, i64) = sqlx::query_as(
            "SELECT status,controller_token,ended_at_ms FROM data_analysis_runs WHERE id=?",
        )
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(before, after, "history read must not update submitted run");

        let redo_id = service.retry(&id).await.unwrap();
        let copied_json: String =
            sqlx::query_scalar("SELECT groups_json FROM data_analysis_runs WHERE id=?")
                .bind(&redo_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!copied_json.contains("source_url"));
        assert!(!copied_json.contains("subtopic"));
        assert!(!copied_json.contains("section"));
        let redo = service.run(&redo_id).await.unwrap();
        assert_eq!(redo.groups[0].material.topic, "测试主题");
        assert_eq!(redo.groups[0].questions.len(), QUIZ_PER_GROUP);
    }

    #[tokio::test]
    async fn saved_groups_support_history_without_new_codex_generation() {
        // Exercise native SQLite/service paths without a paid AI API or external network.
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("temporary in-memory sqlite");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");

        let columns = sqlx::query("PRAGMA table_info(data_analysis_groups)")
            .fetch_all(&pool)
            .await
            .unwrap();
        let columns = columns
            .iter()
            .map(|row| row.try_get::<String, _>("name").unwrap())
            .collect::<HashSet<_>>();
        assert!(columns.contains("topic"));
        assert!(columns.contains("group_json"));
        assert!(!columns.contains("source_url"));
        assert!(!columns.contains("section"));

        let materials = TOPICS
            .iter()
            .take(4)
            .map(|(topic, _)| Material {
                topic: (*topic).into(),
                text: "本年度业务量1200亿元，比上年增长15.2%。".repeat(8),
            })
            .collect::<Vec<_>>();

        for material in &materials {
            let questions = (0..5)
                .map(|idx| VerifiedQuestion {
                    question: format!("第{}题：上一年业务量约为多少？", idx + 1),
                    options: [
                        "1000亿元".into(),
                        "1042亿元".into(),
                        "1200亿元".into(),
                        "1400亿元".into(),
                    ],
                    correct_index: 1,
                    answer: "1042亿元".into(),
                    explanation: "用现期除以1.152".into(),
                    skill: "基期量".into(),
                    evidence: "本年度业务量1200亿元，比上年增长15.2%。".into(),
                    card_id: None,
                    card_path: None,
                    source_kind: None,
                    source_hash: None,
                    related_card: None,
                })
                .collect::<Vec<_>>();
            let group = QuestionGroup {
                version: 4,
                material: material.clone(),
                questions,
            };
            sqlx::query("INSERT INTO data_analysis_groups(id,topic,group_json,created_at_ms) VALUES(?,?,?,?)")
                .bind(Uuid::now_v7().to_string())
                .bind(&material.topic)
                .bind(serde_json::to_string(&group).unwrap())
                .bind(now_ms())
                .execute(&pool).await.unwrap();
        }

        let config = parse_env(&format!(
            "HYZ_THINGS_CODEX_WS_TOKEN={}\nHYZ_THINGS_CODEX_CWD=/home/hyz/hyz_things",
            "a".repeat(64)
        ))
        .unwrap()
        .unwrap();
        // Previously saved groups are still readable in history, but a new
        // generation must no longer short-circuit to cached group JSON.
        let historical_rows = sqlx::query("SELECT group_json FROM data_analysis_groups")
            .fetch_all(&pool)
            .await
            .unwrap();
        let archived_groups = historical_rows
            .into_iter()
            .map(|row| {
                serde_json::from_str::<QuestionGroup>(
                    &row.try_get::<String, _>("group_json").unwrap(),
                )
                .unwrap()
            })
            .collect::<Vec<_>>();
        let run_id = Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO data_analysis_runs(id,groups_json,answers_json,status,created_at_ms) VALUES(?,?,?,'ready',?)")
            .bind(&run_id)
            .bind(serde_json::to_string(&archived_groups).unwrap())
            .bind(serde_json::to_string(&vec![None::<usize>;20]).unwrap())
            .bind(now_ms()).execute(&pool).await.unwrap();
        let service = DataAnalysisService::new(pool, Some(config)).unwrap();
        let active = service.run(&run_id).await.expect("start 20-minute timer");
        assert_eq!(active.groups.len(), 4);
        assert!(active.groups.iter().all(|group| group.questions.len() == 5));
        assert_eq!(active.duration_seconds, 1200);
        assert!(active.deadline_at_ms.unwrap() > active.started_at_ms.unwrap());
        assert_eq!(active.answers.len(), 20);
        let paused = service.pause_toggle(&run_id).await.expect("pause timer");
        assert!(paused.paused_remaining_ms.is_some());
        assert!(paused.deadline_at_ms.is_none());
        assert!(
            service.answer(&run_id, 1, 1).await.is_err(),
            "pausing blocks answers"
        );
        let reloaded = service.run(&run_id).await.expect("paused reload");
        assert!(reloaded.paused_remaining_ms.is_some());
        let resumed = service.pause_toggle(&run_id).await.expect("resume timer");
        assert!(resumed.paused_remaining_ms.is_none());
        assert!(resumed.deadline_at_ms.is_some());
        service.answer(&run_id, 1, 1).await.expect("save an answer");
        let refreshed = service.run(&run_id).await.expect("restore an active run");
        assert_eq!(refreshed.answers[0], Some(1));
        assert_eq!(refreshed.started_at_ms, active.started_at_ms);
        let result = service.submit(&run_id).await.expect("submit");
        assert_eq!(result.correct, 1);
        assert_eq!(result.total, 20);
        assert_eq!(result.questions.len(), 20);
        assert!(
            service.answer(&run_id, 2, 1).await.is_err(),
            "submitted run must reject answers"
        );
        let replay = service.submit(&run_id).await.expect("idempotent result");
        assert_eq!(replay.correct, 1);
        let entries = service.history().await.expect("history listing");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].correct, 1);
        assert_eq!(entries[0].total, 20);
        // Repeating a completed paper produces a new blank, separately graded
        // attempt with exactly the same material, questions and options.
        let repeat_id = service.retry(&run_id).await.expect("retry finished paper");
        assert_ne!(repeat_id, run_id);
        let repeated = service.run(&repeat_id).await.expect("start repeated paper");
        assert_eq!(repeated.answers, vec![None; 20]);
        assert_eq!(repeated.groups[0].material, active.groups[0].material);
        assert_eq!(
            repeated.groups[0].questions[0].question,
            active.groups[0].questions[0].question
        );
        assert_eq!(
            repeated.groups[0].questions[0].options,
            active.groups[0].questions[0].options
        );
        assert_eq!(repeated.duration_seconds, QUIZ_SECONDS);
        assert!(
            service.retry(&repeat_id).await.is_err(),
            "active repeat cannot be cloned"
        );
        service
            .answer(&repeat_id, 1, 0)
            .await
            .expect("repeat answer");
        let repeat_result = service.submit(&repeat_id).await.expect("submit repeat");
        assert_eq!(repeat_result.correct, 0);
        assert_eq!(service.result(&run_id).await.unwrap().correct, 1);
        let third_id = service.retry(&repeat_id).await.expect("repeat from repeat");
        let third = service.run(&third_id).await.unwrap();
        assert_eq!(third.answers, vec![None; 20]);
        let before = service.unfinished().await.expect("server discovery");
        assert!(before.iter().any(|item| item.id == third_id));
        let ipad = service.claim(&third_id).await.expect("iPad claim");
        assert_eq!(ipad.run.current_question, 0);
        service
            .save_position(&third_id, 7, &ipad.control_token)
            .await
            .expect("save server position");
        service
            .answer_controlled(&third_id, 1, 1, Some(&ipad.control_token))
            .await
            .expect("iPad answer");
        let phone = service.claim(&third_id).await.expect("phone takeover");
        assert_ne!(phone.control_token, ipad.control_token);
        assert_eq!(phone.run.current_question, 7);
        assert_eq!(phone.run.answers[0], Some(1));
        assert!(
            service
                .save_position(&third_id, 8, &ipad.control_token)
                .await
                .is_err(),
            "stale iPad position rejected"
        );
        assert!(
            service
                .answer_controlled(&third_id, 2, 1, Some(&ipad.control_token))
                .await
                .is_err(),
            "stale iPad answer rejected"
        );
        assert!(
            service
                .pause_toggle_controlled(&third_id, Some(&ipad.control_token))
                .await
                .is_err(),
            "stale iPad pause rejected"
        );
        assert!(
            service
                .submit_controlled(&third_id, Some(&ipad.control_token))
                .await
                .is_err(),
            "stale iPad submit rejected"
        );
        let paused_phone = service
            .pause_toggle_controlled(&third_id, Some(&phone.control_token))
            .await
            .expect("phone pause");
        assert!(paused_phone.paused_remaining_ms.is_some());
        let on_other_device = service.unfinished().await.unwrap();
        let pending = on_other_device
            .iter()
            .find(|item| item.id == third_id)
            .unwrap();
        assert_eq!(pending.current_question, 7);
        assert_eq!(pending.answered, 1);
        assert!(pending.paused_remaining_ms.is_some());
        assert!(
            service
                .answer_controlled(&third_id, 2, 1, Some(&phone.control_token))
                .await
                .is_err(),
            "paused paper cannot accept answers"
        );
        let after_reload = service
            .claim(&third_id)
            .await
            .expect("same paper after reload");
        assert!(after_reload.run.paused_remaining_ms.is_some());
        assert_eq!(after_reload.run.current_question, 7);
        assert_eq!(after_reload.run.answers[0], Some(1));
        let resumed = service
            .pause_toggle_controlled(&third_id, Some(&after_reload.control_token))
            .await
            .expect("continue on phone");
        assert!(resumed.paused_remaining_ms.is_none());
        service
            .answer_controlled(&third_id, 2, 1, Some(&after_reload.control_token))
            .await
            .expect("new lease can answer");
        assert!(
            service.claim(&run_id).await.is_err(),
            "submitted paper cannot be reclaimed"
        );
        assert!(service.result(&third_id).await.is_err());
        let attempt_entries = service.history().await.unwrap();
        assert_eq!(
            attempt_entries
                .iter()
                .find(|entry| entry.id == run_id)
                .unwrap()
                .attempt_no,
            1
        );
        assert_eq!(
            attempt_entries
                .iter()
                .find(|entry| entry.id == repeat_id)
                .unwrap()
                .attempt_no,
            2
        );
        let historical = service.result(&run_id).await.expect("historical review");
        assert_eq!(historical.materials.len(), 4);
        assert_eq!(historical.questions.len(), 20);
        let (source_count, group_count) = service.counts().await.unwrap();
        // Newly generated material is self-contained: no official-source cache.
        assert_eq!((source_count, group_count), (0, 4));
    }

    #[test]
    fn themes_change_by_paper_and_prefer_unused_recent_topics() {
        let themes = exam_theme_materials();
        let first = order_exam_themes(themes.clone(), &[], 123456789);
        let another = order_exam_themes(themes.clone(), &[], 987654321);
        assert_ne!(
            first
                .iter()
                .take(QUIZ_GROUPS)
                .map(|m| &m.topic)
                .collect::<Vec<_>>(),
            another
                .iter()
                .take(QUIZ_GROUPS)
                .map(|m| &m.topic)
                .collect::<Vec<_>>(),
            "different generation IDs should produce different theme choices"
        );
        let previous = first
            .iter()
            .take(QUIZ_GROUPS)
            .map(|m| m.topic.clone())
            .collect::<Vec<_>>();
        let second = order_exam_themes(themes.clone(), std::slice::from_ref(&previous), 20261023);
        assert!(second
            .iter()
            .take(QUIZ_GROUPS)
            .all(|m| !previous.contains(&m.topic)));
        let second_topics = second
            .iter()
            .take(QUIZ_GROUPS)
            .map(|m| m.topic.clone())
            .collect::<Vec<_>>();
        let third = order_exam_themes(themes, &[second_topics.clone(), previous.clone()], 20261024);
        assert!(third
            .iter()
            .take(QUIZ_GROUPS)
            .all(|m| !second_topics.contains(&m.topic) && !previous.contains(&m.topic)));
        assert_eq!(
            third
                .iter()
                .take(QUIZ_GROUPS)
                .map(|m| m.topic.as_str())
                .collect::<HashSet<_>>()
                .len(),
            QUIZ_GROUPS
        );
        assert_eq!(
            order_exam_themes(exam_theme_materials(), std::slice::from_ref(&previous), 42)
                .iter()
                .map(|m| &m.topic)
                .collect::<Vec<_>>(),
            order_exam_themes(exam_theme_materials(), &[previous], 42)
                .iter()
                .map(|m| &m.topic)
                .collect::<Vec<_>>(),
            "retries within one preparation must keep the chosen order"
        );
    }

    #[tokio::test]
    async fn recent_theme_history_excludes_replays_and_includes_unfinished_papers() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let mut root_id = String::new();
        for (index, topic) in ["农业", "工业能源", "国内消费"].iter().enumerate() {
            let id = Uuid::now_v7().to_string();
            if index == 0 {
                root_id = id.clone();
            }
            let group = QuestionGroup {
                version: 4,
                material: Material {
                    topic: (*topic).to_owned(),
                    text: "模拟材料".into(),
                },
                questions: Vec::new(),
            };
            sqlx::query(
                "INSERT INTO data_analysis_runs
                 (id,groups_json,answers_json,status,created_at_ms)
                 VALUES(?,?,?,'ready',?)",
            )
            .bind(id)
            .bind(serde_json::to_string(&vec![group; QUIZ_GROUPS]).unwrap())
            .bind("[]")
            .bind(index as i64 + 1)
            .execute(&pool)
            .await
            .unwrap();
        }
        let replay = QuestionGroup {
            version: 4,
            material: Material {
                topic: "虚假的最近主题".into(),
                text: "模拟材料".into(),
            },
            questions: Vec::new(),
        };
        sqlx::query(
            "INSERT INTO data_analysis_runs
             (id,groups_json,answers_json,status,created_at_ms,origin_run_id)
             VALUES(?,?,?,'ready',?,?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(serde_json::to_string(&vec![replay; QUIZ_GROUPS]).unwrap())
        .bind("[]")
        .bind(99_i64)
        .bind(root_id)
        .execute(&pool)
        .await
        .unwrap();

        let service = DataAnalysisService::new(pool, None).unwrap();
        let recent = service.recent_paper_themes().await.unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0], vec!["国内消费"; QUIZ_GROUPS]);
        assert_eq!(recent[1], vec!["工业能源"; QUIZ_GROUPS]);
        assert_eq!(recent[2], vec!["农业"; QUIZ_GROUPS]);
        assert!(!recent
            .iter()
            .flatten()
            .any(|topic| topic == "虚假的最近主题"));
    }

    #[test]
    fn topic_seeds_do_not_depend_on_official_articles() {
        let topics = exam_theme_materials();
        assert_eq!(topics.len(), TOPICS.len());
        assert!(topics.iter().all(|m| m.text.is_empty()));
        let chosen = select_material_batch(&topics, &[], &HashSet::new(), QUIZ_GROUPS);
        assert_eq!(chosen.len(), QUIZ_GROUPS);
        assert_eq!(
            chosen
                .iter()
                .map(|m| m.topic.as_str())
                .collect::<HashSet<_>>()
                .len(),
            QUIZ_GROUPS
        );
    }
}
