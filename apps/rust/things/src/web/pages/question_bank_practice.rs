//! Browse original papers and categories; share one workspace for practice and review.
use super::material_scroll::{scroll_edges, MaterialScrollButton, ScrollEdges};
use super::*;
use hyz_things::domain::question_bank::{
    CreatePracticeRequest, FavoriteState, ModulePracticeFilters, PaperSummary, PracticeBrowseItem,
    PracticeCatalog, PracticeMode, PracticeReview, PracticeSession, PracticeWorkspace,
    SavePracticeAnswer, SetFavorite, SetPracticePaused,
};

const ROOT: &str = "/api/v1/question-bank";

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
    }
}

fn history_filter_matches(run: &PracticeSession, filter: Option<&PracticeMode>) -> bool {
    match filter {
        None => true,
        Some(PracticeMode::Paper) => {
            matches!(run.mode, PracticeMode::Paper | PracticeMode::PaperModule)
        }
        Some(PracticeMode::Module) => run.mode == PracticeMode::Module,
        Some(mode) => run.mode == *mode,
    }
}

fn history_matches_entry(entry: &PracticeMode, run: &PracticeSession) -> bool {
    let paper_run = matches!(run.mode, PracticeMode::Paper | PracticeMode::PaperModule)
        || matches!(run.mode, PracticeMode::Wrong | PracticeMode::Favorite)
            && run.paper_id.is_some();
    if *entry == PracticeMode::Paper {
        paper_run
    } else {
        !paper_run
    }
}
const MODULE_FILTERS_STORAGE: &str = "hyz-question-bank-module-settings-v1";

fn read_module_filters() -> ModulePracticeFilters {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(MODULE_FILTERS_STORAGE).ok().flatten())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn persist_module_filters(filters: &ModulePracticeFilters) -> bool {
    let Some(storage) = web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    else {
        return false;
    };
    let Ok(json) = serde_json::to_string(filters) else {
        return false;
    };
    storage.set_item(MODULE_FILTERS_STORAGE, &json).is_ok()
}

