//! Browse original papers and categories; share one workspace for practice and review.
use super::favorite_flashcard::{favorite_can_create, FavoriteFlashcardWorkspace, FavoritePreview};
use super::material_scroll::{scroll_edges, MaterialScrollButton, ScrollEdges};
use super::*;
use hyz_things::domain::question_bank::{
    primary_module, AiReviewState, CreatePracticeRequest, DrillCollection, FavoriteState,
    ModulePracticeFilters, PaperSummary, PracticeBrowseItem, PracticeCatalog, PracticeMode,
    PracticeQuestion, PracticeReview, PracticeSession, PracticeWorkspace, SavePracticeAnswer,
    SetFavorite, SetPracticePaused,
};

#[wasm_bindgen::prelude::wasm_bindgen(module = "/src/web/review_scroll.js")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = restoreReviewScroll)]
    fn restore_review_scroll(element: &HtmlElement, offset: i32) -> js_sys::Function;
}

#[wasm_bindgen::prelude::wasm_bindgen(module = "/src/web/ai_diagram.js")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = renderAiDiagrams)]
    fn render_ai_diagrams(root: &HtmlElement);
}

#[derive(serde::Serialize)]
struct AiReviewStartRequest {
    request_id: String,
    instruction: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attachments: Vec<String>,
}

#[derive(serde::Serialize)]
struct AiReviewFollowUpRequest {
    request_id: String,
    message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    attachments: Vec<String>,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
struct AiRequestInfo {
    request_id: String,
    intent: String,
    status: String,
    error: Option<String>,
    remote_uncertain: bool,
}

#[derive(serde::Deserialize)]
struct AiRequestReceipt {
    #[serde(flatten)]
    state: AiReviewState,
    request: AiRequestInfo,
}

#[derive(serde::Serialize)]
struct AiDeleteRequest {
    confirm_uncertain: bool,
}

#[derive(serde::Serialize)]
struct AiRetryRequest {
    request_id: String,
    confirm_uncertain: bool,
    confirm_new_thread: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    replacement_attachments: Option<Vec<String>>,
}

fn definite_ai_rejection(error: &ApiError) -> bool {
    matches!(
        error.code(),
        Some(
            "question_bank_ai_invalid"
                | "question_bank_ai_busy"
                | "question_bank_ai_request_conflict"
                | "question_bank_ai_request_not_found"
                | "question_bank_ai_attachment_expired"
                | "question_bank_ai_confirmation_required"
        )
    )
}

fn remember_ai_request(key: &str, id: &str) -> Result<(), String> {
    let storage = web_sys::window()
        .and_then(|window| window.session_storage().ok().flatten())
        .ok_or("浏览器会话存储不可用，无法安全记录请求；未发送。")?;
    storage
        .set_item(key, id)
        .map_err(|_| "无法保存请求标识；未发送。".to_owned())
}

fn new_ai_request_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    web_sys::window()
        .ok_or("浏览器不可用")?
        .crypto()
        .map_err(|_| "无法获取安全随机数")?
        .get_random_values_with_u8_array(&mut bytes)
        .map_err(|_| "无法获取安全随机数")?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15]
    ))
}

const ROOT: &str = "/api/v1/question-bank";

#[derive(Default)]
struct ReviewScrollMemory {
    session: Option<String>,
    question: Option<String>,
    positions: std::collections::BTreeMap<String, (i32, i32)>,
}

#[derive(serde::Deserialize)]
struct DataVersion {
    version: String,
}

fn module_filter_labels(filters: &ModulePracticeFilters) -> [String; 3] {
    let years = match (filters.year_min, filters.year_max) {
        (None, None) => "全部年份".into(),
        (Some(min), Some(max)) => format!("{min}—{max} 年"),
        (Some(min), None) => format!("{min} 年以后"),
        (None, Some(max)) => format!("{max} 年以前"),
    };
    let regions = if filters.regions.is_empty() {
        "不限地区".into()
    } else if filters.exclude_regions {
        format!("屏蔽 {}", filters.regions.join("、"))
    } else {
        filters.regions.join("、")
    };
    [
        years,
        regions,
        format!("每轮 {} 题", filters.count.unwrap_or(20)),
    ]
}

fn paper_catalog_label(papers: &[PaperSummary]) -> String {
    let mut years = papers
        .iter()
        .filter(|paper| paper.role == "primary")
        .map(|paper| paper.year);
    let Some(first) = years.next() else {
        return "0 份原卷".into();
    };
    let (min, max, count) = years.fold((first, first, 1), |(min, max, count), year| {
        (min.min(year), max.max(year), count + 1)
    });
    format!("{min}—{max} · {count} 份原卷")
}

fn history_mode_label(mode: &PracticeMode) -> &'static str {
    match mode {
        PracticeMode::Paper | PracticeMode::PaperModule => "套卷训练",
        PracticeMode::Module => "模块训练",
        PracticeMode::Wrong => "错题复练",
        PracticeMode::Favorite => "收藏练习",
        PracticeMode::Drill => "专项练习",
    }
}

fn question_scope(question: Option<&PracticeQuestion>) -> Option<String> {
    let view = question?.question.as_ref()?;
    let module = primary_module(view.module.as_deref())
        .map(str::to_owned)
        .or_else(|| view.module.clone())?;
    Some(
        match view.category.as_deref().filter(|value| !value.is_empty()) {
            Some(category) => format!("{module} · {category}"),
            None => module,
        },
    )
}

fn navigation_groups(questions: &[PracticeQuestion], mode: &PracticeMode) -> Vec<NavigationGroup> {
    let mut groups: Vec<NavigationGroup> = Vec::new();
    for (index, question) in questions.iter().enumerate() {
        let module = primary_module(
            question
                .question
                .as_ref()
                .and_then(|view| view.module.as_deref()),
        );
        let (label, subtitle) = if *mode == PracticeMode::Drill {
            (question.paper_title.clone(), module.map(str::to_owned))
        } else {
            (module.unwrap_or("未分类").to_owned(), None)
        };
        match groups.last_mut() {
            Some(group) if group.label == label => group.end = index + 1,
            _ => groups.push(NavigationGroup {
                label,
                subtitle,
                start: index,
                end: index + 1,
            }),
        }
    }
    groups
}

fn history_filter_matches(run: &PracticeSession, filter: Option<&PracticeMode>) -> bool {
    match filter {
        None => true,
        Some(PracticeMode::Paper) => {
            matches!(run.mode, PracticeMode::Paper | PracticeMode::PaperModule)
        }
        Some(PracticeMode::Module) => run.mode == PracticeMode::Module,
        Some(PracticeMode::Drill) => run.mode == PracticeMode::Drill,
        Some(mode) => run.mode == *mode,
    }
}

fn history_matches_entry(entry: &PracticeMode, run: &PracticeSession) -> bool {
    let paper_run = matches!(run.mode, PracticeMode::Paper | PracticeMode::PaperModule)
        || matches!(run.mode, PracticeMode::Wrong | PracticeMode::Favorite)
            && run.paper_id.is_some()
            && run.collection_id.is_none();
    match entry {
        PracticeMode::Paper => paper_run,
        PracticeMode::Drill => run.mode == PracticeMode::Drill,
        _ => !paper_run && run.mode != PracticeMode::Drill,
    }
}
pub(super) const MODULE_SETTINGS_URL: &str = "/api/v1/question-bank/module-settings";

pub(super) fn module_filter_query(filters: &ModulePracticeFilters) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    if let Some(year) = filters.year_min {
        query.append_pair("year_min", &year.to_string());
    }
    if let Some(year) = filters.year_max {
        query.append_pair("year_max", &year.to_string());
    }
    if !filters.regions.is_empty() {
        query.append_pair("regions", &filters.regions.join(","));
        if filters.exclude_regions {
            query.append_pair("exclude_regions", "true");
        }
    }
    query.finish()
}

fn catalog_filter_url(filters: &ModulePracticeFilters) -> String {
    format!("{ROOT}/practice/catalog?{}", module_filter_query(filters))
}

fn browse_filter_url(
    scope: &PracticeMode,
    entry_mode: &PracticeMode,
    paper_id: &str,
    module: &str,
    category: &str,
    filters: &ModulePracticeFilters,
) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair(
        "mode",
        match scope {
            PracticeMode::Paper => "paper",
            PracticeMode::Module => "module",
            PracticeMode::Wrong => "wrong",
            PracticeMode::Favorite => "favorite",
            PracticeMode::PaperModule => "paper_module",
            PracticeMode::Drill => "drill",
        },
    );
    if *entry_mode == PracticeMode::Paper {
        if !paper_id.is_empty() {
            query.append_pair("paper_id", paper_id);
        }
    } else {
        if !module.is_empty() {
            query.append_pair("module", module);
        }
        if !category.is_empty() {
            query.append_pair("category", category);
        }
        if let Some(year) = filters.year_min {
            query.append_pair("year_min", &year.to_string());
        }
        if let Some(year) = filters.year_max {
            query.append_pair("year_max", &year.to_string());
        }
        if !filters.regions.is_empty() {
            query.append_pair("regions", &filters.regions.join(","));
            if filters.exclude_regions {
                query.append_pair("exclude_regions", "true");
            }
        }
    }
    format!("{ROOT}/practice/questions?{}", query.finish())
}

const WORKSPACE: &str = "question-bank-workspace training-workspace fixed inset-0 z-[70] flex h-dvh w-screen flex-col overflow-hidden bg-base-200 px-3 py-2";

#[derive(Properties, PartialEq)]
pub(crate) struct QuestionBankTrainingProps {
    pub csrf: String,
    pub entry_mode: PracticeMode,
    #[prop_or(true)]
    pub active: bool,
    pub on_immersive_change: Callback<bool>,
    pub on_paper_pause_change: Callback<bool>,
    #[prop_or_default]
    pub show_history: bool,
}

#[derive(Clone)]
enum Action {
    Start,
    Continue(String),
    Repeat(String),
    Review(String),
    Answer(String),
    Pause,
    Exit,
    Submit,
    Favorite(String),
}

#[derive(Clone, PartialEq)]
struct CollectionReviewTarget {
    scope: PracticeMode,
    items: Vec<PracticeBrowseItem>,
    index: usize,
}

#[derive(Clone, PartialEq)]
struct PendingAnswer {
    session_id: String,
    question_id: String,
    answer: Vec<String>,
}

struct PracticeCollectionCache {
    items: UseStateHandle<Option<Vec<PracticeBrowseItem>>>,
    loading: UseStateHandle<bool>,
    failure: UseStateHandle<Option<String>>,
    invalidate: Callback<()>,
}

#[hook]
fn use_practice_collection(
    scope: PracticeMode,
    refresh: u64,
    ready: bool,
) -> PracticeCollectionCache {
    let items = use_state(|| None::<Vec<PracticeBrowseItem>>);
    let loading = use_state(|| true);
    let failure = use_state(|| None::<String>);
    let revision = use_state(|| 0u64);
    let generation = use_mut_ref(|| 0u64);
    {
        let items = items.clone();
        let loading = loading.clone();
        let failure = failure.clone();
        let generation = generation.clone();
        use_effect_with(
            (scope, refresh, *revision, ready),
            move |(scope, _, _, ready)| {
                *generation.borrow_mut() += 1;
                let request_generation = *generation.borrow();
                let response_generation = generation.clone();
                let ready = *ready;
                let url = browse_filter_url(
                    scope,
                    &PracticeMode::Paper,
                    "",
                    "",
                    "",
                    &ModulePracticeFilters::default(),
                );
                loading.set(true);
                failure.set(None);
                spawn_local(async move {
                    if !ready {
                        return;
                    }
                    let result = fetch_json::<Vec<PracticeBrowseItem>>(&url, "真题分类浏览").await;
                    if *response_generation.borrow() != request_generation {
                        return;
                    }
                    match result {
                        Ok(result) => items.set(Some(result)),
                        Err(message) => failure.set(Some(message)),
                    }
                    loading.set(false);
                });
                move || *generation.borrow_mut() += 1
            },
        );
    }
    let invalidate = {
        let loading = loading.clone();
        let failure = failure.clone();
        Callback::from(move |()| {
            *generation.borrow_mut() += 1;
            loading.set(true);
            failure.set(None);
            revision.set(*generation.borrow());
        })
    };
    PracticeCollectionCache {
        items,
        loading,
        failure,
        invalidate,
    }
}

#[derive(Clone)]
pub(super) struct PracticeFavorite {
    pub(super) value: UseStateHandle<Option<FavoriteState>>,
    pub(super) failure: UseStateHandle<Option<(String, String)>>,
    pub(super) ready: Rc<RefCell<Option<FavoriteState>>>,
    pub(super) source: Rc<RefCell<Option<String>>>,
    pub(super) generation: Rc<RefCell<u64>>,
    pub(super) retry: Callback<()>,
}

#[hook]
pub(super) fn use_practice_favorite(question_id: Option<String>) -> PracticeFavorite {
    let value = use_state(|| None::<FavoriteState>);
    let failure = use_state(|| None::<(String, String)>);
    let ready = use_mut_ref(|| None::<FavoriteState>);
    let source = use_mut_ref(|| None::<String>);
    let reading = use_mut_ref(|| false);
    let generation = use_mut_ref(|| 0u64);
    let revision = use_state(|| 0u64);
    if *source.borrow() != question_id {
        *ready.borrow_mut() = None;
    }
    *source.borrow_mut() = question_id.clone();
    {
        let value = value.clone();
        let failure = failure.clone();
        let ready = ready.clone();
        let source = source.clone();
        let reading = reading.clone();
        let generation = generation.clone();
        use_effect_with((question_id, *revision), move |(question_id, _)| {
            *generation.borrow_mut() += 1;
            let request_generation = *generation.borrow();
            *reading.borrow_mut() = question_id.is_some();
            *ready.borrow_mut() = None;
            value.set(None);
            failure.set(None);
            if let Some(id) = question_id.clone() {
                let response_generation = generation.clone();
                let response_reading = reading.clone();
                let response_ready = ready.clone();
                spawn_local(async move {
                    let result = fetch_json::<FavoriteState>(
                        &format!("{ROOT}/practice/favorites/{id}"),
                        "真题收藏",
                    )
                    .await;
                    if *response_generation.borrow() != request_generation
                        || source.borrow().as_ref() != Some(&id)
                    {
                        return;
                    }
                    let result = result.and_then(|state| {
                        if state.question_id == id {
                            Ok(state)
                        } else {
                            Err("收藏状态响应不匹配，请重试".into())
                        }
                    });
                    match result {
                        Ok(state) => {
                            *response_ready.borrow_mut() = Some(state.clone());
                            value.set(Some(state));
                        }
                        Err(message) => failure.set(Some((id, message))),
                    }
                    *response_reading.borrow_mut() = false;
                });
            }
            move || {
                *generation.borrow_mut() += 1;
                *reading.borrow_mut() = false;
                *ready.borrow_mut() = None;
            }
        });
    }
    let retry = {
        let failure = failure.clone();
        let source = source.clone();
        let generation = generation.clone();
        Callback::from(move |()| {
            if *reading.borrow() || source.borrow().is_none() {
                return;
            }
            *reading.borrow_mut() = true;
            *generation.borrow_mut() += 1;
            failure.set(None);
            revision.set(*generation.borrow());
        })
    };
    PracticeFavorite {
        value,
        failure,
        ready,
        source,
        generation,
        retry,
    }
}

async fn load_workspace(id: &str) -> Result<PracticeWorkspace, String> {
    fetch_json(&format!("{ROOT}/practice/sessions/{id}"), "真题练习").await
}

pub(super) fn answer_options(markdown: &str) -> Vec<(String, String)> {
    markdown
        .lines()
        .filter_map(|line| {
            let line = line.trim_start().strip_prefix("- ")?;
            let (label, rest) = line.split_once(['.', '．', '、', ' '])?;
            (label.len() == 1 && label.bytes().all(|b| b.is_ascii_uppercase()))
                .then(|| (label.to_string(), rest.trim().to_string()))
        })
        .collect()
}

#[derive(Properties, PartialEq)]
struct QuestionBankScrollButtonsProps {
    passage: NodeRef,
    can_scroll_up: bool,
    can_scroll_down: bool,
    question_index: usize,
    target: &'static str,
    label: &'static str,
}

/// Scroll arrows belong to the scratchpad tools, matching the other drills.
#[function_component(QuestionBankScrollButtons)]
fn question_bank_scroll_buttons(props: &QuestionBankScrollButtonsProps) -> Html {
    let scratchpad_open = use_context::<bool>().unwrap_or(false);
    if !scratchpad_open {
        return Html::default();
    }
    html! {
        <div class="flex shrink-0 gap-1" role="group" aria-label={props.label}>
            <MaterialScrollButton passage={props.passage.clone()} up={true}
                disabled={!props.can_scroll_up} question_index={props.question_index} target={props.target} />
            <MaterialScrollButton passage={props.passage.clone()} up={false}
                disabled={!props.can_scroll_down} question_index={props.question_index} target={props.target} />
        </div>
    }
}

pub(super) const QUESTION_BANK_PANE_HEADER: &str =
    "relative z-30 flex min-h-14 shrink-0 items-center justify-between gap-2 px-3 py-2";

#[derive(Properties, PartialEq)]
pub(super) struct QuestionBankQuestionHeaderProps {
    /// Left-side action such as the favorite star.
    #[prop_or_default]
    pub(super) leading: Html,
    #[prop_or_default]
    pub(super) tabs: Html,
    pub(super) title: String,
    #[prop_or_default]
    pub(super) subtitle: Option<String>,
    pub(super) title_test_id: &'static str,
    #[prop_or_default]
    pub(super) passage: Option<NodeRef>,
    #[prop_or_default]
    pub(super) can_scroll_up: bool,
    #[prop_or_default]
    pub(super) can_scroll_down: bool,
    pub(super) question_index: usize,
    pub(super) target: &'static str,
    pub(super) label: &'static str,
}