fn catalog_filter_url(filters: &ModulePracticeFilters) -> String {
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
    format!("{ROOT}/practice/catalog?{}", query.finish())
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

async fn load_workspace(id: &str) -> Result<PracticeWorkspace, String> {
    fetch_json(&format!("{ROOT}/practice/sessions/{id}"), "真题练习").await
}

fn answer_options(markdown: &str) -> Vec<(String, String)> {
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

#[function_component(QuestionBankTraining)]
pub(crate) fn question_bank_training(props: &QuestionBankTrainingProps) -> Html {
    let papers = use_state(Vec::<PaperSummary>::new);
    let history = use_state(Vec::<PracticeSession>::new);
    let history_open = use_state(|| props.show_history);
    let history_filter = use_state(|| None::<PracticeMode>);
    let history_unfinished = use_state(|| false);
    let selected_paper = use_state(String::new);
    let selected_module = use_state(String::new);
    let catalog = use_state(|| None::<PracticeCatalog>);
    let selected_category = use_state(String::new);
    let module_filters = use_state(read_module_filters);
    let draft_filters = use_state(read_module_filters);
    let settings_open = use_state(|| false);
    let custom_years = use_state(|| false);
    let collection = use_state(|| None::<PracticeMode>);
    let browse = use_state(Vec::<PracticeBrowseItem>::new);
    let wrong_total = use_state(|| 0usize);
    let favorite_total = use_state(|| 0usize);

    let mode = use_state(|| props.entry_mode.clone());
    let workspace = use_state(|| None::<PracticeWorkspace>);
    let review = use_state(|| None::<PracticeReview>);
    let index = use_state(|| 0usize);
    let multi_select = use_state(|| false);
    let favorite = use_state(|| false);
    let busy = use_state(|| false);
    let error = use_state(|| None::<String>);
    let submit_confirm = use_state(|| false);
    let elapsed = use_state(|| 0u64);
    let seconds_ref = use_mut_ref(|| 0u64);
    let refresh = use_state(|| 0u64);
    let material_scroll = use_node_ref();
    let question_scroll = use_node_ref();
    let material_edges = use_state(ScrollEdges::default);
    let question_edges = use_state(ScrollEdges::default);
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
        let history = history.clone();
        let error = error.clone();
        use_effect_with(*refresh, move |_| {
            spawn_local(async move {
                match fetch_json::<Vec<PaperSummary>>(&format!("{ROOT}/papers"), "真题试卷列表")
                    .await
                {
                    Ok(list) => papers.set(list),
                    Err(e) => error.set(Some(e)),
                }
                if let Ok(items) = fetch_json::<Vec<PracticeSession>>(
                    &format!("{ROOT}/practice/history"),
                    "练习历史",
                )
                .await
                {
                    history.set(items);
                }
            });
            || ()
        });
    }
    {
        // The collection chips show how much is waiting. Browse results are
        // already small metadata rows, so reuse that endpoint instead of
        // introducing a second count-only contract.
        let wrong_total = wrong_total.clone();
        let favorite_total = favorite_total.clone();
        use_effect_with(*refresh, move |_| {
            spawn_local(async move {
                let filters = ModulePracticeFilters::default();
                if let Ok(items) = fetch_json::<Vec<PracticeBrowseItem>>(
                    &browse_filter_url(
                        &PracticeMode::Wrong,
                        &PracticeMode::Paper,
                        "",
                        "",
                        "",
                        &filters,
                    ),
                    "错题数量",
                )
                .await
                {
                    wrong_total.set(items.len());
                }
                if let Ok(items) = fetch_json::<Vec<PracticeBrowseItem>>(
                    &browse_filter_url(
                        &PracticeMode::Favorite,
                        &PracticeMode::Paper,
                        "",
                        "",
                        "",
                        &filters,
                    ),
                    "收藏数量",
                )
                .await
                {
                    favorite_total.set(items.len());
                }
            });
            || ()
        });
    }
    {
        let filters = (*module_filters).clone();
        let catalog = catalog.clone();
        let error = error.clone();
        use_effect_with((filters, *refresh), move |(filters, _)| {
            let url = catalog_filter_url(filters);
            spawn_local(async move {
                match fetch_json::<PracticeCatalog>(&url, "模块题型进度").await {
                    Ok(result) => catalog.set(Some(result)),
                    Err(message) => error.set(Some(message)),
                }
            });
            || ()
        });
    }
    {
        let current = workspace.as_ref().as_ref().map(|w| w.session.clone());
        let immersive = props.on_immersive_change.clone();
        let paused = props.on_paper_pause_change.clone();
        use_effect_with(current, move |value| {
            let active = value.as_ref().is_some_and(|s| s.status == "active");
            immersive.emit(active);
            paused.emit(value.as_ref().is_some_and(|s| s.paused));
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
        let index_value = *index;
        let source = workspace
            .as_ref()
            .as_ref()
            .and_then(|w| w.questions.get(index_value))
            .map(|q| q.item.question_id.clone());
        let favorite = favorite.clone();
        use_effect_with(source, move |source| {
            favorite.set(false);
            if let Some(id) = source {
                let id = id.clone();
                spawn_local(async move {
                    if let Ok(value) = fetch_json::<FavoriteState>(
                        &format!("{ROOT}/practice/favorites/{id}"),
                        "真题收藏",
                    )
                    .await
                    {
                        favorite.set(value.favorite);
                    }
                });
            }
            || ()
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
        // Only saved collections use the browse endpoint now. The setup cards
        // provide module/category filtering locally, so switching a chip does
        // not refetch or reset the result list.
        let query = collection.as_ref().map(|scope| {
            browse_filter_url(
                scope,
                &PracticeMode::Paper,
                "",
                "",
                "",
                &ModulePracticeFilters::default(),
            )
        });
        let browse = browse.clone();
        let error = error.clone();
        use_effect_with((query, *refresh), move |(query, _)| {
            browse.set(Vec::new());
            if let Some(url) = query.clone() {
                spawn_local(async move {
                    match fetch_json::<Vec<PracticeBrowseItem>>(&url, "真题分类浏览").await {
                        Ok(items) => browse.set(items),
                        Err(message) => error.set(Some(message)),
                    }
                });
            }
            || ()
        });
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
    let invoke = {
        let csrf = props.csrf.clone();
        let papers = papers.clone();
        let history = history.clone();
        let mode = mode.clone();
        let collection = collection.clone();
        let selected_paper = selected_paper.clone();
        let selected_module = selected_module.clone();
        let selected_category = selected_category.clone();
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
        Callback::from(move |action: Action| {
            if *busy {
                return;
            }
            busy.set(true);
            error.set(None);
            let csrf = csrf.clone();
            let papers = papers.clone();
            let history = history.clone();
            let mode = mode.clone();
            let collection = collection.clone();
            let selected_paper = selected_paper.clone();
            let selected_module = selected_module.clone();
            let selected_category = selected_category.clone();
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
            spawn_local(async move {
                let result: Result<(), String> = async {
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
                            elapsed.set(loaded.session.elapsed_seconds);
                            workspace.set(Some(loaded));
                            review.set(None);
                            index.set(0);
                            multi_select.set(false);
                            submit_confirm.set(false);
                        }
                        Action::Continue(id) => {
                            let loaded = load_workspace(&id).await?;
                            elapsed.set(loaded.session.elapsed_seconds);
                            workspace.set(Some(loaded));
                            review.set(None);
                            index.set(0);
                            multi_select.set(false);
                            submit_confirm.set(false);
                        }
                        Action::Repeat(id) => {
                            let run = history
                                .iter()
                                .find(|run| run.id == id)
                                .cloned()
                                .ok_or("练习记录不存在")?;
                            let practice_mode = run.mode.clone();
                            let request = CreatePracticeRequest {
                                mode: practice_mode.clone(),
                                paper_id: run.paper_id.clone(),
                                module: run.module.clone(),
                                category: run.category.clone(),
                                filters: match practice_mode {
                                    PracticeMode::Module => (*module_filters).clone(),
                                    PracticeMode::Wrong | PracticeMode::Favorite => {
                                        ModulePracticeFilters {
                                            count: Some(20),
                                            ..Default::default()
                                        }
                                    }
                                    _ => ModulePracticeFilters::default(),
                                },
                            };
                            let session: PracticeSession = post_json_response(
                                &format!("{ROOT}/practice/sessions"),
                                &csrf,
                                &request,
                                "重新开始真题练习",
                            )
                            .await?;
                            let loaded = load_workspace(&session.id).await?;
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
                            workspace.set(Some(plain));
                            review.set(Some(loaded));
                            index.set(0);
                        }
                        Action::Answer(choice) => {
                            let active = (*workspace).clone().ok_or("训练不存在")?;
                            let q = active.questions.get(*index).ok_or("题位不存在")?;
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
                            let _: PracticeSession = post_json_response(
                                &format!(
                                    "{ROOT}/practice/sessions/{}/answers/{}",
                                    active.session.id, question_id
                                ),
                                &csrf,
                                &SavePracticeAnswer {
                                    answer: answer.clone(),
                                },
                                "保存真题答案",
                            )
                            .await?;
                            // The server has saved the answer. Keep the current
                            // workspace and avoid downloading every question again.
                            let mut updated = active;
                            if let Some(question) = updated.questions.get_mut(*index) {
                                question.item.answer = answer.clone();
                            }
                            if let Some(question) = updated
                                .session
                                .items
                                .iter_mut()
                                .find(|question| question.question_id == question_id)
                            {
                                question.answer = answer;
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
                                    if matches!(action, Action::Exit) {
                                        workspace.set(None);
                                    } else {
                                        workspace
                                            .set(Some(load_workspace(&active.session.id).await?));
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
                                elapsed.set(completed.elapsed_seconds.max(*elapsed));
                                workspace.set(Some(load_workspace(&completed.id).await?));
                                let detail: PracticeReview = fetch_json(
                                    &format!("{ROOT}/practice/sessions/{}/review", completed.id),
                                    "真题解析",
                                )
                                .await?;
                                review.set(Some(detail));
                                submit_confirm.set(false);
                                refresh.set(*refresh + 1);
                            }
                        }
                        Action::Favorite(id) => {
                            let updated: FavoriteState = post_json_response(
                                &format!("{ROOT}/practice/favorites/{id}"),
                                &csrf,
                                &SetFavorite {
                                    favorite: !*favorite,
                                },
                                "收藏真题",
                            )
                            .await?;
                            favorite.set(updated.favorite);
                        }
                    }
                    Ok(())
                }
                .await;
                if let Err(e) = result {
                    error.set(Some(e));
                }
                busy.set(false);
            });
        })
    };
    let alert = if let Some(message) = error.as_ref() {
        html! { <p class="rounded-box bg-error/10 px-3 py-2 text-sm text-error" role="alert" data-testid="question-bank-error">{message.clone()}</p> }
    } else {
        Html::default()
    };

    if let Some(current) = workspace.as_ref() {
        let item_index = (*index).min(current.questions.len().saturating_sub(1));
        if let Some(item) = current.questions.get(item_index) {
            let completed = current.session.status == "completed";
            let selected_review = review.as_ref().and_then(|r| r.questions.get(item_index));
            let question = item.question.as_ref();
            let options = question
                .map(|q| answer_options(&q.options_markdown))
                .unwrap_or_default();
            let qid = item.item.question_id.clone();
            let starred = *favorite;
            let on_favorite = invoke.clone();
            let on_scrolling = {
                let refresh_edges = refresh_edges.clone();
                Callback::from(move |_: web_sys::Event| refresh_edges.emit(()))
            };
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
                    <div class={classes!(TRAINING_PANE_HEADER,"question-bank-pane-header")}>
                        <strong>{"原卷材料"}</strong>
                        <div class="ml-auto flex shrink-0 items-center gap-2">
                            <span class="hidden max-w-40 truncate text-[11px] text-base-content/45 sm:inline">{material_meta.clone()}</span>
                            <QuestionBankScrollButtons passage={material_scroll.clone()}
                                can_scroll_up={material_edges.can_scroll_up}
                                can_scroll_down={material_edges.can_scroll_down}
                                question_index={item_index} target="材料" label="材料滚动" />
                        </div>
                    </div>
                    <div ref={material_scroll.clone()} onscroll={on_scrolling.clone()}
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
            let body = html! {
                <section class="flex h-full min-h-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100" data-testid="question-bank-practice-question-pane">
                    <div class={classes!(TRAINING_PANE_HEADER,"question-bank-pane-header")}>
                        <button type="button" class="btn btn-ghost btn-sm shrink-0" data-testid="question-bank-favorite"
                            onclick={Callback::from(move |_| on_favorite.emit(Action::Favorite(qid.clone())))}
                            aria-label={if starred {"取消收藏"} else {"收藏题目"}}>
                            {if starred {"★"} else {"☆"}}
                        </button>
                        <strong class="min-w-0 truncate text-sm" data-testid="question-bank-practice-heading">
                            {if completed {
                                format!("原卷第 {} 题 · {}/{}", item.item.number, item_index + 1, current.questions.len())
                            } else {
                                format!("第 {}/{} 题", item_index + 1, current.questions.len())
                            }}
                        </strong>
                        <div class="ml-auto flex shrink-0 items-center gap-1">
                            <QuestionBankScrollButtons passage={question_scroll.clone()}
                                can_scroll_up={question_edges.can_scroll_up}
                                can_scroll_down={question_edges.can_scroll_down}
                                question_index={item_index} target="题目" label="题目与解析滚动" />
                            <InkToolbarSlot />
                        </div>
                    </div>
                    <div ref={question_scroll.clone()} onscroll={on_scrolling.clone()}
                        class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4 space-y-4"
                        data-testid="question-bank-practice-question-scroll">
                    if item.content_changed {
                        <p class="text-warning">{"题目源文件已更新或暂不可用，不能用新版题面替代旧题。"}</p>
                    } else if let Some(q) = question {
                        if shared {
                            <div class="study-markdown question-bank-passage" data-testid="question-bank-practice-stem">{render_question_bank_markdown(&q.stem)}</div>
                        }
                        if !q.issues.is_empty() && !completed {
                            <p class="text-xs text-base-content/60">{"本题内容尚待核验，可能不计入成绩。"}</p>
                        }
                        <div class="space-y-2">
                            {for options.iter().map(|(label, markdown)| {
                                let chosen = item.item.answer.contains(label);
                                let correct = selected_review.is_some_and(|r|r.correct_answer.contains(label));
                                let grade = item.item.grade.as_deref();
                                if completed {
                                    let style = if correct && grade != Some("ungradable") {
                                        "flex items-start gap-2.5 rounded-xl border border-success bg-success/10 p-3 text-sm leading-6 text-success"
                                    } else if chosen && grade == Some("wrong") {
                                        "flex items-start gap-2.5 rounded-xl border border-error bg-error/10 p-3 text-sm leading-6 text-error"
                                    } else if chosen && grade == Some("correct") {
                                        "flex items-start gap-2.5 rounded-xl border border-success bg-success/10 p-3 text-sm leading-6 text-success"
                                    } else {
                                        "flex items-start gap-2.5 rounded-xl border border-base-content/20 p-3 text-sm leading-6"
                                    };
                                    html! {<div class={style} data-testid="question-bank-practice-choice">
                                        <span class="shrink-0 font-bold">{format!("{label}.")}</span>
                                        <span class="study-markdown min-w-0">{render_question_bank_markdown(markdown)}</span>
                                    </div>}
                                } else {
                                    let style = if chosen {
                                        "border-primary bg-primary text-primary-content"
                                    } else {
                                        "border-base-content/20 bg-base-100 hover:border-primary/50"
                                    };
                                    let action = invoke.clone(); let choice = label.clone();
                                    html! {<button type="button" class={classes!("flex","w-full","items-start","gap-2.5","rounded-xl","border","p-3","text-left","text-sm","leading-6","transition-colors","disabled:opacity-60",style)}
                                        aria-pressed={chosen.to_string()} data-testid="question-bank-practice-choice"
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
                                <input type="checkbox" class="checkbox checkbox-sm" checked={*multi_select}
                                    onchange={Callback::from({let multi_select=multi_select.clone();move |e: Event| {
                                        let checkbox: HtmlInputElement=e.target_unchecked_into();
                                        multi_select.set(checkbox.checked());
                                    }})}/>
                                {"多选作答（题目明确允许多选时开启）"}
                            </label>
                        }
                        if let Some(r) = selected_review {
                            <p class="text-sm font-semibold" data-testid="question-bank-practice-review-answer">
                                {format!("你的答案：{} · 正确答案：{}",
                                    if item.item.answer.is_empty() {"未答".into()} else {item.item.answer.join("、")},
                                    if r.correct_answer.is_empty() {"暂无可靠答案".into()} else {r.correct_answer.join("、")})}
                            </p>
                            <div class="border-t border-base-content/10 pt-3">
                                <h3 class="font-bold">{"原始解析"}</h3>
                                <div class="study-markdown mt-2 text-sm leading-7 text-base-content/75">{if r.explanation.is_empty() {html!{"暂无解析"}} else {render_question_bank_markdown(&r.explanation)}}</div>
                                <h3 class="mt-3 font-bold">{"考点"}</h3>
                                <p class="text-sm text-base-content/60">{if q.topics.is_empty() {"暂无考点".into()} else {q.topics.join("、")}}</p>
                            </div>
                        }
                    }
                    </div>
                </section>
            };
            let left_pane = if shared {
                material_pane
            } else {
                html! {
                    <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100"
                        data-testid="question-bank-practice-stem-pane">
                        <div class={classes!(TRAINING_PANE_HEADER,"question-bank-pane-header")}>
                            <strong>{"题干"}</strong>
                            <div class="ml-auto">
                                <QuestionBankScrollButtons passage={material_scroll.clone()}
                                    can_scroll_up={material_edges.can_scroll_up}
                                    can_scroll_down={material_edges.can_scroll_down}
                                    question_index={item_index} target="题干" label="题干滚动" />
                            </div>
                        </div>
                        <div ref={material_scroll.clone()} onscroll={on_scrolling.clone()}
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
                <TrainingSplitWorkspace test_id="question-bank-practice-split" split_at_md={true}
                    left={left_pane} right={body}/>
            };
            let nav = html! {
                <CompactQuestionNavigation current={index.clone()}
                    numbers={current.questions.iter().map(|q|q.item.number).collect::<Vec<_>>()}
                    answered={current.questions.iter().map(|q|!q.item.answer.is_empty()).collect::<Vec<_>>()}
                    grades={if completed {Some(current.questions.iter().map(|q| match q.item.grade.as_deref() {
                        Some("correct") => Some(true), Some("wrong") => Some(false), _ => None
                    }).collect::<Vec<_>>())} else {None}}/>
            };
            let display_seconds = if completed {
                current.session.elapsed_seconds
            } else {
                *elapsed
            };
            let paused = current.session.paused && !completed;
            let subtitle = match (&current.session.module, &current.session.category) {
                (Some(module), Some(category)) => format!("模块训练 · {module} · {category}"),
                (Some(module), None) => format!("模块训练 · {module}"),
                _ => "套卷训练".to_string(),
            };
            return html! {
                <section class={WORKSPACE} data-testid="question-bank-practice-workspace">
                    <header class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-base-content/10 pb-2">
                        <div class="flex min-w-0 items-center gap-3">
                            <button class="btn btn-ghost btn-sm" disabled={*busy} onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Exit)})}>
                                {if completed {"← 返回训练"} else {"← 暂停并退出"}}
                            </button>
                            <div class="min-w-0">
                                <strong class="block truncate text-sm">{item.paper_title.clone()}</strong>
                                <span class="block truncate text-[11px] text-base-content/50">{subtitle.clone()}</span>
                            </div>
                        </div>
                        <div class="flex shrink-0 items-center gap-2">
                            <span class="hidden text-xs text-base-content/55 sm:inline">{"学习"}</span>
                            <StudyTimeInline />
                            <span class="font-mono text-xl font-semibold tabular-nums" role="timer" data-testid="question-bank-practice-timer">{format!("{:02}:{:02}:{:02}",display_seconds/3600,display_seconds/60%60,display_seconds%60)}</span>
                            if !completed {
                                <button class="btn btn-sm btn-outline" data-testid="question-bank-practice-pause"
                                    disabled={*busy} onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Pause)})}>
                                    {if paused {"继续"} else {"暂停"}}
                                </button>
                                <button class="btn btn-sm btn-outline btn-error" data-testid="question-bank-practice-submit"
                                    disabled={*busy} onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Submit)})}>{"交卷"}</button>
                            } else if let Some(score) = &current.session.score {
                                <span class="flex shrink-0 items-center gap-3 font-semibold" data-testid="question-bank-practice-score">
                                    <span class="text-success">{format!("正确 {}",score.correct)}</span>
                                    <span class="text-error">{format!("错误 {}",score.wrong)}</span>
                                    if score.unanswered > 0 {
                                        <span class="text-base-content/60">{format!("未答 {}",score.unanswered)}</span>
                                    }
                                </span>
                            }
                        </div>
                    </header>
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
                        if paused {
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
                                <TrainingSwipeSurface current={index.clone()} total={current.questions.len()}>
                                    {main}
                                </TrainingSwipeSurface>
                            </InkOverlay>
                        }
                    </div>
                    {nav}
                </section>
            };
        }
    }

    if *history_open {
        let filter = history_filter.as_ref();
        let only_unfinished = *history_unfinished;
        let visible = history
            .iter()
            .filter(|run| {
                history_filter_matches(run, filter) && (!only_unfinished || run.status == "active")
            })
            .cloned()
            .collect::<Vec<_>>();
        let back_label = if *mode == PracticeMode::Paper {
            "返回套卷"
        } else {
            "返回模块"
        };
        return html! {
            <TrainingSetupLayout test_id="question-bank-practice-history">
                <div class="question-bank-history-content space-y-3">
                    <div class="flex items-center justify-between gap-3">
                        <div>
                            <h2 class="text-2xl font-black tracking-tight">{"练习记录"}</h2>
                            <p class="mt-1 text-sm text-base-content/60">{"套卷、模块、错题与收藏共用同一份历史记录"}</p>
                        </div>
                        <button class="btn btn-outline btn-sm" onclick={Callback::from({
                            let history_open=history_open.clone();move |_|history_open.set(false)
                        })}>{back_label}</button>
                    </div>
                    {alert}
                    <div class="flex flex-wrap items-center gap-2">
                        {for [
                            (None,"全部","question-bank-history-filter-all"),
                            (Some(PracticeMode::Paper),"套卷","question-bank-history-filter-paper"),
                            (Some(PracticeMode::Module),"模块","question-bank-history-filter-module"),
                            (Some(PracticeMode::Wrong),"错题","question-bank-history-filter-wrong"),
                            (Some(PracticeMode::Favorite),"收藏","question-bank-history-filter-favorite"),
                        ].into_iter().map(|(target,label,id)| {
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
                                .map(|score|(format!("{}/{}",score.correct,score.total),"badge-success"))
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
                                _=>format!(
                                    "{mode_label} · 用时 {time} · 正确率 {}%",
                                    score.correct.saturating_mul(100)/score.total.max(1)),
                            }
                        } else {
                            mode_label.to_string()
                        };
                        let can_repeat=matches!(run.mode,PracticeMode::Paper|PracticeMode::PaperModule|PracticeMode::Module);
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
                                                    {"再做一遍"}
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
                disabled={*busy || props.csrf.is_empty() ||
                    (collection.is_some() && browse.is_empty()) ||
                    (collection.is_none() && *mode == PracticeMode::Paper && selected_paper.is_empty()) ||
                    (collection.is_none() && *mode == PracticeMode::Module && selected_module.is_empty())}
                onclick={Callback::from(move |_| invoke.emit(Action::Start))}>
                {if *collection == Some(PracticeMode::Wrong) {"开始错题复练"}
                    else if *collection == Some(PracticeMode::Favorite) {"开始收藏题练习"}
                    else if *mode == PracticeMode::Paper {"开始套卷训练"} else {"开始模块训练"}}
            </button>
        }
    };
    let expanded_regions = web_sys::window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|width| width.as_f64())
        .filter(|width| *width >= 1024.0)
        .map(|_| 2)
        .unwrap_or(1);
    let collection_groups = collection_modules(&browse, catalog.as_ref());
    let collection_filtered = browse
        .iter()
        .filter(|item| {
            (selected_module.is_empty() || item.module.as_deref() == Some(selected_module.as_str()))
                && (selected_category.is_empty()
                    || item.category.as_deref() == Some(selected_category.as_str()))
        })
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
        _ => "模块训练",
    };
    let setup_explanation = match collection.as_ref() {
        Some(PracticeMode::Wrong) => {
            format!("错题按现有模块与二级题型分类，共 {} 题", *wrong_total)
        }
        Some(PracticeMode::Favorite) => {
            format!("收藏按现有模块与二级题型分类，共 {} 题", *favorite_total)
        }
        _ if *mode == PracticeMode::Paper => "选择完整原卷，按原始题号顺序作答".to_string(),
        _ => format!(
            "按一级模块与二级题型逐层筛选，每轮最多 {} 题",
            module_filters.count.unwrap_or(20)
        ),
    };
    let action_summary = match collection.as_ref() {
        Some(PracticeMode::Wrong) => "错题复练",
        Some(PracticeMode::Favorite) => "收藏题练习",
        _ if *mode == PracticeMode::Paper => "套卷训练",
        _ => "模块训练",
    };
    let action_detail = match collection.as_ref() {
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
            <div class="flex flex-wrap gap-2" role="group" aria-label="题目浏览分类">
                {for [
                    (None,"全部题目","question-bank-collection-all",None),
                    (Some(PracticeMode::Wrong),"我的错题","question-bank-collection-wrong",Some(*wrong_total)),
                    (Some(PracticeMode::Favorite),"我的收藏","question-bank-collection-favorite",Some(*favorite_total)),
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
            if collection.is_none() { {resume_card} }
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
                        <button type="button" class="btn btn-outline btn-sm" data-testid="module-drill-settings-open"
                            onclick={Callback::from({
                                let draft_filters=draft_filters.clone();
                                let module_filters=module_filters.clone();
                                let custom_years=custom_years.clone();
                                let settings_open=settings_open.clone();
                                move |_| {
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
                    <div class="question-bank-collections-layout">
                        <div class="space-y-3">
                            <div class="flex items-center justify-between gap-2">
                            <h3 class="font-semibold">{"按模块查看"}</h3>
                            <button type="button" class="btn btn-ghost btn-xs" data-testid="question-bank-collection-module"
                                aria-pressed={selected_module.is_empty().to_string()}
                                onclick={Callback::from({let selected_module=selected_module.clone();let selected_category=selected_category.clone();move |_|{selected_module.set(String::new());selected_category.set(String::new());}})}>
                                {format!("全部题目 {}",browse.len())}
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
                                    {format!("{}{}{}题",selection_scope,if selection_scope.is_empty() {""} else {" · "},collection_filtered.len())}
                                </span>
                            </div>
                            <div class="question-bank-results">
                            if collection_filtered.is_empty() {
                                <p class="p-4 text-sm text-base-content/60">{"这个分类还没有题目。"}</p>
                            } else {
                                <div class="question-bank-results-list" data-testid="question-bank-browse-list">
                                    {for collection_filtered.iter().take(100).map(|item|html! {
                                        <div class="question-bank-result-row" data-testid="question-bank-collection-item" title={item.paper_title.clone()}>
                                            <div class="min-w-0">
                                                <strong class="block truncate text-sm">{format!("{} {} · 第 {} 题",item.year,item.region,item.number)}</strong>
                                                <p class="mt-0.5 truncate text-xs text-base-content/50">{format!("{} / {}",item.module.as_deref().unwrap_or("未分类"),item.category.as_deref().unwrap_or("未分类"))}</p>
                                            </div>
                                            <span class="question-bank-result-year">{item.year}</span>
                                        </div>
                                    })}
                                </div>
                            }
                            </div>
                        </div>
                    </div>
                </section>
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
                                disabled={matches!((draft_filters.year_min,draft_filters.year_max),(Some(a),Some(b)) if a>b)}
                                onclick={Callback::from({
                                    let draft_filters=draft_filters.clone();
                                    let module_filters=module_filters.clone();
                                    let settings_open=settings_open.clone();
                                    let error=error.clone();
                                    move |_|{
                                        let next=(*draft_filters).clone();
                                        if persist_module_filters(&next) {
                                            module_filters.set(next);
                                            settings_open.set(false);
                                        } else {
                                            error.set(Some("设置保存失败，请检查浏览器存储权限。".into()));
                                        }
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