#[function_component(QuestionBankQuestionHeader)]
pub(super) fn question_bank_question_header(props: &QuestionBankQuestionHeaderProps) -> Html {
    let scratch_open = use_context::<bool>().unwrap_or(false);
    html! {
        <div class={classes!(QUESTION_BANK_PANE_HEADER,"question-bank-pane-header")}>
            <div class="min-w-0 flex-1">
                <strong class="block truncate text-sm" data-testid={props.title_test_id}>{props.title.clone()}</strong>
                if let Some(subtitle) = props.subtitle.as_ref().filter(|value| !value.is_empty()) {
                    <span class="block truncate text-[11px] text-base-content/45">{subtitle.clone()}</span>
                }
            </div>
            <div class="ml-auto flex shrink-0 items-center gap-0">
                if scratch_open {
                    if let Some(passage) = &props.passage {
                        <QuestionBankScrollButtons passage={passage.clone()} can_scroll_up={props.can_scroll_up}
                            can_scroll_down={props.can_scroll_down} question_index={props.question_index} target={props.target} label={props.label}/>
                    }
                    <QuestionInkControls />
                } else {
                    {props.leading.clone()}{props.tabs.clone()}
                }
                <QuestionInkToggle />
            </div>
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub(super) struct QuestionAiHeaderActionsProps {
    #[prop_or(true)]
    pub available: bool,
    pub active: bool,
    pub can_delete: bool,
    pub original: Callback<MouseEvent>,
    pub ai: Callback<MouseEvent>,
    pub delete: Callback<MouseEvent>,
}

#[function_component(QuestionAiHeaderActions)]
pub(super) fn question_ai_header_actions(props: &QuestionAiHeaderActionsProps) -> Html {
    let open = use_state(|| false);
    {
        let open = open.clone();
        use_effect_with(props.active, move |_| {
            open.set(false);
            || ()
        });
    }
    let delete = {
        let open = open.clone();
        let callback = props.delete.clone();
        Callback::from(move |event: MouseEvent| {
            open.set(false);
            callback.emit(event);
        })
    };
    html! {
        <div class="flex shrink-0 items-center gap-0" data-testid="question-bank-review-tabs">
            if props.available {
            <button type="button" class={classes!(super::question_header_actions::QUESTION_HEADER_ICON_BUTTON,if !props.active {"btn-active"} else {""})} aria-label="原始解析" title="原始解析"
                data-testid="question-bank-original-tab" aria-pressed={(!props.active).to_string()} onclick={props.original.clone()}>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 3h9l4 4v14H6zM14 3v5h5M9 12h7M9 16h7"/></svg>
            </button>
            <button type="button" class={classes!(super::question_header_actions::QUESTION_HEADER_ICON_BUTTON,if props.active {"btn-active"} else {""},if props.active {"text-primary"} else {""})} aria-label="AI 解析" title="AI 解析"
                data-testid="question-bank-ai-tab" aria-pressed={props.active.to_string()} onclick={props.ai.clone()}>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m12 3 1.5 4.5L18 9l-4.5 1.5L12 15l-1.5-4.5L6 9l4.5-1.5zM19 14l.8 2.2L22 17l-2.2.8L19 20l-.8-2.2L16 17l2.2-.8z"/></svg>
            </button>
            }
                <div class="relative">
                    <button type="button" class={super::question_header_actions::QUESTION_HEADER_ICON_BUTTON} aria-label="更多解析操作" title="更多解析操作" aria-expanded={open.to_string()} onclick={Callback::from({let open=open.clone();move |_|open.set(!*open)})}>
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="12" cy="5" r="1.5"/><circle cx="12" cy="12" r="1.5"/><circle cx="12" cy="19" r="1.5"/></svg>
                    </button>
                    if *open { <div class="absolute right-0 top-full z-40 min-w-36 rounded-box border border-base-content/10 bg-base-100 p-1 shadow-lg">
                        if props.available && props.active && props.can_delete {
                            <button type="button" class="btn btn-ghost btn-sm w-full justify-start text-error" data-testid="question-bank-ai-delete" onclick={delete}>{"删除 AI 解析"}</button>
                        } else {
                            <span class="block px-3 py-2 text-xs text-base-content/60">{"暂无可用操作"}</span>
                        }
                    </div> }
                </div>
        </div>
    }
}

#[derive(Clone, PartialEq)]
struct CollectionModule {
    name: String,
    count: usize,
    categories: Vec<(String, usize)>,
}

fn collection_modules(
    items: &[PracticeBrowseItem],
    catalog: Option<&PracticeCatalog>,
) -> Vec<CollectionModule> {
    let mut grouped = std::collections::BTreeMap::<String, CollectionModule>::new();
    for item in items {
        let name = item.module.clone().unwrap_or_else(|| "未分类".into());
        let entry = grouped
            .entry(name.clone())
            .or_insert_with(|| CollectionModule {
                name,
                count: 0,
                categories: Vec::new(),
            });
        entry.count += 1;
        if let Some(category) = item.category.as_ref().filter(|value| !value.is_empty()) {
            if let Some(existing) = entry
                .categories
                .iter_mut()
                .find(|(name, _)| name == category)
            {
                existing.1 += 1;
            } else {
                entry.categories.push((category.clone(), 1));
            }
        }
    }
    let mut modules = grouped.into_values().collect::<Vec<_>>();
    if let Some(catalog) = catalog {
        modules.sort_by_key(|module| {
            catalog
                .modules
                .iter()
                .position(|entry| entry.name == module.name)
                .unwrap_or(usize::MAX)
        });
    }
    modules
}

fn has_existing_ai_analysis(state: &AiReviewState) -> bool {
    state.status == "running"
        || state
            .messages
            .iter()
            .any(|message| message.role == "assistant" && !message.content.trim().is_empty())
}

#[derive(Properties, PartialEq)]
pub(crate) struct ReviewAnswerStatsProps {
    pub correct_answer: Vec<String>,
    pub answer: Option<Vec<String>>,
    pub correct_count: usize,
    pub wrong_count: usize,
    pub answer_test_id: &'static str,
    pub counts_test_id: &'static str,
}

#[function_component(ReviewAnswerStats)]
pub(crate) fn review_answer_stats(props: &ReviewAnswerStatsProps) -> Html {
    html! {
        <section class="rounded-box border border-base-content/15 p-3" aria-label="答案统计">
            if let Some(answer) = &props.answer {
                <p class="text-sm text-base-content/60">{format!("你的答案：{}", if answer.is_empty() {"未答".into()} else {answer.join("、")})}</p>
            }
            <p class="text-xs opacity-60">{"正确答案"}</p>
            <p class="font-semibold" data-testid={props.answer_test_id}>
                {if props.correct_answer.is_empty() {"暂无可靠答案".into()} else {props.correct_answer.join("、")}}
            </p>
            <p class="mt-2 text-sm text-base-content/60" data-testid={props.counts_test_id}
                data-correct={props.correct_count.to_string()} data-wrong={props.wrong_count.to_string()}>
                {format!("本题累计：做对 {} 次 · 做错 {} 次", props.correct_count, props.wrong_count)}
            </p>
        </section>
    }
}

#[hook]
pub(crate) fn use_question_ai_tab(
    question_id: Option<String>,
) -> (UseStateHandle<bool>, Rc<RefCell<bool>>) {
    let ai_tab = use_state(|| false);
    let chosen = use_mut_ref(|| false);
    {
        let ai_tab = ai_tab.clone();
        let chosen = chosen.clone();
        use_effect_with(question_id, move |id| {
            ai_tab.set(false);
            *chosen.borrow_mut() = false;
            let alive = Rc::new(Cell::new(true));
            if let Some(question_id) = id.clone() {
                let active = alive.clone();
                spawn_local(async move {
                    let endpoint = format!("{ROOT}/practice/questions/{question_id}/ai");
                    if let Ok(state) = fetch_json::<AiReviewState>(&endpoint, "AI 复盘记录").await
                    {
                        if active.get()
                            && !*chosen.borrow()
                            && state.question_id == question_id
                            && has_existing_ai_analysis(&state)
                        {
                            ai_tab.set(true);
                        }
                    }
                });
            }
            move || alive.set(false)
        });
    }
    (ai_tab, chosen)
}

fn streaming_markdown_complete(text: &str) -> bool {
    let fences = text
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            line.starts_with("```") || line.starts_with("~~~")
        })
        .count();
    let dollar_count = text.chars().filter(|ch| *ch == '$').count();
    fences % 2 == 0
        && dollar_count % 2 == 0
        && (!text.contains("<svg") || text.contains("</svg>"))
        && text.matches("\\(").count() == text.matches("\\)").count()
        && text.matches("\\[").count() == text.matches("\\]").count()
}

pub(super) fn ai_draft_key(question_id: &str, kind: &str) -> String {
    format!("hyz.ai-review.draft.{question_id}.{kind}")
}

fn ai_draft_read(key: &str) -> String {
    web_sys::window()
        .and_then(|window| window.session_storage().ok().flatten())
        .and_then(|storage| storage.get_item(key).ok().flatten())
        .unwrap_or_default()
}

pub(super) fn ai_draft_write(key: &str, value: &str) {
    if let Some(storage) =
        web_sys::window().and_then(|window| window.session_storage().ok().flatten())
    {
        if value.is_empty() {
            let _ = storage.remove_item(key);
        } else {
            let _ = storage.set_item(key, value);
        }
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct QuestionAiReviewProps {
    pub csrf: String,
    pub question_id: String,
    pub question_content: Html,
    pub question_scroll: NodeRef,
    pub on_question_scroll: Callback<Event>,
    #[prop_or_default]
    pub delete_signal: u32,
    #[prop_or_default]
    pub on_delete_availability: Callback<bool>,
    #[prop_or("analysis")]
    pub mode: &'static str,
}

/// The component is mounted only while the AI tab is visible; the native
/// backend owns its turns and never cancels on unmount or question navigation.
#[function_component(QuestionAiReview)]
pub(crate) fn question_ai_review(props: &QuestionAiReviewProps) -> Html {
    let state = use_state(|| None::<AiReviewState>);
    let history_generation = use_state(|| 0u64);
    {
        let can_delete = state
            .as_ref()
            .as_ref()
            .is_some_and(|value| value.status != "idle" || !value.messages.is_empty());
        let notify = props.on_delete_availability.clone();
        use_effect_with(can_delete, move |available| {
            notify.emit(*available);
            || ()
        });
    }
    let diagram_root = use_node_ref();
    let failure = use_state(|| None::<String>);
    let submitting = use_state(|| false);
    let follow_key = ai_draft_key(&props.question_id, "follow-up");
    let start_key = ai_draft_key(&props.question_id, "start");
    let follow_up = use_state(|| ai_draft_read(&follow_key));
    let start_instruction = use_state(|| ai_draft_read(&start_key));
    let request_key = ai_draft_key(&props.question_id, "request-id");
    let request_id = use_state(|| {
        let value = ai_draft_read(&request_key);
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    });
    let request_info = use_state(|| None::<AiRequestInfo>);
    let request_check = use_state(|| 0u64);
    let request_busy = use_mut_ref(|| false);
    let request_error = use_state(|| None::<String>);
    let request_not_found = use_state(|| false);
    let retry_open = use_state(|| false);
    let confirm_uncertain = use_state(|| false);
    let confirm_new_thread = use_state(|| false);
    let retry_expired = use_state(|| false);
    let retry_images = use_state(Vec::<PendingImage>::new);
    let retry_uploading = use_state(|| false);
    let pending_message = use_state(|| None::<String>);
    let messages_ref = props.question_scroll.clone();
    let follow_bottom = use_mut_ref(|| true);
    let new_content = use_state(|| false);
    let start_images = use_state(Vec::<PendingImage>::new);
    let follow_images = use_state(Vec::<PendingImage>::new);
    let uploading_start = use_state(|| false);
    let uploading_follow = use_state(|| false);
    let start_input_ref = use_node_ref();
    let follow_input_ref = use_node_ref();
    {
        let start_ref = start_input_ref.clone();
        let follow_ref = follow_input_ref.clone();
        let key = (
            (*start_instruction).clone(),
            (*follow_up).clone(),
            start_images.len(),
            follow_images.len(),
            state.as_ref().as_ref().map(|value| value.status.clone()),
        );
        use_effect_with(key, move |_| {
            for node in [&start_ref, &follow_ref] {
                if let Some(input) = node.cast::<web_sys::HtmlTextAreaElement>() {
                    let _ = input.style().set_property("height", "auto");
                    let _ = input.style().set_property(
                        "height",
                        &format!("{}px", input.scroll_height().clamp(24, 160)),
                    );
                }
            }
            || ()
        });
    }
    let deleting = use_state(|| false);
    let delete_confirmation_required = use_state(|| false);
    let confirm_delete_uncertain = use_state(|| false);
    let delete_error = use_state(|| None::<String>);
    {
        let previous = use_mut_ref(|| props.delete_signal);
        let deleting = deleting.clone();
        let delete_confirmation_required = delete_confirmation_required.clone();
        let confirm_delete_uncertain = confirm_delete_uncertain.clone();
        let delete_error = delete_error.clone();
        use_effect_with(props.delete_signal, move |signal| {
            if *signal != *previous.borrow() {
                delete_confirmation_required.set(false);
                confirm_delete_uncertain.set(false);
                delete_error.set(None);
                deleting.set(true);
            }
            *previous.borrow_mut() = *signal;
            || ()
        });
    }
    let revision = use_mut_ref(|| 0u64);
    let endpoint = format!("{ROOT}/practice/questions/{}/ai", props.question_id);
    {
        let state = state.clone();
        let failure = failure.clone();
        let endpoint = endpoint.clone();
        let revision = revision.clone();
        let key = props.question_id.clone();
        use_effect_with(key, move |_| {
            state.set(None);
            failure.set(None);
            let alive = std::rc::Rc::new(std::cell::Cell::new(true));
            let request_alive = alive.clone();
            let request_revision = *revision.borrow();
            spawn_local(async move {
                let response = fetch_json::<AiReviewState>(&endpoint, "AI 复盘记录").await;
                if request_alive.get() && *revision.borrow() == request_revision {
                    match response {
                        Ok(value) => state.set(Some(value)),
                        Err(error) => failure.set(Some(error)),
                    }
                }
            });
            move || alive.set(false)
        });
    }
    let (stream, disconnected, reconnect_stream) = super::ai_review_stream::use_ai_review_stream(
        props.question_id.clone(),
        endpoint.clone(),
        state.clone(),
        revision.clone(),
    );
    {
        let request_info = request_info.clone();
        let request_error = request_error.clone();
        let request_not_found = request_not_found.clone();
        let state = state.clone();
        let endpoint = endpoint.clone();
        let key = (
            (*request_id).clone(),
            (*stream).as_ref().map(|value| value.status.clone()),
            *request_check,
            *submitting,
        );
        use_effect_with(key, move |(id, _, _, busy)| {
            let alive = Rc::new(Cell::new(true));
            if let Some(id) = id.clone().filter(|_| !*busy) {
                let alive = alive.clone();
                let request_info = request_info.clone();
                let request_error = request_error.clone();
                let request_not_found = request_not_found.clone();
                let state = state.clone();
                spawn_local(async move {
                    match fetch_json_typed::<AiRequestInfo>(
                        &format!("{endpoint}/requests/{id}"),
                        "AI 请求状态",
                    )
                    .await
                    {
                        Ok(info) if alive.get() => {
                            request_not_found.set(false);
                            request_error.set(None);
                            request_info.set(Some(info));
                        }
                        Err(error)
                            if alive.get()
                                && error.is_code("question_bank_ai_request_not_found") =>
                        {
                            // Do not discard a missing ID on the strength of one 404: a
                            // server task may still be active after a lost response.
                            let review =
                                fetch_json::<AiReviewState>(&endpoint, "AI 复盘记录").await;
                            if alive.get() {
                                request_info.set(None);
                                match review {
                                    Ok(value) => {
                                        state.set(Some(value));
                                        request_not_found.set(true);
                                        request_error.set(Some("服务端不存在这条 AI 请求记录，已重新核对题目状态。若决定放弃追踪原请求，可手动清除失效标识；不会自动重发。".into()));
                                    }
                                    Err(review_error) => {
                                        request_not_found.set(false);
                                        request_error.set(Some(format!("AI 请求不存在，但无法核对题目状态：{review_error}。请重新查询。")));
                                    }
                                }
                            }
                        }
                        Err(error) if alive.get() => {
                            request_not_found.set(false);
                            request_info.set(None);
                            request_error.set(Some(format!(
                                "请求结果尚未确认：{error}。可再次查询，不会自动重发。"
                            )));
                        }
                        _ => (),
                    }
                });
            }
            move || alive.set(false)
        });
    }
    {
        let diagram_root = diagram_root.clone();
        let key = (
            props.question_id.clone(),
            (*state).clone(),
            (*stream).clone(),
        );
        use_effect_with(key, move |_| {
            hydrate_math();
            if let Some(root) = diagram_root.cast::<HtmlElement>() {
                render_ai_diagrams(&root);
            }
            || ()
        });
    }
    {
        let messages_ref = messages_ref.clone();
        let follow_bottom = follow_bottom.clone();
        let new_content = new_content.clone();
        let key = (
            (*state).clone(),
            (*pending_message).clone(),
            (*stream).clone(),
        );
        use_effect_with(key, move |_| {
            if let Some(element) = messages_ref.cast::<HtmlElement>() {
                if element.has_attribute("data-review-scroll-restoring") {
                    // Returning to a review question restores its saved position
                    // after AI history arrives; never override it with chat autoscroll.
                    *follow_bottom.borrow_mut() = false;
                    new_content.set(false);
                } else if *follow_bottom.borrow() {
                    element.set_scroll_top(element.scroll_height());
                    new_content.set(false);
                } else {
                    new_content.set(true);
                }
            }
            || ()
        });
    }
    let on_messages_scroll = {
        let messages_ref = messages_ref.clone();
        let follow_bottom = follow_bottom.clone();
        let new_content = new_content.clone();
        let on_question_scroll = props.on_question_scroll.clone();
        Callback::from(move |event: Event| {
            on_question_scroll.emit(event);
            if let Some(element) = messages_ref.cast::<HtmlElement>() {
                let near_bottom =
                    element.scroll_height() - element.client_height() - element.scroll_top() <= 48;
                *follow_bottom.borrow_mut() = near_bottom;
                if near_bottom {
                    new_content.set(false);
                }
            }
        })
    };
    let scroll_to_bottom = {
        let messages_ref = messages_ref.clone();
        let follow_bottom = follow_bottom.clone();
        let new_content = new_content.clone();
        Callback::from(move |_: MouseEvent| {
            *follow_bottom.borrow_mut() = true;
            if let Some(element) = messages_ref.cast::<HtmlElement>() {
                element.set_scroll_top(element.scroll_height());
            }
            new_content.set(false);
        })
    };
    let start_review = {
        let state = state.clone();
        let request_id = request_id.clone();
        let request_busy = request_busy.clone();
        let request_info = request_info.clone();
        let request_error = request_error.clone();
        let request_key = request_key.clone();
        let reconnect_stream = reconnect_stream.clone();
        let revision = revision.clone();
        let failure = failure.clone();
        let submitting = submitting.clone();
        let endpoint = endpoint.clone();
        let csrf = props.csrf.clone();
        let start_instruction = start_instruction.clone();
        let start_images = start_images.clone();
        let uploading_start = uploading_start.clone();
        let start_key = start_key.clone();
        let grade_mode = props.mode == "grade";
        Callback::from(move |_: ()| {
            if *submitting || *uploading_start {
                return;
            }
            if request_id.is_some()
                && request_info.as_ref().as_ref().is_none_or(|info| {
                    Some(&info.request_id) != (*request_id).as_ref()
                        || info.remote_uncertain
                        || matches!(info.status.as_str(), "unknown" | "running")
                })
            {
                return;
            }
            if *request_busy.borrow() {
                return;
            }
            let next_id = match new_ai_request_id() {
                Ok(id) => id,
                Err(error) => {
                    failure.set(Some(error));
                    return;
                }
            };
            let previous_id = (*request_id).clone();
            let previous_info = (*request_info).clone();
            if let Err(error) = remember_ai_request(&request_key, &next_id) {
                failure.set(Some(error));
                return;
            }
            *request_busy.borrow_mut() = true;
            let request_busy = request_busy.clone();
            request_id.set(Some(next_id.clone()));
            request_info.set(None);
            request_error.set(None);
            submitting.set(true);
            failure.set(None);
            let state = state.clone();
            let request_id = request_id.clone();
            let request_info = request_info.clone();
            let request_key = request_key.clone();
            let reconnect_stream = reconnect_stream.clone();
            let revision = revision.clone();
            let failure = failure.clone();
            let submitting = submitting.clone();
            let endpoint = endpoint.clone();
            let csrf = csrf.clone();
            let draft = (*start_instruction).clone();
            let instruction = if grade_mode {
                format!("【申论批改】\n{}", draft.trim())
            } else {
                draft.trim().to_owned()
            };
            let start_key = start_key.clone();
            let start_instruction = start_instruction.clone();
            let start_images = start_images.clone();
            let attachments = start_images.iter().map(|image| image.id.clone()).collect();
            spawn_local(async move {
                let result = post_json_response_typed::<_, AiRequestReceipt>(
                    &endpoint,
                    &csrf,
                    &AiReviewStartRequest {
                        request_id: next_id,
                        instruction,
                        attachments,
                    },
                    "开始 AI 解析",
                )
                .await;
                *revision.borrow_mut() += 1;
                match result {
                    Ok(result) => {
                        request_info.set(Some(result.request));
                        state.set(Some(result.state));
                        if ai_draft_read(&start_key) == draft {
                            ai_draft_write(&start_key, "");
                            start_instruction.set(String::new());
                        }
                        start_images.set(Vec::new());
                    }
                    Err(error) => {
                        if definite_ai_rejection(&error) {
                            ai_draft_write(&request_key, previous_id.as_deref().unwrap_or(""));
                            request_id.set(previous_id);
                            request_info.set(previous_info);
                            if error.is_code("question_bank_ai_attachment_expired") {
                                start_images.set(Vec::new());
                            }
                        }
                        failure.set(Some(error.to_string()));
                    }
                }
                reconnect_stream.emit(());
                *request_busy.borrow_mut() = false;
                submitting.set(false);
            });
        })
    };
    let on_start = {
        let start = start_review.clone();
        Callback::from(move |_: MouseEvent| start.emit(()))
    };
    let on_start_key = {
        let start = start_review.clone();
        Callback::from(move |event: KeyboardEvent| {
            if !event.is_composing()
                && event.key() == "Enter"
                && (event.ctrl_key() || event.meta_key())
            {
                event.prevent_default();
                start.emit(());
            }
        })
    };
    let on_stop = {
        let state = state.clone();
        let request_check = request_check.clone();
        let reconnect_stream = reconnect_stream.clone();
        let revision = revision.clone();
        let failure = failure.clone();
        let submitting = submitting.clone();
        let endpoint = format!("{endpoint}/stop");
        let csrf = props.csrf.clone();
        Callback::from(move |_| {
            if *submitting {
                return;
            }
            submitting.set(true);
            let state = state.clone();
            let request_check = request_check.clone();
            let reconnect_stream = reconnect_stream.clone();
            let revision = revision.clone();
            let failure = failure.clone();
            let submitting = submitting.clone();
            let endpoint = endpoint.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let result = post_json_response::<_, AiReviewState>(
                    &endpoint,
                    &csrf,
                    &EmptyRequest {},
                    "停止 AI 解析",
                )
                .await;
                *revision.borrow_mut() += 1;
                match result {
                    Ok(result) => state.set(Some(result)),
                    Err(error) => failure.set(Some(error)),
                }
                request_check.set(*request_check + 1);
                reconnect_stream.emit(());
                submitting.set(false);
            });
        })
    };
    let send_follow_up = {
        let state = state.clone();
        let request_id = request_id.clone();
        let request_busy = request_busy.clone();
        let request_info = request_info.clone();
        let request_error = request_error.clone();
        let request_key = request_key.clone();
        let reconnect_stream = reconnect_stream.clone();
        let revision = revision.clone();
        let failure = failure.clone();
        let submitting = submitting.clone();
        let follow_up = follow_up.clone();
        let follow_images = follow_images.clone();
        let uploading_follow = uploading_follow.clone();
        let current_state = state.clone();
        let stream = stream.clone();
        let pending_message = pending_message.clone();
        let follow_bottom = follow_bottom.clone();
        let follow_key = follow_key.clone();
        let endpoint = format!("{endpoint}/messages");
        let csrf = props.csrf.clone();
        let grade_mode = props.mode == "grade";
        Callback::from(move |_: ()| {
            if *submitting
                || *uploading_follow
                || current_state
                    .as_ref()
                    .as_ref()
                    .is_some_and(|value| value.status == "running")
                || stream
                    .as_ref()
                    .as_ref()
                    .is_some_and(|value| value.status == "running")
            {
                return;
            }
            let draft = (*follow_up).clone();
            let message = if grade_mode {
                format!("【申论批改请求或追问】\n如果以下内容是一份新答案，请按当前题目给出完整批改；如果是针对之前报告的问题，请直接解答，不把问题误认为答卷。\n{}", draft.trim())
            } else {
                draft.trim().to_owned()
            };
            if (message.is_empty() && follow_images.is_empty()) || message.chars().count() > (if grade_mode {50_256} else {2000}) {
                return;
            }
            if request_id.is_some()
                && request_info.as_ref().as_ref().is_none_or(|info| {
                    Some(&info.request_id) != (*request_id).as_ref()
                        || info.remote_uncertain
                        || matches!(info.status.as_str(), "unknown" | "running")
                })
            {
                return;
            }
            if *request_busy.borrow() {
                return;
            }
            let next_id = match new_ai_request_id() {
                Ok(id) => id,
                Err(error) => {
                    failure.set(Some(error));
                    return;
                }
            };
            let previous_id = (*request_id).clone();
            let previous_info = (*request_info).clone();
            if let Err(error) = remember_ai_request(&request_key, &next_id) {
                failure.set(Some(error));
                return;
            }
            *request_busy.borrow_mut() = true;
            let request_busy = request_busy.clone();
            request_id.set(Some(next_id.clone()));
            request_info.set(None);
            request_error.set(None);
            submitting.set(true);
            failure.set(None);
            *follow_bottom.borrow_mut() = true;
            pending_message.set(Some(if message.is_empty() {
                "图片追问".into()
            } else {
                message.clone()
            }));
            let pending_message = pending_message.clone();
            let follow_key = follow_key.clone();
            let state = state.clone();
            let request_id = request_id.clone();
            let request_info = request_info.clone();
            let request_key = request_key.clone();
            let reconnect_stream = reconnect_stream.clone();
            let revision = revision.clone();
            let failure = failure.clone();
            let submitting = submitting.clone();
            let follow_up = follow_up.clone();
            let follow_images = follow_images.clone();
            let endpoint = endpoint.clone();
            let csrf = csrf.clone();
            let attachments = follow_images.iter().map(|image| image.id.clone()).collect();
            spawn_local(async move {
                let payload = AiReviewFollowUpRequest {
                    request_id: next_id,
                    message,
                    attachments,
                };
                let result = post_json_response_typed::<_, AiRequestReceipt>(
                    &endpoint,
                    &csrf,
                    &payload,
                    "AI 追问",
                )
                .await;
                *revision.borrow_mut() += 1;
                match result {
                    Ok(result) => {
                        request_info.set(Some(result.request));
                        state.set(Some(result.state));
                        if ai_draft_read(&follow_key) == draft {
                            ai_draft_write(&follow_key, "");
                            follow_up.set(String::new());
                        }
                        follow_images.set(Vec::new());
                    }
                    Err(error) => {
                        if definite_ai_rejection(&error) {
                            ai_draft_write(&request_key, previous_id.as_deref().unwrap_or(""));
                            request_id.set(previous_id);
                            request_info.set(previous_info);
                            if error.is_code("question_bank_ai_attachment_expired") {
                                follow_images.set(Vec::new());
                            }
                        }
                        failure.set(Some(format!("{error}。请先查询请求状态。")));
                    }
                }
                pending_message.set(None);
                reconnect_stream.emit(());
                *request_busy.borrow_mut() = false;
                submitting.set(false);
            });
        })
    };
    let on_follow_up = {
        let send = send_follow_up.clone();
        Callback::from(move |_: MouseEvent| send.emit(()))
    };
    let on_follow_up_key = {
        let send = send_follow_up.clone();
        Callback::from(move |event: KeyboardEvent| {
            if !event.is_composing()
                && event.key() == "Enter"
                && (event.ctrl_key() || event.meta_key())
            {
                event.prevent_default();
                send.emit(());
            }
        })
    };
    let on_delete = {
        let state = state.clone();
        let history_generation = history_generation.clone();
        let request_id = request_id.clone();
        let request_info = request_info.clone();
        let request_not_found = request_not_found.clone();
        let request_error = request_error.clone();
        let request_key = request_key.clone();
        let reconnect_stream = reconnect_stream.clone();
        let revision = revision.clone();
        let submitting = submitting.clone();
        let deleting = deleting.clone();
        let delete_confirmation_required = delete_confirmation_required.clone();
        let confirm_delete_uncertain = confirm_delete_uncertain.clone();
        let delete_error = delete_error.clone();
        let endpoint = endpoint.clone();
        let csrf = props.csrf.clone();
        Callback::from(move |_| {
            if *submitting || (*delete_confirmation_required && !*confirm_delete_uncertain) {
                return;
            }
            submitting.set(true);
            delete_error.set(None);
            let state = state.clone();
            let history_generation = history_generation.clone();
            let request_id = request_id.clone();
            let request_info = request_info.clone();
            let request_not_found = request_not_found.clone();
            let request_error = request_error.clone();
            let request_key = request_key.clone();
            let reconnect_stream = reconnect_stream.clone();
            let revision = revision.clone();
            let submitting = submitting.clone();
            let deleting = deleting.clone();
            let delete_confirmation_required = delete_confirmation_required.clone();
            let confirm_delete_uncertain = confirm_delete_uncertain.clone();
            let delete_error = delete_error.clone();
            let endpoint = endpoint.clone();
            let csrf = csrf.clone();
            let confirmed = *confirm_delete_uncertain;
            spawn_local(async move {
                let result = delete_json_body_response_typed::<_, AiReviewState>(
                    &endpoint,
                    &csrf,
                    &AiDeleteRequest {
                        confirm_uncertain: confirmed,
                    },
                    "删除 AI 解析",
                )
                .await;
                *revision.borrow_mut() += 1;
                match result {
                    Ok(value) => {
                        state.set(Some(value));
                        history_generation.set(history_generation.wrapping_add(1));
                        ai_draft_write(&request_key, "");
                        request_id.set(None);
                        request_info.set(None);
                        request_not_found.set(false);
                        request_error.set(None);
                        delete_confirmation_required.set(false);
                        confirm_delete_uncertain.set(false);
                        deleting.set(false);
                    }
                    Err(error) if error.is_code("question_bank_ai_confirmation_required") => {
                        delete_confirmation_required.set(true);
                        confirm_delete_uncertain.set(false);
                        delete_error.set(None);
                    }
                    Err(error) => delete_error.set(Some(error.to_string())),
                }
                reconnect_stream.emit(());
                submitting.set(false);
            });
        })
    };
    let on_retry = {
        let state = state.clone();
        let request_id = request_id.clone();
        let request_busy = request_busy.clone();
        let request_info = request_info.clone();
        let request_key = request_key.clone();
        let request_error = request_error.clone();
        let failure = failure.clone();
        let submitting = submitting.clone();
        let retry_open = retry_open.clone();
        let retry_expired = retry_expired.clone();
        let retry_images = retry_images.clone();
        let retry_uploading = retry_uploading.clone();
        let confirm_uncertain = confirm_uncertain.clone();
        let confirm_new_thread = confirm_new_thread.clone();
        let endpoint = endpoint.clone();
        let csrf = props.csrf.clone();
        let reconnect_stream = reconnect_stream.clone();
        let revision = revision.clone();
        Callback::from(move |_: MouseEvent| {
            let Some(source) = (*request_info).clone() else {
                return;
            };
            if *submitting
                || *retry_uploading
                || Some(&source.request_id) != (*request_id).as_ref()
                || !matches!(source.status.as_str(), "failed" | "stopped" | "unknown")
                || (source.remote_uncertain && !*confirm_uncertain)
                || (source
                    .error
                    .as_deref()
                    .is_some_and(|error| error.starts_with("无法恢复原 Codex Thread"))
                    && !*confirm_new_thread)
                || (*retry_expired && retry_images.is_empty())
            {
                return;
            }
            if *request_busy.borrow() {
                return;
            }
            let next_id = match new_ai_request_id() {
                Ok(id) => id,
                Err(error) => {
                    failure.set(Some(error));
                    return;
                }
            };
            let payload = AiRetryRequest {
                request_id: next_id.clone(),
                confirm_uncertain: *confirm_uncertain,
                confirm_new_thread: *confirm_new_thread,
                replacement_attachments: if *retry_expired {
                    Some(retry_images.iter().map(|image| image.id.clone()).collect())
                } else {
                    None
                },
            };
            if let Err(error) = remember_ai_request(&request_key, &next_id) {
                failure.set(Some(error));
                return;
            }
            *request_busy.borrow_mut() = true;
            let request_busy = request_busy.clone();
            request_id.set(Some(next_id));
            request_info.set(None);
            request_error.set(None);
            failure.set(None);
            submitting.set(true);
            let state = state.clone();
            let request_id = request_id.clone();
            let request_info = request_info.clone();
            let request_key = request_key.clone();
            let failure = failure.clone();
            let submitting = submitting.clone();
            let retry_open = retry_open.clone();
            let retry_expired = retry_expired.clone();
            let retry_images = retry_images.clone();
            let reconnect_stream = reconnect_stream.clone();
            let revision = revision.clone();
            let endpoint = format!("{endpoint}/requests/{}/retry", source.request_id);
            let csrf = csrf.clone();
            spawn_local(async move {
                let result = post_json_response_typed::<_, AiRequestReceipt>(
                    &endpoint,
                    &csrf,
                    &payload,
                    "重试 AI 解析",
                )
                .await;
                *revision.borrow_mut() += 1;
                match result {
                    Ok(receipt) => {
                        request_info.set(Some(receipt.request));
                        state.set(Some(receipt.state));
                        retry_open.set(false);
                        retry_expired.set(false);
                        retry_images.set(Vec::new());
                    }
                    Err(error) => {
                        if definite_ai_rejection(&error) {
                            ai_draft_write(&request_key, &source.request_id);
                            request_id.set(Some(source.request_id.clone()));
                            request_info.set(Some(source));
                            if error.is_code("question_bank_ai_attachment_expired") {
                                retry_expired.set(true);
                            }
                        }
                        failure.set(Some(error.to_string()));
                    }
                }
                reconnect_stream.emit(());
                *request_busy.borrow_mut() = false;
                submitting.set(false);
            });
        })
    };
    let current: Option<AiReviewState> = (*state).clone();
    let running = stream
        .as_ref()
        .as_ref()
        .is_some_and(|value| value.status == "running")
        || current
            .as_ref()
            .is_some_and(|value| value.status == "running")
        || request_info
            .as_ref()
            .as_ref()
            .is_some_and(|info| info.status == "running");
    let has_answer = current
        .as_ref()
        .is_some_and(|value| value.messages.iter().any(|msg| msg.role == "assistant"));
    let request_unresolved = request_id.is_some()
        && request_info
            .as_ref()
            .as_ref()
            .is_none_or(|info| Some(&info.request_id) != (*request_id).as_ref());
    let request_uncertain = request_info
        .as_ref()
        .as_ref()
        .is_some_and(|info| info.remote_uncertain || info.status == "unknown");
    let can_start = !running
        && current
            .as_ref()
            .is_some_and(|value| value.messages.is_empty() && value.status == "idle");
    let can_retry = !running
        && request_info.as_ref().as_ref().is_some_and(|info| {
            Some(&info.request_id) == (*request_id).as_ref()
                && matches!(info.status.as_str(), "failed" | "stopped" | "unknown")
        });
    html! {
        <section class="flex min-h-0 min-w-0 flex-1 flex-col" data-testid="question-bank-ai-review" ref={diagram_root}>
            <div class="min-h-0 flex-1 space-y-3 overflow-y-auto overscroll-contain touch-pan-y p-4" ref={messages_ref} onscroll={on_messages_scroll} data-testid="question-bank-ai-messages" role="log" aria-label="AI 对话记录" aria-live="polite">
            <div class="space-y-4" data-testid="question-bank-ai-question-content">{props.question_content.clone()}</div>
            <div class="flex items-center justify-between gap-2">
                <strong>{if props.mode=="grade" {"AI 批改与追问"} else {"AI 解析与追问"}}</strong>
                <span class="text-xs text-base-content/60" data-testid="question-bank-ai-status">
                    {if running { "正在生成" } else { match stream.as_ref().as_ref().map(|v| v.status.as_str()).or_else(|| current.as_ref().map(|v| v.status.as_str())) {
                        Some("complete") => "已完成",
                        Some("stopped") => "已停止",
                        Some("failed") => "生成失败",
                        Some(_) => "未生成",
                        None => "正在加载",
                    } }}
                </span>
            </div>
            if *disconnected {
                <p class="text-xs text-warning" role="status" data-testid="question-bank-ai-disconnected">{"连接中断，正在重新连接…"}</p>
            }
            if let Some(error) = failure.as_ref() {
                <p class="text-sm text-error" role="alert">{error.clone()}</p>
            }
            if request_id.is_some() && (request_unresolved || request_error.is_some()) {
                <div class="space-y-2 rounded-xl border border-warning/40 p-3" role="status" data-testid="question-bank-ai-request-unknown">
                    <p>{request_error.as_deref().unwrap_or("正在查询此前请求；结果确认前不会再次发送。")}</p>
                    <button type="button" class="btn btn-outline btn-sm" data-testid="question-bank-ai-request-check"
                        disabled={*submitting} onclick={Callback::from({let request_check=request_check.clone(); move |_| request_check.set(*request_check + 1)})}>{"查询请求状态"}</button>
                    if *request_not_found {
                        if !running {
                            <p class="text-xs text-warning">{"清除仅放弃追踪该请求 ID，不会删除服务端 AI 解析，也不会自动重发；原有输入草稿将保留。"}</p>
                            <button type="button" class="btn btn-outline btn-sm" data-testid="question-bank-ai-request-discard"
                                disabled={*submitting}
                                onclick={Callback::from({
                                    let request_key=request_key.clone();
                                    let request_id=request_id.clone();
                                    let request_info=request_info.clone();
                                    let request_not_found=request_not_found.clone();
                                    let request_error=request_error.clone();
                                    let failure=failure.clone();
                                    move |_| {
                                        ai_draft_write(&request_key, "");
                                        request_id.set(None);
                                        request_info.set(None);
                                        request_not_found.set(false);
                                        request_error.set(None);
                                        failure.set(None);
                                    }
                                })}>{"清除失效请求标识"}</button>
                        } else {
                            <p class="text-xs text-warning">{"当前题目仍在生成，不能清除请求标识；请先停止或查询任务状态。"}</p>
                        }
                    }
                </div>
            }
            if *disconnected && request_id.is_some() && !request_unresolved {
                <button type="button" class="btn btn-ghost btn-sm" data-testid="question-bank-ai-request-refresh"
                    disabled={*submitting} onclick={Callback::from({let request_check=request_check.clone(); move |_| request_check.set(*request_check + 1)})}>{"查询本次请求状态"}</button>
            }
            if can_retry {
                if !*retry_open {
                    <button type="button" class="btn btn-outline btn-sm" data-testid="question-bank-ai-restart"
                        disabled={*submitting} onclick={Callback::from({let retry_open=retry_open.clone(); let confirm_uncertain=confirm_uncertain.clone(); let confirm_new_thread=confirm_new_thread.clone(); move |_| { confirm_uncertain.set(false); confirm_new_thread.set(false); retry_open.set(true); }})}>
                        {if request_info.as_ref().as_ref().is_some_and(|info| info.intent == "follow_up") {"重试本次追问"} else {"重试首次解析"}}
                    </button>
                } else if let Some(info) = request_info.as_ref().as_ref() {
                    <div class="space-y-2 rounded-xl border border-warning/40 p-3" data-testid="question-bank-ai-retry-confirmation">
                        <p>{if info.intent == "follow_up" {"将使用原追问和原有上下文重试，不重复插入追问。"} else {"将使用原要求重试首次解析。"}}</p>
                        if info.remote_uncertain {
                            <label class="flex items-start gap-2 text-sm">
                                <input type="checkbox" class="checkbox checkbox-sm" checked={*confirm_uncertain} data-testid="question-bank-ai-confirm-uncertain"
                                    onchange={Callback::from({let confirm_uncertain=confirm_uncertain.clone(); move |event: Event| {let input: HtmlInputElement=event.target_unchecked_into(); confirm_uncertain.set(input.checked());}})}/>
                                <span>{"远端执行结果不确定，可能重复消耗额度；我确认仍要重试。"}</span>
                            </label>
                        }
                        if info.error.as_deref().is_some_and(|error| error.starts_with("无法恢复原 Codex Thread")) {
                            <label class="flex items-start gap-2 text-sm">
                                <input type="checkbox" class="checkbox checkbox-sm" checked={*confirm_new_thread} data-testid="question-bank-ai-confirm-new-thread"
                                    onchange={Callback::from({let confirm_new_thread=confirm_new_thread.clone(); move |event: Event| {let input: HtmlInputElement=event.target_unchecked_into(); confirm_new_thread.set(input.checked());}})}/>
                                <span>{"原对话上下文无法恢复；我确认创建新对话后重试。"}</span>
                            </label>
                        }
                        if *retry_expired {
                            <p class="text-sm text-warning" role="alert">{"原临时图片已失效，请重新上传与原请求相同数量的图片，然后确认重试。"}</p>
                            <AiImagePicker endpoint={format!("{endpoint}/attachments")} csrf={props.csrf.clone()}
                                attachments={retry_images.clone()} uploading={retry_uploading.clone()} disabled={*submitting}
                                testid="question-bank-ai-retry-images">
                                <span class="text-sm">{"重新上传原请求的图片"}</span>
                            </AiImagePicker>
                        }
                        <button type="button" class="btn btn-primary btn-sm" data-testid="question-bank-ai-retry-confirm"
                            disabled={*submitting || *retry_uploading || (info.remote_uncertain && !*confirm_uncertain)
                                || (info.error.as_deref().is_some_and(|error| error.starts_with("无法恢复原 Codex Thread")) && !*confirm_new_thread)
                                || (*retry_expired && retry_images.is_empty())} onclick={on_retry}>{"确认重试"}</button>
                        <button type="button" class="btn btn-ghost btn-sm" disabled={*submitting}
                            onclick={Callback::from({let retry_open=retry_open.clone(); move |_| retry_open.set(false)})}>{"取消"}</button>
                    </div>
                }
            }
            if let Some(value) = current.as_ref() {
                if let Some(error) = &value.error { <p class="text-sm text-error" role="alert">{error.clone()}</p> }
            }
            <super::ai_review_activity::AiReviewActivity
                key={format!("{}:{}", props.question_id, *history_generation)}
                refresh_key={format!("{:?}", current.as_ref().map(|value| {
                    (&value.status, value.messages.iter().rev().find(|message| message.role == "assistant").map(|message| message.id))
                }))}
                endpoint={endpoint.clone()}
                stream={if current.as_ref().is_some_and(|value| value.status == "idle") {None} else {(*stream).clone()}}
                messages={current.as_ref().map(|value| value.messages.clone()).unwrap_or_default()}/>
            if let Some(active) = stream.as_ref().as_ref() {
                if active.status == "running" && !active.text.is_empty() {
                    <div class="py-2" data-testid="question-bank-ai-stream-text">
                        <div class="mb-2 text-xs font-semibold text-base-content/60">{"AI 正在回复"}</div>
                        if streaming_markdown_complete(&active.text) {
                            <div class="study-markdown text-base leading-7" data-ai-diagram-source="true">{render_question_bank_markdown(&active.text)}</div>
                        } else {
                            <div class="whitespace-pre-wrap break-words text-base leading-7">{active.text.clone()}</div>
                        }
                    </div>
                }
            }
            if let Some(message) = pending_message.as_ref() {
                <div class="rounded-xl bg-primary/10 p-3" data-testid="question-bank-ai-pending">
                    <div class="mb-2 text-xs font-semibold text-base-content/60">{"你的追问 · 发送中"}</div>
                    <div class="study-markdown text-base leading-7">{render_question_bank_markdown(message)}</div>
                </div>
            }
            if *deleting && current.is_some() {
                <div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/50 p-4" role="dialog" aria-modal="true" aria-label="确认删除 AI 解析">
                    <div class="w-full max-w-sm space-y-4 rounded-box bg-base-100 p-5 shadow-xl">
                        <p>{"确定删除这道题的 AI 解析和全部追问吗？"}</p>
                        if *delete_confirmation_required {
                            <p class="text-sm text-warning" role="alert">{"远端执行结果不确定，可能仍在运行；删除将清空本地 AI 解析和追问记录，请确认后继续。"}</p>
                            <label class="flex items-start gap-2 text-sm">
                                <input type="checkbox" class="checkbox checkbox-sm"
                                    checked={*confirm_delete_uncertain}
                                    data-testid="question-bank-ai-delete-confirm-uncertain"
                                    onchange={Callback::from({
                                        let confirm_delete_uncertain=confirm_delete_uncertain.clone();
                                        move |event: Event| {
                                            let input: HtmlInputElement=event.target_unchecked_into();
                                            confirm_delete_uncertain.set(input.checked());
                                        }
                                    })}/>
                                <span>{"我确认清除可能仍在远端执行的 AI 请求记录。"}</span>
                            </label>
                        }
                        if let Some(error) = delete_error.as_ref() {
                            <p class="text-sm text-error" role="alert" data-testid="question-bank-ai-delete-error">{error.clone()}</p>
                        }
                        <div class="flex justify-end gap-2">
                            <button type="button" class="btn btn-ghost btn-sm" data-testid="question-bank-ai-delete-cancel" disabled={*submitting}
                                onclick={Callback::from({let deleting=deleting.clone(); move |_|deleting.set(false)})}>{"取消"}</button>
                            <button type="button" class="btn btn-error btn-sm" data-testid="question-bank-ai-delete-confirm"
                                disabled={*submitting || (*delete_confirmation_required && !*confirm_delete_uncertain)}
                                onclick={on_delete}>{"确认删除"}</button>
                        </div>
                    </div>
                </div>
            }
            if can_start {
                <p class="text-sm text-base-content/60">{if props.mode=="grade" {
                    "粘贴作答或上传手写图片。AI 先辨认原图并确认不清楚的文字，再根据原题给出 Markdown 批改报告。"
                } else {
                    "AI 根据原题、相关给定材料和你的要求进行解析，来源参考答案仅作参考。"
                }}</p>
            }
            </div>
            <div class="relative shrink-0 bg-base-100 px-2 pb-3 pt-2" data-testid="question-bank-ai-composer-area">
            if *new_content {
                <button type="button" class="btn btn-circle btn-sm absolute bottom-full left-1/2 z-10 -translate-x-1/2 border border-base-content/15 bg-base-100 shadow-md" aria-label="有新内容，回到底部" data-testid="question-bank-ai-scroll-bottom" onclick={scroll_to_bottom}>{"↓"}</button>
            }
            if can_start {
                <div class="space-y-1">
                    <label class="sr-only" for="question-ai-instruction">{"可选要求"}</label>
                    {ai_composer(AiComposer {
                        id: "question-ai-instruction", input_ref: start_input_ref.clone(),
                        compose_testid: "question-bank-ai-requirements-compose",
                        input_testid: "question-bank-ai-instruction",
                        button_testid: "question-bank-ai-start",
                        value: start_instruction.as_str(), max_length: if props.mode=="grade" {50_000} else {2000}, read_only: false,
                        placeholder: if props.mode=="grade" {"粘贴申论作答，或上传手写图片…"} else {"输入解析要求（可选）…"},
                        button_label: if props.mode=="grade" {"发送批改"} else {"开始 AI 解析"}, is_stop: false,
                        disabled: *submitting || *uploading_start || request_unresolved || request_uncertain || (props.mode=="grade" && start_instruction.trim().is_empty() && start_images.is_empty()),
                        endpoint: format!("{endpoint}/attachments"), csrf: props.csrf.clone(),
                        attachments: start_images.clone(), uploading: uploading_start.clone(),
                        picker_disabled: *submitting, picker_testid: "question-bank-ai-start-images",
                        oninput: Callback::from({
                            let start_instruction=start_instruction.clone();
                            let start_key=start_key.clone();
                            move |event: InputEvent| {
                                let input: web_sys::HtmlTextAreaElement = event.target_unchecked_into();
                                ai_draft_write(&start_key, &input.value());
                                start_instruction.set(input.value());
                            }
                        }),
                        onkeydown: on_start_key.clone(),
                        onclick: on_start.clone(),
                    })}
                </div>
            }
            if has_answer || running {
                <div class="space-y-2">
                    <label class="sr-only" for="question-ai-follow-up">{"继续追问"}</label>
                    {ai_composer(AiComposer {
                        id: "question-ai-follow-up", input_ref: follow_input_ref.clone(),
                        compose_testid: "question-bank-ai-compose",
                        input_testid: "question-bank-ai-input",
                        button_testid: if running {"question-bank-ai-stop"} else {"question-bank-ai-send"},
                        value: follow_up.as_str(), max_length: if props.mode=="grade" {50_000} else {2000}, read_only: false,
                        placeholder: if props.mode=="grade" {"追问扣分原因，或发送修改后的答案…"} else {"输入消息…"},
                        button_label: if running {"停止生成"} else {"发送追问"}, is_stop: running,
                        disabled: *submitting || (!running && (*uploading_follow || request_unresolved || request_uncertain || (follow_up.trim().is_empty() && follow_images.is_empty()))),
                        endpoint: format!("{endpoint}/attachments"), csrf: props.csrf.clone(),
                        attachments: follow_images.clone(), uploading: uploading_follow.clone(),
                        picker_disabled: *submitting, picker_testid: "question-bank-ai-follow-up-images",
                        oninput: Callback::from({
                            let follow_up=follow_up.clone();
                            let follow_key=follow_key.clone();
                            move |event: InputEvent| {
                                let input: web_sys::HtmlTextAreaElement = event.target_unchecked_into();
                                ai_draft_write(&follow_key, &input.value());
                                follow_up.set(input.value());
                            }
                        }),
                        onkeydown: on_follow_up_key.clone(),
                        onclick: if running {on_stop.clone()} else {on_follow_up.clone()},
                    })}
                </div>
            }
            </div>
        </section>
    }
}

#[function_component(QuestionBankTraining)]
pub(crate) fn question_bank_training(props: &QuestionBankTrainingProps) -> Html {
    let clock = use_context::<StudyClockContext>();
    let papers = use_state(Vec::<PaperSummary>::new);
    let drill_collections = use_state(|| None::<Vec<DrillCollection>>);
    let selected_drill_collection = use_state(String::new);
    let selected_drill_paper = use_state(String::new);
    let history = use_state(Vec::<PracticeSession>::new);
    let history_open = use_state(|| props.show_history);
    let history_forced = use_state(|| false);
    let title_open = use_state(|| false);
    let history_filter = use_state(|| None::<PracticeMode>);
    let history_unfinished = use_state(|| false);
    let selected_paper = use_state(String::new);
    let selected_module = use_state(String::new);
    let catalog = use_state(|| None::<PracticeCatalog>);
    let selected_category = use_state(String::new);
    let module_filters = use_state(ModulePracticeFilters::default);
    let draft_filters = use_state(ModulePracticeFilters::default);
    let filters_loaded = use_state(|| false);
    let saving_filters = use_state(|| false);
    let settings_error = use_state(|| None::<String>);
    let settings_open = use_state(|| false);
    let custom_years = use_state(|| false);
    let collection = use_state(|| None::<PracticeMode>);
    let collection_review = use_state(|| None::<CollectionReviewTarget>);

    let mode = use_state(|| props.entry_mode.clone());
    let workspace = use_state(|| None::<PracticeWorkspace>);
    let review = use_state(|| None::<PracticeReview>);
    let index = use_state(|| 0usize);
    let selected_ai_question = workspace.as_ref().as_ref().and_then(|workspace| {
        let question = workspace.questions.get(*index)?;
        let review = (*review).as_ref()?;
        let reviewed = review.questions.get(*index)?;
        (review.session.id == workspace.session.id
            && reviewed.question.item.question_id == question.item.question_id)
            .then(|| question.item.question_id.clone())
    });
    let (ai_tab, ai_tab_chosen) = use_question_ai_tab(selected_ai_question);
    let ai_delete_signal = use_state(|| 0u32);
    let ai_can_delete = use_state(|| false);
    let multi_select = use_state(|| false);
    let current_favorite_source = workspace
        .as_ref()
        .as_ref()
        .and_then(|w| w.questions.get(*index))
        .map(|q| q.item.question_id.clone());
    let favorite = use_practice_favorite(current_favorite_source.clone());
    let flashcard_preview = use_state(|| None::<(String, FavoritePreview)>);
    let flashcard_error = use_state(|| None::<String>);
    let flashcard_retry = use_state(|| 0u32);
    let flashcard_open = use_state(|| false);
    let flashcard_changed = Callback::from({
        let flashcard_retry = flashcard_retry.clone();
        move |_| flashcard_retry.set((*flashcard_retry).wrapping_add(1))
    });
    let mounted = use_mut_ref(|| true);
    {
        let mounted = mounted.clone();
        use_effect_with((), move |_| move || *mounted.borrow_mut() = false);
    }
    let busy = use_state(|| false);
    let operation_lock = use_mut_ref(|| false);
    let pending_answer = use_state(|| None::<PendingAnswer>);
    let error = use_state(|| None::<String>);
    let refresh = use_state(|| 0u64);
    let version_ready = use_state(|| false);
    let version_checking = use_state(|| true);
    let version_error = use_state(|| None::<String>);
    let last_version = use_mut_ref(|| None::<String>);
    {
        let active = props.active;
        let version_ready = version_ready.clone();
        let version_checking = version_checking.clone();
        let version_error = version_error.clone();
        let last_version = last_version.clone();
        let refresh = refresh.clone();
        let filters_loaded = filters_loaded.clone();
        let had_data = *version_ready;
        use_effect_with(active, move |active| {
            let current = Rc::new(Cell::new(true));
            if *active {
                version_checking.set(true);
                let current = current.clone();
                spawn_local(async move {
                    let result =
                        fetch_json::<DataVersion>(&format!("{ROOT}/data-version"), "题库数据版本")
                            .await;
                    if !current.get() {
                        return;
                    }
                    let changed = match result {
                        Ok(value) => {
                            version_error.set(None);
                            let changed = last_version.borrow().as_ref() != Some(&value.version);
                            *last_version.borrow_mut() = Some(value.version);
                            changed
                        }
                        Err(message) => {
                            *last_version.borrow_mut() = None;
                            version_error.set(Some(message));
                            true
                        }
                    };
                    if changed && had_data {
                        filters_loaded.set(false);
                        refresh.set((*refresh).saturating_add(1));
                    }
                    version_ready.set(true);
                    version_checking.set(false);
                });
            }
            move || current.set(false)
        });
    }
    {
        let module_filters = module_filters.clone();
        let filters_loaded = filters_loaded.clone();
        let error = error.clone();
        use_effect_with((*version_ready, *refresh), move |(ready, _)| {
            let current = Rc::new(Cell::new(true));
            if *ready {
                let current = current.clone();
                spawn_local(async move {
                    let result =
                        fetch_json::<ModulePracticeFilters>(MODULE_SETTINGS_URL, "刷题设置").await;
                    if !current.get() {
                        return;
                    }
                    match result {
                        Ok(filters) => {
                            module_filters.set(filters);
                            filters_loaded.set(true);
                        }
                        Err(message) => error.set(Some(message)),
                    }
                });
            }
            move || current.set(false)
        });
    }
    let submit_confirm = use_state(|| false);
    let elapsed = use_state(|| 0u64);
    let seconds_ref = use_mut_ref(|| 0u64);
    let material_scroll = use_node_ref();
    let question_scroll = use_node_ref();
    let review_scrolls = use_mut_ref(ReviewScrollMemory::default);
    let material_edges = use_state(ScrollEdges::default);
    let question_edges = use_state(ScrollEdges::default);
    let wrong_collection = use_practice_collection(PracticeMode::Wrong, *refresh, *version_ready);
    let favorite_collection =
        use_practice_collection(PracticeMode::Favorite, *refresh, *version_ready);
    let wrong_total = wrong_collection
        .items
        .as_ref()
        .map(|items| items.len().to_string())
        .unwrap_or_else(|| "…".into());
    let favorite_total = favorite_collection
        .items
        .as_ref()
        .map(|items| items.len().to_string())
        .unwrap_or_else(|| "…".into());
    let selected_collection = match collection.as_ref() {
        Some(PracticeMode::Wrong) => Some(&wrong_collection),
        Some(PracticeMode::Favorite) => Some(&favorite_collection),
        _ => None,
    };
    let collection_items = selected_collection.and_then(|cache| cache.items.as_ref());
    let collection_loaded = collection_items.is_some();
    let collection_loading = selected_collection.is_some_and(|cache| *cache.loading);
    let collection_failed = selected_collection.is_some_and(|cache| cache.failure.is_some());
    let collection_retry = selected_collection.map(|cache| cache.invalidate.clone());
    let browse = collection_items.map(Vec::as_slice).unwrap_or(&[]);
    let refresh_edges = {
        let material_scroll = material_scroll.clone();
        let question_scroll = question_scroll.clone();
        let material_edges = material_edges.clone();
        let question_edges = question_edges.clone();
        Callback::from(move |_: ()| {
            if let Some(node) = material_scroll.cast::<web_sys::HtmlElement>() {
                let value = scroll_edges(&node);
                if *material_edges != value {
                    material_edges.set(value);
                }
            }
            if let Some(node) = question_scroll.cast::<web_sys::HtmlElement>() {
                let value = scroll_edges(&node);
                if *question_edges != value {
                    question_edges.set(value);
                }
            }
        })
    };

    {
        let papers = papers.clone();
        let drill_collections = drill_collections.clone();
        let history = history.clone();
        let error = error.clone();
        use_effect_with((*version_ready, *refresh), move |(ready, _)| {
            let ready = *ready;
            let current = std::rc::Rc::new(std::cell::Cell::new(true));
            let response_current = current.clone();
            spawn_local(async move {
                if !ready {
                    return;
                }
                let result =
                    fetch_json::<Vec<PaperSummary>>(&format!("{ROOT}/papers"), "真题试卷列表")
                        .await;
                if !response_current.get() {
                    return;
                }
                match result {
                    Ok(list) => papers.set(list),
                    Err(e) => error.set(Some(e)),
                }
                let result = fetch_json::<Vec<DrillCollection>>(
                    &format!("{ROOT}/collections"),
                    "专项合集列表",
                )
                .await;
                if !response_current.get() {
                    return;
                }
                match result {
                    Ok(list) => drill_collections.set(Some(list)),
                    Err(e) => error.set(Some(e)),
                }
                let result = fetch_json::<Vec<PracticeSession>>(
                    &format!("{ROOT}/practice/history"),
                    "练习历史",
                )
                .await;
                if !response_current.get() {
                    return;
                }
                if let Ok(items) = result {
                    history.set(items);
                }
            });
            move || current.set(false)
        });
    }
    {
        let filters = (*module_filters).clone();
        let loaded = *filters_loaded;
        let catalog = catalog.clone();
        let error = error.clone();
        use_effect_with((filters, *refresh, loaded), move |(filters, _, loaded)| {
            let url = catalog_filter_url(filters);
            let loaded = *loaded;
            let current = std::rc::Rc::new(std::cell::Cell::new(true));
            let response_current = current.clone();
            spawn_local(async move {
                if !loaded {
                    return;
                }
                let result = fetch_json::<PracticeCatalog>(&url, "模块题型进度").await;
                if !response_current.get() {
                    return;
                }
                match result {
                    Ok(result) => catalog.set(Some(result)),
                    Err(message) => error.set(Some(message)),
                }
            });
            move || current.set(false)
        });
    }
    {
        let current = workspace.as_ref().as_ref().map(|w| w.session.clone());
        let reviewing = review.is_some();
        let immersive = props.on_immersive_change.clone();
        let paused = props.on_paper_pause_change.clone();
        use_effect_with((current, reviewing), move |(value, reviewing)| {
            let active = value.as_ref().is_some_and(|s| s.status == "active");
            // Submitted papers keep the workspace open for review. Review is
            // still effective study time even though the session is completed
            // and its pause flag remains set.
            immersive.emit(active || *reviewing);
            paused.emit(active && value.as_ref().is_some_and(|s| s.paused));
            || ()
        });
    }
    {
        let elapsed = elapsed.clone();
        let seconds_ref = seconds_ref.clone();
        let current_id = workspace.as_ref().as_ref().map(|w| w.session.id.clone());
        let server_seconds = workspace
            .as_ref()
            .as_ref()
            .map(|w| w.session.elapsed_seconds)
            .unwrap_or(0);
        let running = workspace
            .as_ref()
            .as_ref()
            .is_some_and(|w| w.session.status == "active" && !w.session.paused);
        use_effect_with((running, current_id), move |(running, _)| {
            *seconds_ref.borrow_mut() = server_seconds;
            let timer = if *running {
                Some(gloo_timers::callback::Interval::new(1000, move || {
                    let next = {
                        let mut seconds = seconds_ref.borrow_mut();
                        *seconds = seconds.saturating_add(1);
                        *seconds
                    };
                    elapsed.set(next);
                }))
            } else {
                None
            };
            move || drop(timer)
        });
    }
    {
        let current = workspace
            .as_ref()
            .as_ref()
            .and_then(|w| w.questions.get(*index))
            .map(|q| q.item.question_id.clone());
        use_effect_with((current, review.is_some()), move |_| {
            hydrate_math();
            || ()
        });
    }
    {
        let review_scrolls = review_scrolls.clone();
        let material_scroll = material_scroll.clone();
        let question_scroll = question_scroll.clone();
        let session = workspace.as_ref().as_ref().and_then(|workspace| {
            let review = (*review).as_ref()?;
            (props.active && !*history_forced && review.session.id == workspace.session.id)
                .then(|| workspace.session.id.clone())
        });
        let question = workspace
            .as_ref()
            .as_ref()
            .and_then(|workspace| workspace.questions.get(*index))
            .map(|question| question.item.question_id.clone());
        use_effect_with(
            (session, question, *flashcard_open, *ai_tab),
            move |(session, question, panel_open, _)| {
                let mut memory = review_scrolls.borrow_mut();
                if memory.session != *session {
                    memory.session = session.clone();
                    memory.positions.clear();
                }
                memory.question = question
                    .clone()
                    .filter(|_| session.is_some() && !*panel_open);
                let mut cleanup = Vec::new();
                if session.is_some() && !*panel_open {
                    if let Some(question) = question {
                        let (material, question) =
                            memory.positions.get(question).copied().unwrap_or_default();
                        if let Some(node) = material_scroll.cast::<HtmlElement>() {
                            cleanup.push(restore_review_scroll(&node, material));
                        }
                        if let Some(node) = question_scroll.cast::<HtmlElement>() {
                            cleanup.push(restore_review_scroll(&node, question));
                        }
                    }
                }
                move || {
                    for cancel in cleanup {
                        let _ = cancel.call0(&JsValue::NULL);
                    }
                }
            },
        );
    }
    {
        let refresh_edges = refresh_edges.clone();
        let selected = (
            *index,
            review.is_some(),
            workspace.as_ref().as_ref().map(|w| w.session.id.clone()),
        );
        use_effect_with(selected, move |_| {
            refresh_edges.emit(());
            || ()
        });
    }
    {
        let flashcard_preview = flashcard_preview.clone();
        let flashcard_error = flashcard_error.clone();
        let flashcard_open = flashcard_open.clone();
        use_effect_with(
            (
                current_favorite_source.clone(),
                review.is_some(),
                *flashcard_retry,
            ),
            move |(question_id, is_review, _)| {
                let active = Rc::new(Cell::new(true));
                let current = active.clone();
                flashcard_preview.set(None);
                flashcard_error.set(None);
                flashcard_open.set(false);
                if *is_review {
                    if let Some(id) = question_id.clone() {
                        spawn_local(async move {
                            let result = fetch_json::<FavoritePreview>(
                                &format!("{ROOT}/practice/questions/{id}/flashcard-preview"),
                                "真题 Flashcard 预览",
                            )
                            .await;
                            if current.get() {
                                match result {
                                    Ok(value) => flashcard_preview.set(Some((id, value))),
                                    Err(message) => flashcard_error.set(Some(message)),
                                }
                            }
                        });
                    }
                }
                move || active.set(false)
            },
        );
    }
    let invoke = {
        let csrf = props.csrf.clone();
        let papers = papers.clone();
        let mode = mode.clone();
        let collection = collection.clone();
        let selected_paper = selected_paper.clone();
        let selected_module = selected_module.clone();
        let selected_category = selected_category.clone();
        let selected_drill_collection = selected_drill_collection.clone();
        let selected_drill_paper = selected_drill_paper.clone();
        let module_filters = module_filters.clone();
        let workspace = workspace.clone();
        let review = review.clone();
        let index = index.clone();
        let multi_select = multi_select.clone();
        let favorite = favorite.clone();
        let busy = busy.clone();
        let error = error.clone();
        let refresh = refresh.clone();
        let submit_confirm = submit_confirm.clone();
        let elapsed = elapsed.clone();
        let operation_lock = operation_lock.clone();
        let pending_answer = pending_answer.clone();
        let history_forced = history_forced.clone();
        let invalidate_favorites = favorite_collection.invalidate.clone();
        let mounted = mounted.clone();
        let clock = clock.clone();
        Callback::from(move |action: Action| {
            if !*mounted.borrow() || *operation_lock.borrow() {
                return;
            }
            let favorite_target = if let Action::Favorite(id) = &action {
                let ready = favorite.ready.borrow();
                let Some(saved) = ready.as_ref().filter(|saved| {
                    saved.question_id == *id && favorite.source.borrow().as_ref() == Some(id)
                }) else {
                    return;
                };
                Some(!saved.favorite)
            } else {
                None
            };
            *operation_lock.borrow_mut() = true;
            let answer_index = *index;
            if favorite_target.is_some() {
                *favorite.generation.borrow_mut() += 1;
            }
            let mutation_generation = *favorite.generation.borrow();
            busy.set(true);
            error.set(None);
            let csrf = csrf.clone();
            let papers = papers.clone();
            let mode = mode.clone();
            let collection = collection.clone();
            let selected_paper = selected_paper.clone();
            let selected_module = selected_module.clone();
            let selected_category = selected_category.clone();
            let selected_drill_collection = selected_drill_collection.clone();
            let selected_drill_paper = selected_drill_paper.clone();
            let module_filters = module_filters.clone();
            let workspace = workspace.clone();
            let review = review.clone();
            let index = index.clone();
            let multi_select = multi_select.clone();
            let favorite = favorite.clone();
            let busy = busy.clone();
            let error = error.clone();
            let refresh = refresh.clone();
            let submit_confirm = submit_confirm.clone();
            let elapsed = elapsed.clone();
            let operation_lock = operation_lock.clone();
            let pending_answer = pending_answer.clone();
            let history_forced = history_forced.clone();
            let invalidate_favorites = invalidate_favorites.clone();
            let mounted = mounted.clone();
            let clock = clock.clone();
            spawn_local(async move {
                let result: Result<(), String> = async {
                    if !*mounted.borrow() {
                        return Ok(());
                    }
                    match action {
                        Action::Start => {
                            let practice_mode = collection
                                .as_ref()
                                .cloned()
                                .unwrap_or_else(|| (*mode).clone());
                            let collection_active = collection.is_some();
                            let request = CreatePracticeRequest {
                                mode: practice_mode.clone(),
                                paper_id: match practice_mode {
                                    PracticeMode::Paper | PracticeMode::PaperModule
                                        if !collection_active =>
                                    {
                                        if selected_paper.is_empty()
                                            || !papers
                                                .iter()
                                                .any(|p| p.id.as_str() == selected_paper.as_str())
                                        {
                                            return Err("请先选择原始试卷".into());
                                        }
                                        Some((*selected_paper).clone())
                                    }
                                    PracticeMode::Drill => {
                                        if selected_drill_collection.is_empty() {
                                            return Err("请先选择专项合集".into());
                                        }
                                        (!selected_drill_paper.is_empty())
                                            .then(|| (*selected_drill_paper).clone())
                                    }
                                    _ => None,
                                },
                                module: if *mode == PracticeMode::Module || collection_active {
                                    if selected_module.is_empty() {
                                        if collection_active {
                                            None
                                        } else {
                                            return Err("请先选择模块".into());
                                        }
                                    } else {
                                        Some((*selected_module).clone())
                                    }
                                } else {
                                    None
                                },
                                category: if (*mode == PracticeMode::Module || collection_active)
                                    && !selected_category.is_empty()
                                {
                                    Some((*selected_category).clone())
                                } else {
                                    None
                                },
                                collection_id: if practice_mode == PracticeMode::Drill {
                                    Some((*selected_drill_collection).clone())
                                } else {
                                    None
                                },
                                filters: if *mode == PracticeMode::Module {
                                    (*module_filters).clone()
                                } else if collection_active {
                                    ModulePracticeFilters {
                                        count: Some(20),
                                        ..Default::default()
                                    }
                                } else {
                                    ModulePracticeFilters::default()
                                },
                            };
                            let session: PracticeSession = post_json_response(
                                &format!("{ROOT}/practice/sessions"),
                                &csrf,
                                &request,
                                "创建真题练习",
                            )
                            .await?;
                            let loaded = load_workspace(&session.id).await?;
                            if !*mounted.borrow() {
                                return Ok(());
                            }
                            elapsed.set(loaded.session.elapsed_seconds);
                            workspace.set(Some(loaded));
                            review.set(None);
                            index.set(0);
                            multi_select.set(false);
                            submit_confirm.set(false);
                        }
                        Action::Continue(id) => {
                            let loaded = load_workspace(&id).await?;
                            if !*mounted.borrow() {
                                return Ok(());
                            }
                            history_forced.set(false);
                            elapsed.set(loaded.session.elapsed_seconds);
                            workspace.set(Some(loaded));
                            review.set(None);
                            index.set(0);
                            multi_select.set(false);
                            submit_confirm.set(false);
                        }
                        Action::Repeat(id) => {
                            let session: PracticeSession = post_json_response(
                                &format!("{ROOT}/practice/sessions/{id}/repeat"),
                                &csrf,
                                &EmptyRequest {},
                                "按原题重做",
                            )
                            .await?;
                            let loaded = load_workspace(&session.id).await?;
                            if !*mounted.borrow() {
                                return Ok(());
                            }
                            history_forced.set(false);
                            elapsed.set(loaded.session.elapsed_seconds);
                            workspace.set(Some(loaded));
                            review.set(None);
                            index.set(0);
                            multi_select.set(false);
                            submit_confirm.set(false);
                        }
                        Action::Review(id) => {
                            let loaded: PracticeReview = fetch_json(
                                &format!("{ROOT}/practice/sessions/{id}/review"),
                                "真题解析",
                            )
                            .await?;
                            let plain = load_workspace(&id).await?;
                            if !*mounted.borrow() {
                                return Ok(());
                            }
                            history_forced.set(false);
                            workspace.set(Some(plain));
                            review.set(Some(loaded));
                            index.set(0);
                        }
                        Action::Answer(choice) => {
                            let active = (*workspace).clone().ok_or("训练不存在")?;
                            let q = active.questions.get(answer_index).ok_or("题位不存在")?;
                            let question_id = q.item.question_id.clone();
                            let mut answer = if *multi_select {
                                let mut selected = q.item.answer.clone();
                                if let Some(i) = selected.iter().position(|v| v == &choice) {
                                    selected.remove(i);
                                } else {
                                    selected.push(choice);
                                }
                                selected
                            } else {
                                vec![choice]
                            };
                            answer.sort();
                            pending_answer.set(Some(PendingAnswer {
                                session_id: active.session.id.clone(),
                                question_id: question_id.clone(),
                                answer: answer.clone(),
                            }));
                            let saved: PracticeSession = post_json_response(
                                &format!(
                                    "{ROOT}/practice/sessions/{}/answers/{}",
                                    active.session.id, question_id
                                ),
                                &csrf,
                                &SavePracticeAnswer { answer },
                                "保存真题答案",
                            )
                            .await
                            .map_err(|message| {
                                format!("保存失败，请重新选择答案重试：{message}")
                            })?;
                            if saved.id != active.session.id {
                                return Err("保存响应不匹配，请重新选择答案重试".into());
                            }
                            let saved_item = saved
                                .items
                                .iter()
                                .find(|item| item.question_id == question_id)
                                .ok_or("保存响应缺少题目，请重新选择答案重试")?
                                .clone();
                            let mut updated = active;
                            let question = updated
                                .questions
                                .get_mut(answer_index)
                                .filter(|question| question.item.question_id == question_id)
                                .ok_or("保存题位不匹配，请重新选择答案重试")?;
                            question.item = saved_item;
                            updated.session = saved;
                            if !*mounted.borrow() {
                                return Ok(());
                            }
                            workspace.set(Some(updated));
                        }
                        Action::Pause | Action::Exit => {
                            if let Some(active) = workspace.as_ref().as_ref() {
                                if active.session.status == "active" {
                                    let pause = match action {
                                        Action::Exit => true,
                                        _ => !active.session.paused,
                                    };
                                    let _: PracticeSession = post_json_response(
                                        &format!(
                                            "{ROOT}/practice/sessions/{}/pause",
                                            active.session.id
                                        ),
                                        &csrf,
                                        &SetPracticePaused {
                                            paused: pause,
                                            elapsed_seconds: *elapsed,
                                        },
                                        "暂停训练",
                                    )
                                    .await?;
                                    if !*mounted.borrow() {
                                        return Ok(());
                                    }
                                    if matches!(action, Action::Exit) {
                                        workspace.set(None);
                                    } else {
                                        let loaded = load_workspace(&active.session.id).await?;
                                        if !*mounted.borrow() {
                                            return Ok(());
                                        }
                                        workspace.set(Some(loaded));
                                    }
                                } else {
                                    workspace.set(None);
                                    review.set(None);
                                }
                            }
                            if matches!(action, Action::Exit) {
                                refresh.set(*refresh + 1);
                            }
                        }
                        Action::Submit => {
                            let active =
                                workspace.as_ref().as_ref().cloned().ok_or("训练不存在")?;
                            if !*submit_confirm
                                && active.session.items.iter().any(|q| q.answer.is_empty())
                            {
                                submit_confirm.set(true);
                            } else {
                                let _: PracticeSession = post_json_response(
                                    &format!(
                                        "{ROOT}/practice/sessions/{}/pause",
                                        active.session.id
                                    ),
                                    &csrf,
                                    &SetPracticePaused {
                                        paused: true,
                                        elapsed_seconds: *elapsed,
                                    },
                                    "交卷前保存用时",
                                )
                                .await?;
                                let completed: PracticeSession = post_json_response(
                                    &format!(
                                        "{ROOT}/practice/sessions/{}/submit",
                                        active.session.id
                                    ),
                                    &csrf,
                                    &EmptyRequest {},
                                    "真题交卷",
                                )
                                .await?;
                                let loaded = load_workspace(&completed.id).await?;
                                if !*mounted.borrow() {
                                    return Ok(());
                                }
                                elapsed.set(completed.elapsed_seconds.max(*elapsed));
                                workspace.set(Some(loaded));
                                let detail: PracticeReview = fetch_json(
                                    &format!("{ROOT}/practice/sessions/{}/review", completed.id),
                                    "真题解析",
                                )
                                .await?;
                                if !*mounted.borrow() {
                                    return Ok(());
                                }
                                review.set(Some(detail));
                                submit_confirm.set(false);
                                refresh.set(*refresh + 1);
                                if let Some(context) = clock.clone() {
                                    context.refresh_goals.emit(());
                                }
                            }
                        }
                        Action::Favorite(id) => {
                            let updated: FavoriteState = post_json_response(
                                &format!("{ROOT}/practice/favorites/{id}"),
                                &csrf,
                                &SetFavorite {
                                    favorite: favorite_target.ok_or("收藏状态尚未加载")?,
                                },
                                "收藏真题",
                            )
                            .await?;
                            if *mounted.borrow() {
                                invalidate_favorites.emit(());
                                if *favorite.generation.borrow() == mutation_generation
                                    && favorite.source.borrow().as_ref() == Some(&id)
                                    && updated.question_id == id
                                {
                                    *favorite.ready.borrow_mut() = Some(updated.clone());
                                    favorite.value.set(Some(updated));
                                }
                            }
                        }
                    }
                    Ok(())
                }
                .await;
                *operation_lock.borrow_mut() = false;
                if !*mounted.borrow() {
                    return;
                }
                if let Err(e) = result {
                    error.set(Some(e));
                }
                pending_answer.set(None);
                busy.set(false);
            });
        })
    };
    let visible_error = version_error
        .as_ref()
        .or_else(|| error.as_ref())
        .or_else(|| selected_collection.and_then(|cache| cache.failure.as_ref()))
        .or_else(|| wrong_collection.failure.as_ref())
        .or_else(|| favorite_collection.failure.as_ref());
    let alert = if let Some(message) = visible_error {
        html! { <p class="rounded-box bg-error/10 px-3 py-2 text-sm text-error" role="alert" data-testid="question-bank-error">{message.clone()}</p> }
    } else {
        Html::default()
    };

    if !props.active {
        return Html::default();
    }

    if let Some(current) = workspace.as_ref() {
        let item_index = (*index).min(current.questions.len().saturating_sub(1));
        if let Some(item) = current.questions.get(item_index) {
            let completed = current.session.status == "completed";
            let selected_review = review.as_ref().and_then(|r| r.questions.get(item_index));
            let card_preview = flashcard_preview
                .as_ref()
                .as_ref()
                .filter(|(id, _)| id == &item.item.question_id)
                .map(|(_, preview)| preview.clone());
            let question = item.question.as_ref();
            let options = question
                .map(|q| answer_options(&q.options_markdown))
                .unwrap_or_default();
            let pending = pending_answer.as_ref().filter(|pending| {
                pending.session_id == current.session.id
                    && pending.question_id == item.item.question_id
            });
            let displayed_answer = pending
                .map(|pending| &pending.answer)
                .unwrap_or(&item.item.answer);
            let qid = item.item.question_id.clone();
            let favorite_value = favorite
                .value
                .as_ref()
                .filter(|state| state.question_id == item.item.question_id);
            let favorite_failure = favorite
                .failure
                .as_ref()
                .filter(|(id, _)| *id == item.item.question_id);
            let favorite_pending = favorite_value.is_none() && favorite_failure.is_none();
            let starred = favorite_value.is_some_and(|state| state.favorite);
            let on_favorite = invoke.clone();
            let retry_favorite = {
                let retry = favorite.retry.clone();
                let operation_lock = operation_lock.clone();
                Callback::from(move |_| {
                    if !*operation_lock.borrow() {
                        retry.emit(());
                    }
                })
            };
            let on_scrolling = |is_material: bool| {
                let refresh_edges = refresh_edges.clone();
                let review_scrolls = review_scrolls.clone();
                let question_id = item.item.question_id.clone();
                let session_id = current.session.id.clone();
                Callback::from(move |event: web_sys::Event| {
                    let mut memory = review_scrolls.borrow_mut();
                    if memory.session.as_ref() == Some(&session_id)
                        && memory.question.as_ref() == Some(&question_id)
                    {
                        let node = event.target_unchecked_into::<HtmlElement>();
                        if node.has_attribute("data-review-scroll-restoring") {
                            return;
                        }
                        let position = node.scroll_top();
                        let saved = memory.positions.entry(question_id.clone()).or_default();
                        if is_material {
                            saved.0 = position;
                        } else {
                            saved.1 = position;
                        }
                    }
                    drop(memory);
                    refresh_edges.emit(());
                })
            };
            let on_material_scrolling = on_scrolling(true);
            let on_question_scrolling = on_scrolling(false);
            let material_meta = if item.materials.len() == 1 {
                item.materials[0].name.clone()
            } else {
                question
                    .map(|q| match (&q.module, &q.category) {
                        (Some(module), Some(category)) => format!("{module} · {category}"),
                        (Some(module), None) => module.clone(),
                        _ => String::new(),
                    })
                    .unwrap_or_default()
            };
            let material_pane = html! {
                <section class="flex min-h-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100" data-testid="question-bank-practice-material-pane">
                    <div class={classes!(QUESTION_BANK_PANE_HEADER,"question-bank-pane-header")}>
                        <strong>{"原卷材料"}</strong>
                        <div class="ml-auto flex shrink-0 items-center gap-2">
                            <span class="hidden max-w-40 truncate text-[11px] text-base-content/45 sm:inline">{material_meta.clone()}</span>
                            <QuestionBankScrollButtons passage={material_scroll.clone()}
                                can_scroll_up={material_edges.can_scroll_up}
                                can_scroll_down={material_edges.can_scroll_down}
                                question_index={item_index} target="材料" label="材料滚动" />
                        </div>
                    </div>
                    <div ref={material_scroll.clone()} onscroll={on_material_scrolling.clone()}
                        class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4"
                        data-testid="question-bank-practice-material-scroll">
                        {for item.materials.iter().map(|m| html!{
                            <section class="mb-5">
                                if item.materials.len()>1 { <h3 class="font-bold">{m.name.clone()}</h3> }
                                <div class="study-markdown question-bank-passage">{render_question_bank_markdown(&m.markdown)}</div>
                            </section>
                        })}
                    </div>
                </section>
            };
            let shared = !item.materials.is_empty();
            let question_content = html! { <div class="space-y-4">
                    if let Some((_, message)) = favorite_failure {
                        <div class="space-y-2 text-sm text-error" role="alert" data-testid="question-bank-favorite-error">
                            <p>{message.clone()}</p>
                            <button type="button" class="btn btn-outline btn-sm" disabled={*busy}
                                data-testid="question-bank-favorite-retry" onclick={retry_favorite}>{"重试收藏状态"}</button>
                        </div>
                    }
                    if item.content_changed {
                        <p class="text-warning">{"题目源文件已更新或暂不可用，不能用新版题面替代旧题。"}</p>
                    } else if let Some(q) = question {
                        if shared {
                            <div class="study-markdown question-bank-passage" data-testid="question-bank-practice-stem">{render_question_bank_markdown(&q.stem)}</div>
                        }
                        <div class="space-y-2">
                            {for options.iter().map(|(label, markdown)| {
                                let chosen = displayed_answer.contains(label);
                                let correct = selected_review.is_some_and(|r|r.correct_answer.contains(label));
                                let grade = item.item.grade.as_deref();
                                if completed {
                                    let state = if correct && grade != Some("ungradable") || chosen && grade == Some("correct") {
                                        ReviewChoiceState::Correct
                                    } else if chosen && grade == Some("wrong") {
                                        ReviewChoiceState::Wrong
                                    } else {
                                        ReviewChoiceState::Neutral
                                    };
                                    html! {<TrainingReviewChoice class="flex items-start gap-2.5 rounded-xl border p-3 text-base leading-7" test_id="question-bank-practice-choice" {state}>
                                        <span class="shrink-0 font-bold">{format!("{label}.")}</span>
                                        <span class="study-markdown min-w-0">{render_question_bank_markdown(markdown)}</span>
                                    </TrainingReviewChoice>}
                                } else {
                                    let style = if chosen {
                                        "border-primary bg-primary text-primary-content"
                                    } else {
                                        "border-base-content/20 bg-base-100 hover:border-primary/50"
                                    };
                                    let action = invoke.clone(); let choice = label.clone();
                                    html! {<button type="button" class={classes!("flex","w-full","items-start","gap-2.5","rounded-xl","border","p-3","text-left","text-base","leading-7","transition-colors","disabled:opacity-60",style)}
                                        aria-pressed={chosen.to_string()} aria-busy={(pending.is_some() && chosen).to_string()}
                                        data-testid="question-bank-practice-choice"
                                        disabled={current.session.paused || *busy}
                                        onclick={Callback::from(move |_|action.emit(Action::Answer(choice.clone())))}>
                                        <span class="shrink-0 font-bold">{format!("{label}.")}</span>
                                        <span class="study-markdown min-w-0">{render_question_bank_markdown(markdown)}</span>
                                    </button>}
                                }
                            })}
                        </div>
                        if !completed {
                            <label class="flex items-center gap-2 text-sm">
                                <input type="checkbox" class="checkbox checkbox-sm" checked={*multi_select} disabled={*busy}
                                    onchange={Callback::from({let multi_select=multi_select.clone();let operation_lock=operation_lock.clone();move |e: Event| {
                                        let checkbox: HtmlInputElement=e.target_unchecked_into();
                                        if *operation_lock.borrow() {
                                            checkbox.set_checked(*multi_select);
                                            return;
                                        }
                                        multi_select.set(checkbox.checked());
                                    }})}/>
                                {"多选作答（题目明确允许多选时开启）"}
                            </label>
                        }
                        if let Some(r) = selected_review {
                            <ReviewAnswerStats correct_answer={r.correct_answer.clone()} answer={Some(item.item.answer.clone())}
                                correct_count={r.correct_count} wrong_count={r.wrong_count}
                                answer_test_id="question-bank-practice-review-answer" counts_test_id="question-bank-practice-attempt-counts"/>
                            <div class="border-t border-base-content/10 pt-3 space-y-3">
                                if !*ai_tab {
                                    <h3 class="font-bold">{"原始解析"}</h3>
                                    <div class="study-markdown mt-2 text-base leading-7 text-base-content/75">
                                        {if r.explanation.is_empty() {html!{"暂无解析"}} else {render_question_bank_markdown(&r.explanation)}}
                                    </div>
                                    <h3 class="mt-3 font-bold">{"考点"}</h3>
                                    <p class="text-sm text-base-content/60">
                                        {if q.topics.is_empty() {"暂无考点".into()} else {q.topics.join("、")}}
                                    </p>
                                }
                            </div>
                            if !item.content_changed {
                                if let Some(preview) = &card_preview {
                                    if preview.existing.is_empty() && !favorite_can_create(preview) {
                                        <p class="text-xs opacity-60" role="status">
                                            {preview.reason.as_deref().unwrap_or("此题暂不支持创建闪卡")}
                                        </p>
                                    }
                                } else if let Some(message) = &*flashcard_error {
                                    <p role="alert" class="text-xs text-error">{message.clone()}</p>
                                    <button type="button" class="btn btn-outline btn-sm"
                                        data-testid="question-bank-practice-flashcard-retry"
                                        onclick={Callback::from({let flashcard_retry=flashcard_retry.clone();move |_|flashcard_retry.set(*flashcard_retry+1)})}>
                                        {"重试加载闪卡"}
                                    </button>
                                }
                            }
                        }
                    }
            </div> };
            let body = html! {
                <section class="flex h-full min-h-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100" data-testid="question-bank-practice-question-pane">
                <QuestionBankQuestionHeader
                    title_test_id="question-bank-practice-heading"
                    title={if completed {
                        format!("原卷第 {} 题 · {}/{}", item.item.number, item_index + 1, current.questions.len())
                    } else {
                        format!("第 {}/{} 题", item_index + 1, current.questions.len())
                    }}
                    tabs={html! {<QuestionAiHeaderActions available={selected_review.is_some()} active={*ai_tab} can_delete={*ai_can_delete}
                        original={Callback::from({let ai_tab=ai_tab.clone();let chosen=ai_tab_chosen.clone();move |_|{*chosen.borrow_mut()=true;ai_tab.set(false);}})}
                        ai={Callback::from({let ai_tab=ai_tab.clone();let chosen=ai_tab_chosen.clone();move |_|{*chosen.borrow_mut()=true;ai_tab.set(true);}})}
                        delete={Callback::from({let signal=ai_delete_signal.clone();move |_|signal.set(signal.wrapping_add(1))})}/>}}
                    leading={html!{
                        <>
                        <super::question_header_actions::QuestionHeaderActions
                            favorite={favorite_value.map(|value| value.favorite)} favorite_busy={*busy || favorite_pending}
                            favorite_label={if favorite_pending {"正在加载收藏状态"} else if favorite_failure.is_some() {"收藏状态加载失败"} else if starred {"取消收藏"} else {"收藏题目"}}
                            favorite_test_id="question-bank-favorite"
                            on_favorite={Callback::from(move |_| on_favorite.emit(Action::Favorite(qid.clone())))}
                            flashcard_count={if completed && !item.content_changed {card_preview.as_ref().map(|p| p.existing.len())} else {None}}
                            flashcard_can_create={card_preview.as_ref().is_some_and(favorite_can_create)}
                            flashcard_create_test_id="question-bank-practice-create-flashcard" flashcard_manage_test_id="question-bank-practice-manage-flashcard"
                            on_flashcard={Callback::from({let flashcard_open=flashcard_open.clone(); move |_| flashcard_open.set(true)})}/>
                        </>
                    }}
                    passage={Some(question_scroll.clone())}
                    can_scroll_up={question_edges.can_scroll_up}
                    can_scroll_down={question_edges.can_scroll_down}
                    question_index={item_index} target="题目" label="题目与解析滚动" />
                    if selected_review.is_some() {
                        if *ai_tab {
                            <QuestionAiReview delete_signal={*ai_delete_signal} on_delete_availability={Callback::from({let value=ai_can_delete.clone();move |available|value.set(available)})} key={format!("{}:{}", current.session.id, item.item.question_id)} csrf={props.csrf.clone()} question_id={item.item.question_id.clone()} question_content={question_content.clone()} question_scroll={question_scroll.clone()} on_question_scroll={on_question_scrolling.clone()}/>
                        }
                    }
                    if !*ai_tab || selected_review.is_none() {
                        <div ref={question_scroll.clone()} onscroll={on_question_scrolling.clone()}
                            class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4"
                            data-testid="question-bank-practice-question-scroll">{question_content.clone()}</div>
                    }
                </section>
            };
            let left_pane = if shared {
                material_pane
            } else {
                html! {
                    <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100"
                        data-testid="question-bank-practice-stem-pane">
                        <div class={classes!(QUESTION_BANK_PANE_HEADER,"question-bank-pane-header")}>
                            <strong>{"题干"}</strong>
                            <div class="ml-auto">
                                <QuestionBankScrollButtons passage={material_scroll.clone()}
                                    can_scroll_up={material_edges.can_scroll_up}
                                    can_scroll_down={material_edges.can_scroll_down}
                                    question_index={item_index} target="题干" label="题干滚动" />
                            </div>
                        </div>
                        <div ref={material_scroll.clone()} onscroll={on_material_scrolling.clone()}
                            class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4"
                            data-testid="question-bank-practice-material-scroll">
                            if let Some(q) = question {
                                <div class="study-markdown question-bank-passage" data-testid="question-bank-practice-stem">
                                    {render_question_bank_markdown(&q.stem)}
                                </div>
                            }
                        </div>
                    </section>
                }
            };
            let main = html! {
                <TrainingSplitWorkspace test_id="question-bank-practice-split" split_at_md={true} resizable={true}
                    left={left_pane} right={body}/>
            };
            let navigation_allowed = {
                let operation_lock = operation_lock.clone();
                let review_scrolls = review_scrolls.clone();
                let material_scroll = material_scroll.clone();
                let question_scroll = question_scroll.clone();
                let question_id = item.item.question_id.clone();
                let session_id = current.session.id.clone();
                Callback::from(move |()| {
                    if *operation_lock.borrow() {
                        return false;
                    }
                    let mut memory = review_scrolls.borrow_mut();
                    if memory.session.as_ref() == Some(&session_id)
                        && memory.question.as_ref() == Some(&question_id)
                    {
                        let saved = memory.positions.entry(question_id.clone()).or_default();
                        if let Some(node) = material_scroll.cast::<HtmlElement>() {
                            if !node.has_attribute("data-review-scroll-restoring") {
                                saved.0 = node.scroll_top();
                            }
                        }
                        if let Some(node) = question_scroll.cast::<HtmlElement>() {
                            if !node.has_attribute("data-review-scroll-restoring") {
                                saved.1 = node.scroll_top();
                            }
                        }
                    }
                    true
                })
            };
            let nav = html! {
                <fieldset disabled={*busy} class="min-w-0 shrink-0 border-0 p-0">
                <CompactQuestionNavigation current={index.clone()}
                    navigation_allowed={Some(navigation_allowed.clone())}
                    numbers={current.questions.iter().map(|q|q.item.number).collect::<Vec<_>>()}
                    answered={current.questions.iter().map(|q|!q.item.answer.is_empty()).collect::<Vec<_>>()}
                    groups={navigation_groups(&current.questions, &current.session.mode)}
                    grades={if completed {Some(current.questions.iter().map(|q| match q.item.grade.as_deref() {
                        Some("correct") => Some(true), Some("wrong") => Some(false), _ => None
                    }).collect::<Vec<_>>())} else {None}}/>
                </fieldset>
            };
            let display_seconds = if completed {
                current.session.elapsed_seconds
            } else {
                *elapsed
            };
            let paused = current.session.paused && !completed;
            let scope = question_scope(Some(item));
            let subtitle_base = match current.session.mode {
                PracticeMode::Paper => "套卷训练",
                PracticeMode::PaperModule => "模块训练",
                PracticeMode::Module => "模块训练",
                PracticeMode::Wrong => "错题复练",
                PracticeMode::Favorite => "收藏练习",
                PracticeMode::Drill => "专项练习",
            };
            let subtitle = match scope {
                Some(scope) => format!("{subtitle_base} · {scope}"),
                None => subtitle_base.to_string(),
            };
            let active_status = TrainingStatusCounts::active(
                current
                    .questions
                    .iter()
                    .filter(|question| !question.item.answer.is_empty())
                    .count(),
                current.questions.len(),
            );
            if !*history_forced {
                return html! {
                    <section class={WORKSPACE} data-testid="question-bank-practice-workspace">
                        <TrainingHeader class="flex shrink-0 flex-col gap-1 border-b border-base-content/10 pb-2 sm:flex-row sm:flex-nowrap sm:items-center sm:justify-between sm:gap-2">
                            <div class="flex min-w-0 flex-1 items-center gap-3">
                                <button class="btn btn-ghost btn-sm" disabled={*busy} onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Exit)})}>
                                    {if completed {"← 返回训练"} else {"← 暂停并退出"}}
                                </button>
                                <div class="relative min-w-0 flex-1">
                                    <button type="button" class="w-full min-w-0 text-left" data-testid="question-bank-practice-title"
                                        aria-expanded={title_open.to_string()} title={item.paper_title.clone()}
                                        onclick={Callback::from({let title_open=title_open.clone();move |_|title_open.set(!*title_open)})}>
                                        <strong class="block truncate text-sm">{item.paper_title.clone()}</strong>
                                        <span class="block truncate text-[11px] text-base-content/50" data-testid="question-bank-practice-scope">{subtitle.clone()}</span>
                                    </button>
                                    if *title_open {
                                        <div class="absolute left-0 top-full z-30 mt-1 max-w-[min(36rem,80vw)] rounded-lg bg-base-100 p-3 text-sm shadow-lg ring-1 ring-base-content/10"
                                            role="dialog" aria-label="完整试卷名称" data-testid="question-bank-practice-title-popover">
                                            <strong class="block whitespace-normal">{item.paper_title.clone()}</strong>
                                            <span class="mt-1 block whitespace-normal text-xs text-base-content/60">{subtitle.clone()}</span>
                                        </div>
                                    }
                                </div>
                            </div>
                            <div class="flex min-w-0 flex-wrap items-center gap-2 sm:flex-nowrap sm:justify-end">
                                <StudyGoalBars />
                                <CodexLimitBars />
                                if !completed {
                                    <TrainingStatusBar prefix="question-bank-practice" counts={active_status} mode={TrainingStatusMode::Active}/>
                                }
                                <button type="button" class="btn btn-ghost px-2 font-mono text-xl font-semibold tabular-nums"
                                    role="timer" data-testid="question-bank-practice-timer"
                                    title={if completed {"训练已结束"} else if paused {"点击继续计时"} else {"点击暂停计时"}}
                                    disabled={*busy || completed}
                                    onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Pause)})}>
                                    {format!("{:02}:{:02}:{:02}",display_seconds/3600,display_seconds/60%60,display_seconds%60)}
                                </button>
                                if !completed {
                                    <button class="btn btn-sm btn-outline btn-error" data-testid="question-bank-practice-submit"
                                        disabled={*busy} onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Submit)})}>{"交卷"}</button>
                                } else if let Some(score) = &current.session.score {
                                    <span class="flex min-w-0 flex-wrap items-center gap-2" data-testid="question-bank-practice-score">
                                        {score_summary(score)}
                                        <button type="button" class="btn btn-outline btn-sm" data-testid="question-bank-practice-records"
                                            onclick={Callback::from({
                                                let history_forced=history_forced.clone(); move |_|history_forced.set(true)
                                            })}>{"查看练习记录"}</button>
                                        <button type="button" class="btn btn-primary btn-sm" data-testid="question-bank-practice-repeat"
                                            disabled={*busy} onclick={Callback::from({
                                                let invoke=invoke.clone(); let id=current.session.id.clone();
                                                move |_|invoke.emit(Action::Repeat(id.clone()))
                                            })}>{"重做原题"}</button>
                                    </span>
                                }
                            </div>
                        </TrainingHeader>
                        {alert}
                        if *submit_confirm && !completed {
                            <div class="mb-2 flex flex-wrap items-center justify-center gap-2 rounded-box bg-warning/20 p-2" role="alert" data-testid="question-bank-submit-confirm">
                                <span>{"还有未答题，确定交卷吗？"}</span>
                                <button class="btn btn-sm btn-error" onclick={Callback::from({
                                    let invoke=invoke.clone();move |_|invoke.emit(Action::Submit)
                                })}>{"确认交卷"}</button>
                                <button class="btn btn-sm" onclick={Callback::from({
                                    let submit_confirm=submit_confirm.clone();move |_|submit_confirm.set(false)
                                })}>{"继续答题"}</button>
                            </div>
                        }
                        <div class="relative min-h-0 flex-1 overflow-hidden">
                            if *flashcard_open {
                                if let Some(preview) = &card_preview {
                                    <div class="h-full" data-testid="question-bank-practice-flashcard-dialog">
                                        <FavoriteFlashcardWorkspace key={item.item.question_id.clone()}
                                            question_id={item.item.question_id.clone()}
                                            preview={preview.clone()} csrf={props.csrf.clone()}
                                            on_changed={flashcard_changed.clone()}
                                            on_close={Callback::from({let flashcard_open=flashcard_open.clone();move |_|flashcard_open.set(false)})}/>
                                    </div>
                                }
                            } else if paused {
                                <div class="flex h-full items-center justify-center rounded-box bg-base-100" data-testid="question-bank-practice-paused">
                                    <div class="text-center space-y-4">
                                        <h2 class="text-xl font-bold">{"训练已暂停"}</h2>
                                        <p>{"暂停期间不可查看或修改题目。"}</p>
                                        <button class="btn btn-primary" onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Pause)})}>{"继续练习"}</button>
                                    </div>
                                </div>
                            } else {
                                <InkOverlay question_index={item_index} responsive={true} scroll_linked={true}
                                        persist_key={format!("question-bank-{}-{}",current.session.id,item.item.question_id)}>
                                    <TrainingSwipeSurface current={index.clone()} total={current.questions.len()}
                                        navigation_allowed={Some(navigation_allowed)}>
                                        {main}
                                    </TrainingSwipeSurface>
                                </InkOverlay>
                            }
                        </div>
                        if !*flashcard_open {
                            {nav}
                        }
                    </section>
                };
            }
        }
    }

    if *history_open || *history_forced {
        let filter = history_filter.as_ref();
        let only_unfinished = *history_unfinished;
        let visible = history
            .iter()
            .filter(|run| {
                (run.mode == PracticeMode::Drill) == (props.entry_mode == PracticeMode::Drill)
                    && history_filter_matches(run, filter)
                    && (!only_unfinished || run.status == "active")
            })
            .cloned()
            .collect::<Vec<_>>();
        let back_label = match props.entry_mode {
            PracticeMode::Paper => "返回套卷",
            PracticeMode::Drill => "返回专项",
            _ => "返回模块",
        };
        return html! {
            <TrainingSetupLayout test_id="question-bank-practice-history">
                <div class="question-bank-history-content space-y-3">
                    <div class="flex items-center justify-between gap-3">
                        <div>
                            <h2 class="text-2xl font-black tracking-tight">{"练习记录"}</h2>
                            <p class="mt-1 text-sm text-base-content/60">{if props.entry_mode == PracticeMode::Drill {"专项练习记录"} else {"套卷、模块、错题与收藏练习记录"}}</p>
                        </div>
                        <button class="btn btn-outline btn-sm" onclick={Callback::from({
                            let history_open=history_open.clone();let history_forced=history_forced.clone();
                            move |_|{history_open.set(false);history_forced.set(false);}
                        })}>{back_label}</button>
                    </div>
                    {alert}
                    <div class="flex flex-wrap items-center gap-2">
                        {for [
                            (None,"全部","question-bank-history-filter-all"),
                            (Some(PracticeMode::Paper),"套卷","question-bank-history-filter-paper"),
                            (Some(PracticeMode::Module),"模块","question-bank-history-filter-module"),
                            (Some(PracticeMode::Drill),"专项","question-bank-history-filter-drill"),
                            (Some(PracticeMode::Wrong),"错题","question-bank-history-filter-wrong"),
                            (Some(PracticeMode::Favorite),"收藏","question-bank-history-filter-favorite"),
                        ].into_iter().filter(|(target,_,_)| {
                            if props.entry_mode == PracticeMode::Drill { target.is_none() }
                            else { *target != Some(PracticeMode::Drill) }
                        }).map(|(target,label,id)| {
                            let history_filter=history_filter.clone();
                            let selected=*history_filter==target;
                            html! {<button type="button"
                                class={classes!("btn","btn-xs",if selected {"btn-primary"} else {"btn-outline"})}
                                data-testid={id} aria-pressed={selected.to_string()}
                                onclick={Callback::from(move |_|history_filter.set(target.clone()))}>{label}</button>}
                        })}
                        <button type="button" class={classes!("btn","btn-xs","ml-auto",if only_unfinished {"btn-primary"} else {"btn-ghost"})}
                            data-testid="question-bank-history-unfinished" aria-pressed={only_unfinished.to_string()}
                            onclick={Callback::from({
                                let history_unfinished=history_unfinished.clone();
                                move |_|history_unfinished.set(!*history_unfinished)
                            })}>{"仅看未完成"}</button>
                    </div>
                    if visible.is_empty() {
                        <p class="text-sm text-base-content/60" data-testid="question-bank-history-empty">{"还没有符合条件的练习记录。"}</p>
                    }
                    {for visible.iter().map(|run|{
                        let active=run.status=="active";
                        let id=run.id.clone();
                        let invoke=invoke.clone();
                        let stamp=Date::new(&JsValue::from_f64(run.created_at as f64));
                        let date=format!("{}-{:02}-{:02} {:02}:{:02}",
                            stamp.get_full_year(),stamp.get_month()+1,stamp.get_date(),
                            stamp.get_hours(),stamp.get_minutes());
                        let mode_label=history_mode_label(&run.mode);
                        let title=match run.mode {
                            PracticeMode::Paper | PracticeMode::PaperModule => run.paper_id.as_ref()
                                .and_then(|id|papers.iter().find(|paper|paper.id==*id))
                                .map(|paper|paper.title.clone())
                                .unwrap_or_else(||"真题训练".into()),
                            PracticeMode::Drill => run.collection_title.clone()
                                .unwrap_or_else(||"专项合集".into()),
                            _ => {
                                let module=run.module.clone().unwrap_or_else(||"真题训练".into());
                                match run.category.as_ref().filter(|category|!category.is_empty()) {
                                    Some(category)=>format!("{module} · {category}"),
                                    None=>module,
                                }
                            }
                        };
                        let headline=match run.mode {
                            PracticeMode::Wrong=>format!("{date} · 错题复练 · {title}"),
                            PracticeMode::Favorite=>format!("{date} · 收藏练习 · {title}"),
                            _=>format!("{date} · {title}"),
                        };
                        let (badge,badge_class)=if active {
                            if run.paused {("已暂停".to_string(),"badge-warning")} else {("进行中".to_string(),"badge-primary")}
                        } else {
                            run.score.as_ref()
                                .map(|score| {
                                    let gradable=score.total.saturating_sub(score.ungradable);
                                    if gradable==0 {
                                        ("已完成".to_string(),"badge-success")
                                    } else {
                                        (format!("{}/{} 正确",score.correct,gradable),"badge-success")
                                    }
                                })
                                .unwrap_or_else(||("已完成".to_string(),"badge-ghost"))
                        };
                        let detail=if active {
                            let answered=run.items.iter().filter(|item|!item.answer.is_empty()).count();
                            format!("{mode_label} · {} 题 · 已作答 {answered} 题 · 已用 {} 分钟",run.items.len(),run.elapsed_seconds/60)
                        } else if let Some(score)=&run.score {
                            let time=format!("{} 分 {:02} 秒",run.elapsed_seconds/60,run.elapsed_seconds%60);
                            match run.mode {
                                PracticeMode::Paper | PracticeMode::PaperModule=>format!(
                                    "{mode_label} · 用时 {time} · 正确 {} · 错误 {} · 未答 {}",
                                    score.correct,score.wrong,score.unanswered),
                                PracticeMode::Drill if score.ungradable==score.total=>format!(
                                    "{mode_label} · 用时 {time} · 共 {} 题 · 暂不判分",
                                    score.total),
                                PracticeMode::Drill=>format!(
                                    "{mode_label} · 用时 {time} · 可判分 {} 题 · 暂不可判分 {} 题",
                                    score.total.saturating_sub(score.ungradable),score.ungradable),
                                _=>format!(
                                    "{mode_label} · 用时 {time} · 正确率 {}%",
                                    score.correct.saturating_mul(100)/score.total.max(1)),
                            }
                        } else {
                            mode_label.to_string()
                        };
                        let can_repeat=!active;
                        html! {
                            <article class="rounded-box border border-base-content/10 bg-base-100 p-4" data-testid="question-bank-practice-history-item">
                                <div class="flex flex-wrap items-center justify-between gap-3">
                                    <div class="min-w-0">
                                        <div class="flex flex-wrap items-center gap-2">
                                            <strong class="min-w-0 truncate">{headline}</strong>
                                            <span class={classes!("badge","badge-sm",badge_class)}>{badge}</span>
                                        </div>
                                        <p class="mt-1 text-xs text-base-content/55">{detail}</p>
                                    </div>
                                    <div class="flex flex-wrap gap-2">
                                        if active {
                                            <button type="button" class="btn btn-primary btn-sm"
                                                disabled={*busy}
                                                onclick={Callback::from(move |_|invoke.emit(Action::Continue(id.clone())))}>
                                                {"继续练习"}
                                            </button>
                                        } else {
                                            <button type="button" class="btn btn-outline btn-sm"
                                                disabled={*busy}
                                                onclick={Callback::from({
                                                    let invoke=invoke.clone();
                                                    let id=id.clone();
                                                    move |_|invoke.emit(Action::Review(id.clone()))
                                                })}>
                                                {"查看原题与解析"}
                                            </button>
                                            if can_repeat {
                                                <button type="button" class="btn btn-ghost btn-sm"
                                                    disabled={*busy}
                                                    onclick={Callback::from(move |_|invoke.emit(Action::Repeat(id.clone())))}>
                                                    {"重做原题"}
                                                </button>
                                            }
                                        }
                                    </div>
                                </div>
                            </article>
                        }
                    })}
                </div>
            </TrainingSetupLayout>
        };
    }

    let active_resume = history
        .iter()
        .find(|run| run.status == "active" && history_matches_entry(&mode, run))
        .cloned();
    let start_action = {
        let invoke = invoke.clone();
        html! {
            <button class={TRAINING_PRIMARY_BUTTON} data-testid="question-bank-practice-start"
                disabled={*busy || *version_checking || props.csrf.is_empty() ||
                    (collection.is_some() && browse.is_empty()) ||
                    (collection.is_none() && *mode == PracticeMode::Paper && selected_paper.is_empty()) ||
                    (collection.is_none() && *mode == PracticeMode::Drill && selected_drill_collection.is_empty()) ||
                    (collection.is_none() && *mode == PracticeMode::Module && (!*filters_loaded || selected_module.is_empty()))}
                onclick={Callback::from(move |_| invoke.emit(Action::Start))}>
                {if *collection == Some(PracticeMode::Wrong) {"开始错题复练"}
                    else if *collection == Some(PracticeMode::Favorite) {"开始收藏题练习"}
                    else if *mode == PracticeMode::Paper {"开始套卷训练"}
                    else if *mode == PracticeMode::Drill {"开始专项练习"}
                    else {"开始模块训练"}}
            </button>
        }
    };
    let expanded_regions = web_sys::window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|width| width.as_f64())
        .filter(|width| *width >= 1024.0)
        .map(|_| 2)
        .unwrap_or(1);
    let collection_groups = collection_modules(browse, catalog.as_ref());
    let collection_filtered = browse
        .iter()
        .filter(|item| {
            (selected_module.is_empty() || item.module.as_deref() == Some(selected_module.as_str()))
                && (selected_category.is_empty()
                    || item.category.as_deref() == Some(selected_category.as_str()))
        })
        .collect::<Vec<_>>();
    let review_scope = (*collection).clone().unwrap_or(PracticeMode::Wrong);
    let review_items = collection_filtered
        .iter()
        .take(100)
        .map(|item| (*item).clone())
        .collect::<Vec<_>>();
    let filter_labels = module_filter_labels(&module_filters);
    let selection_scope = [selected_module.as_str(), selected_category.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let setup_heading = match collection.as_ref() {
        Some(PracticeMode::Wrong) => "我的错题",
        Some(PracticeMode::Favorite) => "我的收藏",
        _ if *mode == PracticeMode::Paper => "套卷训练",
        _ if *mode == PracticeMode::Drill => "专项练习",
        _ => "模块训练",
    };
    let setup_explanation = match collection.as_ref() {
        Some(PracticeMode::Wrong) => {
            format!("错题按现有模块与二级题型分类，共 {} 题", wrong_total)
        }
        Some(PracticeMode::Favorite) => {
            format!("收藏按现有模块与二级题型分类，共 {} 题", favorite_total)
        }
        _ if *mode == PracticeMode::Paper => "选择完整原卷，按原始题号顺序作答".to_string(),
        _ if *mode == PracticeMode::Drill => {
            "选择合集与专题，按原始顺序作答；没有可靠答案的题只记录完成情况".to_string()
        }
        _ => format!(
            "按一级模块与二级题型逐层筛选，每轮最多 {} 题",
            module_filters.count.unwrap_or(20)
        ),
    };
    let action_summary = match collection.as_ref() {
        Some(PracticeMode::Wrong) => "错题复练",
        Some(PracticeMode::Favorite) => "收藏题练习",
        _ if *mode == PracticeMode::Paper => "套卷训练",
        _ if *mode == PracticeMode::Drill => "专项练习",
        _ => "模块训练",
    };
    let action_detail = match collection.as_ref() {
        Some(_) if !collection_loaded => "题目尚未加载".into(),
        Some(_) => format!(
            "{} · {} 题 · 每轮最多 {} 题",
            if selection_scope.is_empty() {
                "全部分类"
            } else {
                &selection_scope
            },
            collection_filtered.len(),
            module_filters.count.unwrap_or(20)
        ),
        None if *mode == PracticeMode::Drill => {
            let selected = drill_collections
                .as_ref()
                .and_then(|list| list.iter().find(|c| c.id == *selected_drill_collection));
            match selected {
                Some(collection) if !selected_drill_paper.is_empty() => collection
                    .papers
                    .iter()
                    .find(|paper| paper.paper_id == *selected_drill_paper)
                    .map(|paper| {
                        format!(
                            "{} · {} · {} 题",
                            collection.title, paper.title, paper.questions
                        )
                    })
                    .unwrap_or_else(|| "请选择专题".into()),
                Some(collection) => format!(
                    "{} · 全部专题 · {} 题 · 可判分 {} 题",
                    collection.title, collection.questions, collection.gradable
                ),
                None => "选择合集与专题".into(),
            }
        }
        None if *mode == PracticeMode::Paper => papers
            .iter()
            .find(|paper| paper.id == *selected_paper)
            .map(|paper| format!("{} · {} 题", paper.title, paper.questions))
            .unwrap_or_else(|| "请选择完整原卷，按原始题号顺序作答".into()),
        None => [
            if selection_scope.is_empty() {
                "选择模块与二级题型".into()
            } else {
                selection_scope.clone()
            },
            filter_labels[2].clone(),
            filter_labels[0].clone(),
            if module_filters.regions.is_empty() {
                String::new()
            } else {
                filter_labels[1].clone()
            },
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · "),
    };
    let resume_view = active_resume.as_ref().map(|active| {
        let answered = active
            .items
            .iter()
            .filter(|item| !item.answer.is_empty())
            .count();
        let title = active
            .paper_id
            .as_ref()
            .and_then(|id| papers.iter().find(|paper| paper.id == *id))
            .map(|paper| paper.title.clone())
            .or_else(|| active.collection_title.clone())
            .unwrap_or_else(|| match (&active.module, &active.category) {
                (Some(module), Some(category)) => format!(
                    "{module} · {category} · {}",
                    history_mode_label(&active.mode)
                ),
                (Some(module), None) => format!("{module} · {}", history_mode_label(&active.mode)),
                _ => "真题训练".into(),
            });
        (
            answered,
            active.items.len(),
            active.elapsed_seconds / 60,
            title,
            active.id.clone(),
        )
    });
    let resume_card = html! {
            if let Some((answered,total,minutes,title,id)) = resume_view.as_ref() {
                <div class="rounded-box border border-primary/30 bg-primary/5 p-4" data-testid="question-bank-resume-card">
                    <div class="flex flex-wrap items-center justify-between gap-3">
                        <div class="min-w-0">
                            <p class="text-xs font-semibold text-primary">{"继续上次练习"}</p>
                            <strong class="mt-1 block truncate">{title.clone()}</strong>
                            <p class="mt-1 text-xs text-base-content/60">
                                {format!("已作答 {answered}/{total} 题 · 已用 {minutes} 分钟")}
                            </p>
                        </div>
                        <button type="button" class="btn btn-primary btn-sm" disabled={*busy}
                            onclick={Callback::from({let invoke=invoke.clone();let id=id.clone();move |_|invoke.emit(Action::Continue(id.clone()))})}>{"继续练习"}</button>
                    </div>
                </div>
            }
    };
    html! {
        <TrainingSetupLayout test_id="question-bank-practice-setup" wide={true} action={Some(start_action)}
            summary={action_summary}
            detail={action_detail}>
            <div class="question-bank-setup-content space-y-5">
            <div class="question-bank-heading flex items-center justify-between gap-3">
                <div><h2 class="text-2xl font-bold">
                    {setup_heading}
                </h2>
                <p class="mt-1 text-sm leading-6 text-base-content/60">{setup_explanation}</p>
                </div>
                <button type="button" class="btn btn-outline btn-sm" data-testid="question-bank-history-open"
                    onclick={Callback::from({let history_open=history_open.clone();move |_|history_open.set(true)})}>
                    {"练习记录"}
                </button>
            </div>
            {alert}
            if *mode != PracticeMode::Drill {
            <div class="flex flex-wrap gap-2" role="group" aria-label="题目浏览分类">
                {for [
                    (None,"全部题目","question-bank-collection-all",None),
                    (Some(PracticeMode::Wrong),"我的错题","question-bank-collection-wrong",Some(wrong_total)),
                    (Some(PracticeMode::Favorite),"我的收藏","question-bank-collection-favorite",Some(favorite_total)),
                ].into_iter().map(|(target,label,id,count)| {
                    let collection=collection.clone();
                    let selected_module=selected_module.clone();
                    let selected_category=selected_category.clone();
                    let selected=*collection==target;
                    html! {<button type="button"
                        class={classes!("btn","btn-sm",if selected {"btn-primary"} else {"btn-outline"})}
                        data-testid={id} aria-pressed={selected.to_string()}
                        onclick={Callback::from(move |_|{
                            collection.set(target.clone());
                            selected_module.set(String::new());
                            selected_category.set(String::new());
                        })}>
                        {label}
                        if let Some(count)=count {
                            <span class="question-bank-count" data-testid="question-bank-collection-count">{count}</span>
                        }
                    </button>}
                })}
            </div>
            }
            if collection.is_none() { {resume_card} }
            if collection.is_none() && *mode == PracticeMode::Drill {
                <section class="space-y-3" data-testid="question-bank-drill-setup">
                    <div class="flex items-end justify-between gap-3">
                        <div>
                            <h3 class="font-semibold">{"专项合集"}</h3>
                            <p class="text-xs text-base-content/60">{"先选合集，再选专题或全部专题；练习记录与真题分开统计。"}</p>
                        </div>
                        <span class="text-xs text-base-content/50" data-testid="question-bank-drill-total">
                            {drill_collections.as_ref().map(|list| format!("{} 个合集", list.len())).unwrap_or_else(|| "加载中…".into())}
                        </span>
                    </div>
                    if drill_collections.is_none() {
                        <p class="rounded-box bg-base-200 p-4 text-sm" data-testid="question-bank-drill-loading">{"正在加载专项合集…"}</p>
                    } else if drill_collections.as_ref().is_some_and(Vec::is_empty) {
                        <p class="rounded-box bg-base-200 p-4 text-sm" data-testid="question-bank-drill-empty">{"暂无专项合集，请先在题库管理同步合集目录。"}</p>
                    }
                    if let Some(list) = drill_collections.as_ref() {
                        <div class="question-bank-module-grid" role="group" aria-label="专项合集">
                        {for list.iter().map(|collection| {
                            let selected=*selected_drill_collection==collection.id;
                            let selected_collection=selected_drill_collection.clone();
                            let selected_paper=selected_drill_paper.clone();
                            let collection_id=collection.id.clone();
                            let summary=format!("已做 {}/{} 题 · 可判分 {}", collection.done, collection.questions, collection.gradable);
                            html! {
                                <QuestionBankModuleCard name={collection.title.clone()}
                                    summary={summary}
                                    value={collection.done} max={collection.questions} selected={selected}
                                    test_id="question-bank-drill-collection"
                                    on_select={Callback::from({
                                        let selected_collection=selected_collection.clone();
                                        let selected_paper=selected_paper.clone();
                                        let collection_id=collection_id.clone();
                                        move |_| {
                                            selected_collection.set(collection_id.clone());
                                            selected_paper.set(String::new());
                                        }
                                    })}>
                                    <QuestionBankCategoryButton label="全部专题"
                                        count={format!("{}/{}",collection.done,collection.questions)}
                                        selected={selected && selected_drill_paper.is_empty()}
                                        test_id="question-bank-drill-paper"
                                        on_select={Callback::from({
                                            let selected_collection=selected_collection.clone();
                                            let selected_paper=selected_paper.clone();
                                            let collection_id=collection_id.clone();
                                            move |_| {
                                                selected_collection.set(collection_id.clone());
                                                selected_paper.set(String::new());
                                            }
                                        })}/>
                                    {for collection.papers.iter().map(|paper| {
                                        let paper_selected=selected && *selected_drill_paper==paper.paper_id;
                                        let selected_collection=selected_collection.clone();
                                        let selected_paper=selected_paper.clone();
                                        let collection_id=collection_id.clone();
                                        let paper_id=paper.paper_id.clone();
                                        html! {
                                            <QuestionBankCategoryButton label={paper.title.clone()}
                                                count={format!("{}/{}",paper.done,paper.questions)}
                                                selected={paper_selected}
                                                test_id="question-bank-drill-paper"
                                                on_select={Callback::from(move |_| {
                                                    selected_collection.set(collection_id.clone());
                                                    selected_paper.set(paper_id.clone());
                                                })}/>
                                        }
                                    })}
                                </QuestionBankModuleCard>
                            }
                        })}
                        </div>
                    }
                </section>
            }
            if collection.is_none() && *mode == PracticeMode::Paper {
                <section class="space-y-3" aria-label="选择原卷">
                    <div class="flex items-end justify-between gap-3">
                        <div>
                            <h3 class="font-semibold">{"选择原卷"}</h3>
                            <p class="text-xs text-base-content/60">{"按地区展开年份与原卷；已选原卷高亮显示。"}</p>
                        </div>
                        <span class="text-xs text-base-content/50" data-testid="question-bank-paper-total">{paper_catalog_label(&papers)}</span>
                    </div>
                    if papers.is_empty() {
                        <p class="rounded-box bg-base-200 p-4 text-sm">{"暂无可用试卷，请先在题库管理同步原卷。"}</p>
                    }
                    <div class="question-bank-paper-grid">
                    {for {
                        let mut grouped = std::collections::BTreeMap::<String,std::collections::BTreeMap<i32,Vec<&PaperSummary>>>::new();
                        for paper in papers.iter().filter(|paper|paper.role=="primary") {
                            grouped.entry(paper.region.clone()).or_default()
                                .entry(paper.year).or_default().push(paper);
                        }
                        grouped.into_iter().enumerate().map(|(group_index,(region,years))| {
                            let total=years.values().map(Vec::len).sum::<usize>();
                            html! {
                            <details class="rounded-box border border-base-content/10 bg-base-100" open={group_index<expanded_regions} data-testid="question-bank-region-group">
                                <summary class="flex cursor-pointer select-none items-center justify-between px-4 py-3" data-testid="question-bank-region">
                                    <span class="font-semibold">{region.clone()}</span>
                                    <span class="text-xs text-base-content/50">{format!("{total}份")}</span>
                                </summary>
                                <div class="space-y-2 border-t border-base-content/10 p-3">
                                    {for years.into_iter().rev().enumerate().map(|(year_index,(year,items))|html! {
                                        <details class="question-bank-year-group rounded-box bg-base-200/50" open={year_index==0} data-testid="question-bank-year-group">
                                            <summary class="flex items-center justify-between px-3 py-2 text-sm" data-testid="question-bank-year">
                                                <span>{format!("{year} 年")}</span><span class="text-xs text-base-content/45">{format!("{} 份",items.len())}</span>
                                            </summary>
                                            <div class="grid gap-2 px-2 pb-2">
                                                {for items.into_iter().map(|paper|{
                                                    let id=paper.id.clone();
                                                    let active=*selected_paper==id;
                                                    let selected_paper=selected_paper.clone();
                                                    let selected_module=selected_module.clone();
                                                    html! {
                                                        <button type="button" class="question-bank-paper-option"
                                                            data-testid="question-bank-paper-option" aria-pressed={active.to_string()}
                                                            onclick={Callback::from(move |_|{selected_paper.set(id.clone());selected_module.set(String::new());})}>
                                                            <span class="min-w-0">
                                                                <strong class="block text-sm">{paper.title.clone()}</strong>
                                                                if !paper.variant.is_empty() { <span class="mt-0.5 block text-xs text-base-content/50">{paper.variant.clone()}</span> }
                                                            </span>
                                                            <span class="shrink-0 text-xs opacity-60">{format!("{}题",paper.questions)}</span>
                                                        </button>
                                                    }
                                                })}
                                            </div>
                                        </details>
                                    })}
                                </div>
                            </details>
                        }})
                    }}
                    </div>
                </section>
            }
            if collection.is_none() && *mode == PracticeMode::Module {
                <section class="space-y-3" data-testid="question-bank-cross-module-setup">
                    <div class="flex items-center justify-between gap-3">
                        <div>
                            <h3 class="font-semibold">{"专项模块"}</h3>
                            <p class="text-xs text-base-content/65">{"选择一级模块，再点二级题型精准刷题。"}</p>
                        </div>
                        <button type="button" class="btn btn-outline btn-sm" data-testid="module-drill-settings-open" disabled={!*filters_loaded || *saving_filters || *version_checking}
                            onclick={Callback::from({
                                let draft_filters=draft_filters.clone();
                                let module_filters=module_filters.clone();
                                let custom_years=custom_years.clone();
                                let settings_open=settings_open.clone();
                                let settings_error=settings_error.clone();
                                move |_| {
                                    settings_error.set(None);
                                    let now=js_sys::Date::new_0().get_full_year() as i32;
                                    let current=&*module_filters;
                                    let is_quick = current.year_min.is_none() && current.year_max.is_none()
                                        || [3,5,10].iter().any(|years| {
                                            current.year_min==Some(now-years+1) && current.year_max==Some(now)
                                        });
                                    custom_years.set(!is_quick);
                                    draft_filters.set((*module_filters).clone());
                                    settings_open.set(true);
                                }
                            })}>{"⚙ 自定义刷题"}</button>
                    </div>
                    <div class="question-bank-filter-summary" data-testid="module-drill-filter-summary">
                        <strong>{"当前筛选"}</strong>
                        {for filter_labels.iter().map(|label|html!{<span>{label}</span>})}
                    </div>
                    if let Some(info) = &*catalog {
                        <div class="question-bank-module-grid" role="group" aria-label="一级模块">
                            {for info.modules.iter().map(|module| {
                                let selected=*selected_module==module.name;
                                html! {
                                    <QuestionBankModuleCard name={module.name.clone()}
                                        summary={format!("已做 {}/{} 题",module.done,module.count)}
                                        value={module.done} max={module.count} selected={selected}
                                        test_id="question-bank-cross-module"
                                        on_select={Callback::from({
                                            let selected_module=selected_module.clone();
                                            let selected_category=selected_category.clone();
                                            let name=module.name.clone();
                                            move |_|{selected_module.set(name.clone());selected_category.set(String::new());}
                                        })}>
                                        <QuestionBankCategoryButton label="全部题型" count={format!("{}/{}",module.done,module.count)}
                                            selected={selected && selected_category.is_empty()} test_id="question-bank-cross-category"
                                            on_select={Callback::from({
                                                let selected_module=selected_module.clone();
                                                let selected_category=selected_category.clone();
                                                let name=module.name.clone();
                                                move |_|{selected_module.set(name.clone());selected_category.set(String::new());}
                                            })}/>
                                        {for module.categories.iter().map(|category|html! {
                                            <QuestionBankCategoryButton label={category.name.clone()}
                                                count={format!("{}/{}",category.done,category.count)}
                                                selected={selected && *selected_category==category.name} test_id="question-bank-cross-category"
                                                on_select={Callback::from({
                                                    let selected_module=selected_module.clone();
                                                    let selected_category=selected_category.clone();
                                                    let name=module.name.clone();
                                                    let category=category.name.clone();
                                                    move |_|{selected_module.set(name.clone());selected_category.set(category.clone());}
                                                })}/>
                                        })}
                                    </QuestionBankModuleCard>
                                }
                            })}
                        </div>
                    } else {
                        <p class="text-sm">{"暂无可用模块，请先在题库管理同步原卷。"}</p>
                    }
                </section>
            }
            if collection.is_some() {
                <section class="space-y-3" data-testid="question-bank-collection-browser">
                    if collection_loading {
                        <p class="text-sm text-base-content/60" role="status" data-testid="question-bank-collection-loading">
                            {if collection_loaded {"正在刷新题目，保留上次结果…"} else {"正在加载题目…"}}
                        </p>
                    }
                    if collection_failed {
                        if let Some(retry) = collection_retry.clone() {
                            <button type="button" class="btn btn-outline btn-sm" data-testid="question-bank-collection-retry"
                                onclick={Callback::from(move |_|retry.emit(()))}>{"重试加载"}</button>
                        }
                    }
                    <div class="question-bank-collections-layout">
                        <div class="space-y-3">
                            <div class="flex items-center justify-between gap-2">
                            <h3 class="font-semibold">{"按模块查看"}</h3>
                            <button type="button" class="btn btn-ghost btn-xs" data-testid="question-bank-collection-module"
                                aria-pressed={selected_module.is_empty().to_string()}
                                onclick={Callback::from({let selected_module=selected_module.clone();let selected_category=selected_category.clone();move |_|{selected_module.set(String::new());selected_category.set(String::new());}})}>
                                {format!("全部题目 {}",if collection_loaded {browse.len().to_string()} else {"…".into()})}
                            </button>
                            </div>
                            <div class="question-bank-collection-grid">
                            {for collection_groups.iter().map(|module| {
                                let selected=*selected_module==module.name;
                                html! {
                                    <QuestionBankModuleCard name={module.name.clone()} summary={format!("{} 题",module.count)}
                                        value={module.count} max={browse.len()} selected={selected}
                                        test_id="question-bank-collection-module"
                                        on_select={Callback::from({
                                            let selected_module=selected_module.clone();let selected_category=selected_category.clone();
                                            let name=module.name.clone();
                                            move |_|{selected_module.set(name.clone());selected_category.set(String::new());}
                                        })}>
                                        <QuestionBankCategoryButton label="全部" count={module.count.to_string()}
                                            selected={selected && selected_category.is_empty()} test_id="question-bank-collection-category"
                                            on_select={Callback::from({
                                                let selected_module=selected_module.clone();let selected_category=selected_category.clone();
                                                let name=module.name.clone();
                                                move |_|{selected_module.set(name.clone());selected_category.set(String::new());}
                                            })}/>
                                        {for module.categories.iter().map(|(category,count)|html! {
                                            <QuestionBankCategoryButton label={category.clone()} count={count.to_string()}
                                                selected={selected && *selected_category==*category} test_id="question-bank-collection-category"
                                                on_select={Callback::from({
                                                    let selected_module=selected_module.clone();let selected_category=selected_category.clone();
                                                    let name=module.name.clone();let category=category.clone();
                                                    move |_|{selected_module.set(name.clone());selected_category.set(category.clone());}
                                                })}/>
                                        })}
                                    </QuestionBankModuleCard>
                                }
                            })}
                            </div>
                        </div>
                        <div class="space-y-3 min-w-0">
                            <div class="flex min-h-6 items-center justify-between gap-2">
                                <h3 class="font-semibold">{"筛选结果"}</h3>
                                <span class="text-xs text-base-content/50" data-testid="question-bank-browse-count">
                                    {if collection_loaded {
                                        format!("{}{}{}题",selection_scope,if selection_scope.is_empty() {""} else {" · "},collection_filtered.len())
                                    } else {"题目数量待加载".into()}}
                                </span>
                            </div>
                            <div class="question-bank-results">
                            if !collection_loaded {
                                if !collection_loading {
                                    <p class="p-4 text-sm text-base-content/60">{"题目尚未加载，请重试。"}</p>
                                }
                            } else if collection_filtered.is_empty() {
                                <p class="p-4 text-sm text-base-content/60">{"这个分类还没有题目。"}</p>
                            } else {
                                <div class="question-bank-results-list" data-testid="question-bank-browse-list">
                                    {for collection_filtered.iter().take(100).enumerate().map(|(position,item)| {
                                        let content = html! {
                                            <>
                                                <div class="min-w-0">
                                                    <strong class="block truncate text-sm">{format!("{} {} · 第 {} 题",item.year,item.region,item.number)}</strong>
                                                    <p class="mt-0.5 truncate text-xs text-base-content/50">{format!("{} / {}",item.module.as_deref().unwrap_or("未分类"),item.category.as_deref().unwrap_or("未分类"))}</p>
                                                </div>
                                                <span class="question-bank-result-year">{item.year}</span>
                                            </>
                                        };
                                        let on_open = {
                                            let collection_review=collection_review.clone();
                                            let items=review_items.clone();
                                            let scope=review_scope.clone();
                                            move |_| collection_review.set(Some(CollectionReviewTarget{
                                                scope:scope.clone(),items:items.clone(),index:position
                                            }))
                                        };
                                        html! {
                                            <button type="button" class="question-bank-result-row w-full text-left"
                                                data-testid="question-bank-collection-item" title={item.paper_title.clone()}
                                                onclick={Callback::from(on_open)}>{content}</button>
                                        }
                                    })}
                                </div>
                            }
                            </div>
                        </div>
                    </div>
                </section>
            }
            if let Some(target) = &*collection_review {
                <CollectionQuestionReview items={target.items.clone()} initial_index={target.index}
                    scope={target.scope.clone()} csrf={props.csrf.clone()}
                    on_back={Callback::from({
                        let collection_review=collection_review.clone();
                        move |_|collection_review.set(None)
                    })}
                    on_favorite_change={Callback::from({
                        let invalidate=favorite_collection.invalidate.clone();
                        move |_|invalidate.emit(())
                    })}/>
            }
            if *settings_open && *mode == PracticeMode::Module {
                <section class="training-workspace fixed inset-0 z-[85] flex h-dvh flex-col overflow-hidden bg-base-200"
                    data-testid="module-drill-settings" role="dialog" aria-label="自定义刷题设置" aria-modal="true">
                    <header class="flex shrink-0 items-center gap-3 border-b border-base-content/10 px-4 py-3">
                        <button type="button" class="btn btn-ghost btn-sm" onclick={Callback::from({
                            let settings_open=settings_open.clone(); move |_|settings_open.set(false)
                        })}>{"← 返回"}</button>
                        <h3 class="text-lg font-bold">{"自定义刷题"}</h3>
                    </header>
                    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4 sm:p-6">
                        <div class="mx-auto max-w-2xl space-y-7">
                            if let Some(message) = &*settings_error {
                                <p class="rounded-box bg-error/10 px-3 py-2 text-sm text-error" role="alert">{message.clone()}</p>
                            }
                            <div class="space-y-2">
                                <h4 class="font-semibold">{"出题年份"}</h4>
                                <div class="grid grid-cols-4 gap-2">
                                    {for [(0,"不限"),(3,"近3年"),(5,"近5年"),(10,"近10年")].into_iter().map(|(years,label)|{
                                        let draft_filters=draft_filters.clone();
                                        let custom_years=custom_years.clone();
                                        let now=js_sys::Date::new_0().get_full_year() as i32;
                                        let selected=if years==0 {
                                            draft_filters.year_min.is_none() && draft_filters.year_max.is_none()
                                        } else {
                                            draft_filters.year_min==Some(now-years+1)
                                                && draft_filters.year_max==Some(now)
                                        };
                                        html!{<button type="button"
                                            class={classes!("btn","btn-sm",if selected {"btn-primary"} else {"btn-outline"})}
                                            aria-pressed={selected.to_string()}
                                            data-testid="module-filter-year"
                                            onclick={Callback::from(move |_|{
                                                let mut filters=(*draft_filters).clone();
                                                let now=js_sys::Date::new_0().get_full_year() as i32;
                                                filters.year_min=(years>0).then_some(now-years+1);
                                                filters.year_max=(years>0).then_some(now);
                                                draft_filters.set(filters);
                                                custom_years.set(false);
                                            })}>{label}</button>}
                                    })}
                                </div>
                                <button type="button"
                                    class={classes!("btn","btn-sm",if *custom_years {"btn-primary"} else {"btn-ghost"})}
                                    aria-pressed={(*custom_years).to_string()}
                                    data-testid="module-filter-custom-year-toggle"
                                    onclick={Callback::from({let custom_years=custom_years.clone();move |_|custom_years.set(!*custom_years)})}>{"自定义年份范围 ▾"}</button>
                                if *custom_years {
                                    <div class="grid grid-cols-2 gap-3">
                                        <label class="text-sm">{"起始年份"}
                                            <input class="input input-bordered mt-1 w-full" type="number" min="1900" max="2100"
                                                data-testid="module-filter-year-min" value={draft_filters.year_min.map(|v|v.to_string()).unwrap_or_default()}
                                                oninput={Callback::from({let draft_filters=draft_filters.clone(); move |e:InputEvent|{
                                                    let input:HtmlInputElement=e.target_unchecked_into();
                                                    let mut filters=(*draft_filters).clone();
                                                    filters.year_min=input.value().parse().ok();draft_filters.set(filters);
                                                }})}/>
                                        </label>
                                        <label class="text-sm">{"结束年份"}
                                            <input class="input input-bordered mt-1 w-full" type="number" min="1900" max="2100"
                                                data-testid="module-filter-year-max" value={draft_filters.year_max.map(|v|v.to_string()).unwrap_or_default()}
                                                oninput={Callback::from({let draft_filters=draft_filters.clone(); move |e:InputEvent|{
                                                    let input:HtmlInputElement=e.target_unchecked_into();
                                                    let mut filters=(*draft_filters).clone();
                                                    filters.year_max=input.value().parse().ok();draft_filters.set(filters);
                                                }})}/>
                                        </label>
                                    </div>
                                }
                            </div>
                            <div class="space-y-3">
                                <h4 class="font-semibold">{"地区筛选"}</h4>
                                <p class="text-xs text-base-content/60">{"不选地区默认使用全部地区；可选择只练或屏蔽指定地区。"}</p>
                                <div class="grid grid-cols-2 gap-2">
                                    {for [(false,"只练所选地区"),(true,"屏蔽所选地区")].into_iter().map(|(exclude,label)|{
                                        let draft_filters=draft_filters.clone();
                                        html!{<button type="button" class={classes!("btn","btn-sm",if draft_filters.exclude_regions==exclude {"btn-primary"} else {"btn-outline"})}
                                            data-testid="module-filter-region-mode" onclick={Callback::from(move |_|{
                                                let mut filters=(*draft_filters).clone();filters.exclude_regions=exclude;draft_filters.set(filters);
                                            })}>{label}</button>}
                                    })}
                                </div>
                                <div class="grid grid-cols-2 gap-2 sm:grid-cols-3">
                                    {for {
                                        let mut regions=papers.iter().filter(|p|p.role=="primary").map(|p|p.region.clone()).collect::<Vec<_>>();
                                        regions.sort();regions.dedup();
                                        regions.into_iter().map(|region|{
                                            let draft_filters=draft_filters.clone();
                                            let checked=draft_filters.regions.contains(&region);
                                            let label=region.clone();
                                            html!{<button type="button" data-testid="module-filter-region"
                                                aria-pressed={checked.to_string()}
                                                class={classes!("btn","btn-sm","justify-start",if checked {"btn-primary"} else {"btn-outline"})}
                                                onclick={Callback::from(move |_|{
                                                    let mut filters=(*draft_filters).clone();
                                                    if filters.regions.contains(&region) {filters.regions.retain(|r|r!=&region);}
                                                    else {filters.regions.push(region.clone());}
                                                    draft_filters.set(filters);
                                                })}>{label}</button>}
                                        })
                                    }}
                                </div>
                            </div>
                            <div class="space-y-2">
                                <h4 class="font-semibold">{"每轮题量"}</h4>
                                <div class="grid grid-cols-2 gap-2">
                                    {for [10usize,20].into_iter().map(|value|{
                                        let draft_filters=draft_filters.clone();
                                        html!{<button type="button" data-testid="module-filter-count"
                                            aria-pressed={(draft_filters.count.unwrap_or(20)==value).to_string()}
                                            class={classes!("btn",if draft_filters.count.unwrap_or(20)==value {"btn-primary"} else {"btn-outline"})}
                                            onclick={Callback::from(move |_|{
                                                let mut filters=(*draft_filters).clone();filters.count=Some(value);draft_filters.set(filters);
                                            })}>{format!("{value}题" )}</button>}
                                    })}
                                </div>
                            </div>
                        </div>
                    </div>
                    <footer class="shrink-0 border-t border-base-content/10 bg-base-100 px-4 py-3 pb-[max(0.75rem,env(safe-area-inset-bottom))]">
                        <div class="mx-auto flex max-w-2xl items-center gap-3">
                            <button type="button" class="btn btn-ghost" data-testid="module-filter-reset"
                                onclick={Callback::from({let draft_filters=draft_filters.clone();let custom_years=custom_years.clone(); move |_|{
                                    draft_filters.set(ModulePracticeFilters::default());custom_years.set(false);
                                }})}>{"恢复默认"}</button>
                            <button type="button" class="btn btn-primary flex-1" data-testid="module-filter-save"
                                disabled={*saving_filters || matches!((draft_filters.year_min,draft_filters.year_max),(Some(a),Some(b)) if a>b)}
                                onclick={Callback::from({
                                    let draft_filters=draft_filters.clone();
                                    let module_filters=module_filters.clone();
                                    let settings_open=settings_open.clone();
                                    let settings_error=settings_error.clone();
                                    let csrf=props.csrf.clone();
                                    let saving_filters=saving_filters.clone();
                                    move |_|{
                                        let next=(*draft_filters).clone();
                                        let module_filters=module_filters.clone();
                                        let settings_open=settings_open.clone();
                                        let settings_error=settings_error.clone();
                                        let csrf=csrf.clone();
                                        let saving_filters=saving_filters.clone();
                                        settings_error.set(None);
                                        saving_filters.set(true);
                                        spawn_local(async move {
                                            match post_json_response::<_, ModulePracticeFilters>(MODULE_SETTINGS_URL, &csrf, &next, "刷题设置").await {
                                                Ok(saved) => {
                                                    module_filters.set(saved);
                                                    settings_open.set(false);
                                                }
                                                Err(message) => settings_error.set(Some(message)),
                                            }
                                            saving_filters.set(false);
                                        });
                                    }
                                })}>{"保存设置"}</button>
                        </div>
                    </footer>
                </section>
            }
            </div>
        </TrainingSetupLayout>
    }
}
