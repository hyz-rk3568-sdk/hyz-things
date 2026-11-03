//! Browse original papers and categories; share one workspace for practice and review.
use super::material_scroll::{scroll_edges, MaterialScrollButton, ScrollEdges};
use super::*;
use hyz_things::domain::question_bank::{
    CreatePracticeRequest, FavoriteState, MaterialView, ModulePracticeFilters,
    PaperSummary, PaperView, PracticeBrowseItem, PracticeCatalog, PracticeMode, PracticeReview,
    PracticeSession, PracticeWorkspace, QuestionView,
    SavePracticeAnswer, SetFavorite, SetPracticePaused,
};

const ROOT: &str = "/api/v1/question-bank";
const BROWSE_PAGE_SIZE: usize = 20;
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
    scope: &PracticeMode, entry_mode: &PracticeMode, paper_id: &str,
    module: &str, category: &str, filters: &ModulePracticeFilters,
) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("mode", match scope {
        PracticeMode::Paper => "paper",
        PracticeMode::Module => "module",
        PracticeMode::Wrong => "wrong",
        PracticeMode::Favorite => "favorite",
        PracticeMode::PaperModule => "paper_module",
    });
    if *entry_mode == PracticeMode::Paper {
        if !paper_id.is_empty() { query.append_pair("paper_id", paper_id); }
    } else {
        if !module.is_empty() { query.append_pair("module", module); }
        if !category.is_empty() { query.append_pair("category", category); }
        if let Some(year) = filters.year_min { query.append_pair("year_min", &year.to_string()); }
        if let Some(year) = filters.year_max { query.append_pair("year_max", &year.to_string()); }
        if !filters.regions.is_empty() {
            query.append_pair("regions", &filters.regions.join(","));
            if filters.exclude_regions { query.append_pair("exclude_regions", "true"); }
        }
    }
    format!("{ROOT}/practice/questions?{}", query.finish())
}

const WORKSPACE: &str = "training-workspace fixed inset-0 z-[70] flex h-dvh w-screen flex-col overflow-hidden bg-base-200 px-3 py-2 sm:px-6 sm:py-3";

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
    Review(String),
    Answer(String),
    Pause,
    Exit,
    Submit,
    Favorite(String),
    Preview(String, String),
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

#[function_component(QuestionBankTraining)]
pub(crate) fn question_bank_training(props: &QuestionBankTrainingProps) -> Html {
    let papers = use_state(Vec::<PaperSummary>::new);
    let history = use_state(Vec::<PracticeSession>::new);
    let history_open = use_state(|| props.show_history);
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
    let browse_page = use_state(|| 0usize);
    let preview = use_state(|| None::<(QuestionView, Vec<MaterialView>)>);

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
                if *material_edges != value { material_edges.set(value); }
            }
            if let Some(node) = question_scroll.cast::<web_sys::HtmlElement>() {
                let value = scroll_edges(&node);
                if *question_edges != value { question_edges.set(value); }
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
        let scope = collection.as_ref().cloned().unwrap_or_else(|| (*mode).clone());
        let query = if (*mode == PracticeMode::Paper && selected_paper.is_empty())
            || (*mode == PracticeMode::Module && selected_module.is_empty()) {
            None
        } else {
            Some(browse_filter_url(&scope, &mode, &selected_paper,
                &selected_module, &selected_category, &module_filters))
        };
        let browse = browse.clone();
        let browse_page = browse_page.clone();
        let preview = preview.clone();
        let error = error.clone();
        use_effect_with((query, *refresh), move |(query, _)| {
            browse.set(Vec::new());
            browse_page.set(0);
            preview.set(None);
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
        let selected = (*index, review.is_some(),
            workspace.as_ref().as_ref().map(|w| w.session.id.clone()));
        use_effect_with(selected, move |_| {
            refresh_edges.emit(());
            || ()
        });
    }
    let invoke = {
        let csrf = props.csrf.clone();
        let papers = papers.clone();
        let mode = mode.clone();
        let collection = collection.clone();
        let preview = preview.clone();
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
            let mode = mode.clone();
            let collection = collection.clone();
            let preview = preview.clone();
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
                            let practice_mode = collection.as_ref().cloned()
                                .unwrap_or_else(|| (*mode).clone());
                            let collection_active = collection.is_some();
                            let request = CreatePracticeRequest {
                                mode: practice_mode.clone(),
                                paper_id: match practice_mode {
                                    PracticeMode::Paper | PracticeMode::PaperModule => {
                                        if selected_paper.is_empty()
                                            || !papers
                                                .iter()
                                                .any(|p| p.id.as_str() == selected_paper.as_str())
                                        {
                                            return Err("请先选择原始试卷".into());
                                        }
                                        Some((*selected_paper).clone())
                                    }
                                    PracticeMode::Wrong | PracticeMode::Favorite
                                        if *mode == PracticeMode::Paper && !selected_paper.is_empty() =>
                                            Some((*selected_paper).clone()),
                                    _ => None,
                                },
                                module: if *mode == PracticeMode::Module {
                                    if selected_module.is_empty() { return Err("请先选择模块".into()); }
                                    Some((*selected_module).clone())
                                } else { None },
                                category: if *mode == PracticeMode::Module && !selected_category.is_empty() {
                                    Some((*selected_category).clone())
                                } else { None },
                                filters: if *mode == PracticeMode::Module {
                                    (*module_filters).clone()
                                } else if collection_active {
                                    ModulePracticeFilters { count: Some(20), ..Default::default() }
                                } else { ModulePracticeFilters::default() },
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
                        Action::Preview(paper_id, id) => {
                            let paper: PaperView = fetch_json(
                                &format!("{ROOT}/papers/{paper_id}"), "真题预览"
                            ).await?;
                            let question = paper.questions.into_iter().find(|q| q.id == id)
                                .ok_or("题目不存在")?;
                            let materials = paper.materials.into_iter()
                                .filter(|m| question.materials.contains(&m.id)).collect();
                            preview.set(Some((question, materials)));
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
            let material_pane = html! {
                <section class="flex min-h-0 flex-col overflow-hidden rounded-box bg-base-100" data-testid="question-bank-practice-material-pane">
                    <div class={TRAINING_PANE_HEADER}>
                        <strong>{"原卷材料"}</strong>
                        <div class="ml-auto flex shrink-0 gap-1" role="group" aria-label="材料滚动">
                            <MaterialScrollButton passage={material_scroll.clone()} up={true}
                                disabled={!material_edges.can_scroll_up} question_index={item_index} />
                            <MaterialScrollButton passage={material_scroll.clone()} up={false}
                                disabled={!material_edges.can_scroll_down} question_index={item_index} />
                        </div>
                    </div>
                    <div ref={material_scroll.clone()} onscroll={on_scrolling.clone()}
                        class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4"
                        data-testid="question-bank-practice-material-scroll">
                        {for item.materials.iter().map(|m| html!{
                            <section class="mb-5"><h3 class="font-bold">{m.name.clone()}</h3>
                                <div class="study-markdown">{render_question_bank_markdown(&m.markdown)}</div>
                            </section>
                        })}
                    </div>
                </section>
            };
            let shared = !item.materials.is_empty();
            let body = html! {
                <section class="flex h-full min-h-0 flex-col overflow-hidden rounded-box bg-base-100" data-testid="question-bank-practice-question-pane">
                    <div class={TRAINING_PANE_HEADER}>
                        <button type="button" class="btn btn-ghost btn-sm shrink-0" data-testid="question-bank-favorite"
                            onclick={Callback::from(move |_| on_favorite.emit(Action::Favorite(qid.clone())))}
                            aria-label={if starred {"取消收藏"} else {"收藏题目"}}>
                            {if starred {"★"} else {"☆"}}
                        </button>
                        <strong class="min-w-0 truncate text-sm">{format!("原卷第 {} 题（{}/{})", item.item.number,item_index+1,current.questions.len())}</strong>
                        <div class="ml-auto flex shrink-0 items-center gap-1">
                            <div class="flex gap-1" role="group" aria-label="题目与解析滚动">
                                <MaterialScrollButton passage={question_scroll.clone()} up={true}
                                    disabled={!question_edges.can_scroll_up} question_index={item_index} target="题目" />
                                <MaterialScrollButton passage={question_scroll.clone()} up={false}
                                    disabled={!question_edges.can_scroll_down} question_index={item_index} target="题目" />
                            </div>
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
                            <div class="study-markdown text-base leading-8" data-testid="question-bank-practice-stem">{render_question_bank_markdown(&q.stem)}</div>
                        }
                        if !q.issues.is_empty() && !completed {
                            <p class="text-xs text-base-content/60">{"本题内容尚待核验，可能不计入成绩。"}</p>
                        }
                        <div class="space-y-2">
                            {for options.iter().map(|(label, markdown)| {
                                let chosen = item.item.answer.contains(label);
                                let correct = selected_review.is_some_and(|r|r.correct_answer.contains(label));
                                let grade = item.item.grade.as_deref();
                                let style = if completed && correct && grade != Some("ungradable") {
                                    "btn-success"
                                } else if completed && chosen && grade == Some("wrong") {
                                    "btn-error"
                                } else if completed && chosen && grade == Some("correct") {
                                    "btn-success"
                                } else if !completed && chosen {
                                    "btn-primary"
                                } else {"btn-outline"};
                                let action = invoke.clone(); let choice = label.clone();
                                html! {<button type="button" class={classes!("btn","h-auto","min-h-12","w-full","justify-start","whitespace-normal","text-left",style)}
                                    aria-pressed={chosen.to_string()} data-testid="question-bank-practice-choice"
                                    disabled={completed || current.session.paused || *busy}
                                    onclick={Callback::from(move |_|action.emit(Action::Answer(choice.clone())))}>
                                    <span class="mr-2">{format!("{label}.")}</span>
                                    <span class="study-markdown">{render_question_bank_markdown(markdown)}</span>
                                </button>}
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
                            <p class="font-semibold" data-testid="question-bank-practice-review-answer">
                                {format!("你的答案：{} · 正确答案：{}",
                                    if item.item.answer.is_empty() {"未答".into()} else {item.item.answer.join("、")},
                                    if r.correct_answer.is_empty() {"暂无可靠答案".into()} else {r.correct_answer.join("、")})}
                            </p>
                            <h3 class="font-bold">{"原始解析"}</h3>
                            <div class="study-markdown">{if r.explanation.is_empty() {html!{"暂无解析"}} else {render_question_bank_markdown(&r.explanation)}}</div>
                            <h3 class="font-bold">{"考点"}</h3>
                            <p class="text-sm">{if q.topics.is_empty() {"暂无考点".into()} else {q.topics.join("、")}}</p>
                        }
                    }
                    </div>
                </section>
            };
            let left_pane = if shared {
                material_pane
            } else {
                html! {
                    <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box bg-base-100"
                        data-testid="question-bank-practice-stem-pane">
                        <div class={TRAINING_PANE_HEADER}>
                            <strong>{"题干"}</strong>
                            <div class="ml-auto flex shrink-0 gap-1" role="group" aria-label="题干滚动">
                                <MaterialScrollButton passage={material_scroll.clone()} up={true}
                                    disabled={!material_edges.can_scroll_up} question_index={item_index} target="题干" />
                                <MaterialScrollButton passage={material_scroll.clone()} up={false}
                                    disabled={!material_edges.can_scroll_down} question_index={item_index} target="题干" />
                            </div>
                        </div>
                        <div ref={material_scroll.clone()} onscroll={on_scrolling.clone()}
                            class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4"
                            data-testid="question-bank-practice-material-scroll">
                            if let Some(q) = question {
                                <div class="study-markdown text-base leading-8" data-testid="question-bank-practice-stem">
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
            let paused = current.session.paused && !completed;
            return html! {
                <section class={WORKSPACE} data-testid="question-bank-practice-workspace">
                    <header class="flex shrink-0 flex-wrap items-center justify-between gap-2 pb-2">
                        <div class="flex min-w-0 gap-2">
                            <button class="btn btn-ghost btn-sm" onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Exit)})}>{"← 暂停并退出"}</button>
                            <strong class="truncate text-sm">{item.paper_title.clone()}</strong>
                        </div>
                        <div class="flex items-center gap-2">
                            <StudyTimeInline />
                            <span class="font-mono text-sm" role="timer" data-testid="question-bank-practice-timer">{format!("{:02}:{:02}",(*elapsed/60),(*elapsed%60))}</span>
                            if !completed {
                                <button class="btn btn-sm btn-outline" data-testid="question-bank-practice-pause"
                                    disabled={*busy} onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Pause)})}>
                                    {if paused {"继续"} else {"暂停"}}
                                </button>
                                <button class="btn btn-sm btn-error" data-testid="question-bank-practice-submit"
                                    disabled={*busy} onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Submit)})}>{"交卷"}</button>
                            } else if let Some(score) = &current.session.score {
                                <span class="flex shrink-0 items-center gap-3 font-semibold" data-testid="question-bank-practice-score">
                                    <span class="text-success">{format!("正确 {}",score.correct)}</span>
                                    <span class="text-error">{format!("错误 {}",score.wrong)}</span>
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
        return html! {
            <TrainingSetupLayout test_id="question-bank-practice-history" heading={Some(html!{
                <div class="flex items-center justify-between">
                    <h2 class="text-xl font-bold">{"真题练习记录"}</h2>
                    <button class="btn btn-ghost btn-sm" onclick={Callback::from({
                        let history_open=history_open.clone();move |_|history_open.set(false)
                    })}>{if *mode == PracticeMode::Paper {"返回套卷"} else {"返回模块"}}</button>
                </div>
            })}>
                <div class="grid gap-2" data-testid="question-bank-practice-history">
                    {for history.iter().filter(|run| {
                        let paper_run = matches!(run.mode, PracticeMode::Paper | PracticeMode::PaperModule)
                            || matches!(run.mode, PracticeMode::Wrong | PracticeMode::Favorite)
                                && run.paper_id.is_some();
                        if *mode == PracticeMode::Paper { paper_run } else { !paper_run }
                    }).map(|run|{
                        let active=run.status=="active";
                        let id=run.id.clone();
                        let invoke=invoke.clone();
                        html! {
                            <button type="button" class="btn btn-outline h-auto min-h-16 justify-between gap-3 p-3"
                                data-testid="question-bank-practice-history-item"
                                onclick={Callback::from(move |_|invoke.emit(if active {Action::Continue(id.clone())} else {Action::Review(id.clone())}))}>
                                <span>{format!("{}题 · {} · {}",
                                    run.items.len(),run.module.clone().unwrap_or_else(||"整卷".into()),
                                    if active {"继续练习"} else {"查看解析"})}</span>
                                <span>{format!("{}分钟",run.elapsed_seconds/60)}</span>
                            </button>
                        }
                    })}
                </div>
            </TrainingSetupLayout>
        };
    }

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
    html! {
        <TrainingSetupLayout test_id="question-bank-practice-setup" action={Some(start_action)}
            summary={if *mode == PracticeMode::Paper {"套卷训练"} else {"模块训练"}}
            detail={if *mode == PracticeMode::Paper {"选择完整原卷，按原始题号顺序作答"} else {"按一级模块和二级题型分类浏览、自定义刷题"}}>
            <div class="flex items-center justify-between gap-3">
                <h2 class="text-xl font-bold">
                    {if *mode == PracticeMode::Paper {"套卷训练"} else {"模块训练"}}
                </h2>
                <button type="button" class="btn btn-outline btn-sm" data-testid="question-bank-history-open"
                    onclick={Callback::from({let history_open=history_open.clone();move |_|history_open.set(true)})}>
                    {"练习记录"}
                </button>
            </div>
            {alert}
            <div class="flex flex-wrap gap-2" role="group" aria-label="题目浏览分类">
                {for [
                    (None,"全部题目","question-bank-collection-all"),
                    (Some(PracticeMode::Wrong),"我的错题","question-bank-collection-wrong"),
                    (Some(PracticeMode::Favorite),"我的收藏","question-bank-collection-favorite"),
                ].into_iter().map(|(target,label,id)| {
                    let collection=collection.clone();
                    let preview=preview.clone();
                    let selected=*collection==target;
                    html! {<button type="button"
                        class={classes!("btn","btn-sm",if selected {"btn-primary"} else {"btn-outline"})}
                        data-testid={id} aria-pressed={selected.to_string()}
                        onclick={Callback::from(move |_|{
                            collection.set(target.clone());
                            preview.set(None);
                        })}>{label}</button>}
                })}
            </div>
            if *mode == PracticeMode::Paper {
                <section class="space-y-3" aria-label="选择原卷">
                    <p class="text-sm text-base-content/65">{"按地区和年份展开原卷，选中后即可开始。"}</p>
                    if papers.is_empty() {
                        <p class="rounded-box bg-base-200 p-4 text-sm">{"暂无可用试卷，请先在题库管理同步原卷。"}</p>
                    }
                    {for {
                        let mut grouped = std::collections::BTreeMap::<String,std::collections::BTreeMap<i32,Vec<&PaperSummary>>>::new();
                        for paper in papers.iter().filter(|paper|paper.role=="primary") {
                            grouped.entry(paper.region.clone()).or_default()
                                .entry(paper.year).or_default().push(paper);
                        }
                        grouped.into_iter().map(|(region,years)| html! {
                            <details class="rounded-box border border-base-content/10 bg-base-100" data-testid="question-bank-region-group">
                                <summary class="cursor-pointer select-none px-4 py-3 font-semibold" data-testid="question-bank-region">{region.clone()}</summary>
                                <div class="space-y-2 border-t border-base-content/10 p-3">
                                    {for years.into_iter().rev().map(|(year,items)|html! {
                                        <details class="rounded-box bg-base-200/50" data-testid="question-bank-year-group">
                                            <summary class="cursor-pointer select-none px-3 py-2 text-sm font-medium" data-testid="question-bank-year">{format!("{year}年 · {}份",items.len())}</summary>
                                            <div class="grid gap-2 px-2 pb-2">
                                                {for items.into_iter().map(|paper|{
                                                    let id=paper.id.clone();
                                                    let active=*selected_paper==id;
                                                    let selected_paper=selected_paper.clone();
                                                    let selected_module=selected_module.clone();
                                                    html! {
                                                        <button type="button" class={classes!("btn","h-auto","min-h-14","w-full","justify-between","whitespace-normal","text-left",if active {"btn-primary"} else {"btn-outline"})}
                                                            data-testid="question-bank-paper-option" aria-pressed={active.to_string()}
                                                            onclick={Callback::from(move |_|{selected_paper.set(id.clone());selected_module.set(String::new());})}>
                                                            <span>{paper.title.clone()}</span>
                                                            <span class="shrink-0 text-xs opacity-60">{format!("{}题",paper.questions)}</span>
                                                        </button>
                                                    }
                                                })}
                                            </div>
                                        </details>
                                    })}
                                </div>
                            </details>
                        })
                    }}
                </section>
                if let Some(selected) = papers.iter().find(|paper|paper.id==*selected_paper) {
                    <p class="rounded-box bg-primary/10 px-3 py-2 text-sm" data-testid="question-bank-selected-paper">
                        {format!("已选：{} · {}题", selected.title, selected.questions)}
                    </p>
                }
            }
            if *mode == PracticeMode::Module {
                <section class="space-y-3" data-testid="question-bank-cross-module-setup">
                    <div class="flex items-center justify-between gap-3">
                        <div>
                            <h3 class="font-semibold">{"专项模块"}</h3>
                            <p class="text-xs text-base-content/65">{"选择模块，按需展开二级题型。"}</p>
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
                    <div class="rounded-box bg-base-200/70 px-3 py-2 text-sm" data-testid="module-drill-filter-summary">
                        {format!("{} · {}地区 · 每轮{}题",
                            match (module_filters.year_min,module_filters.year_max) {
                                (None,None) => "全部年份".into(),
                                (Some(min),Some(max)) => format!("{min}—{max}年"),
                                (Some(min),None) => format!("{min}年以后"),
                                (None,Some(max)) => format!("{max}年以前"),
                            },
                            if module_filters.regions.is_empty() {"不限".into()}
                            else if module_filters.exclude_regions {format!("屏蔽 {}",module_filters.regions.join("、"))}
                            else {module_filters.regions.join("、")},
                            module_filters.count.unwrap_or(20))}
                    </div>
                    if let Some(info) = &*catalog {
                        <div class="grid gap-2 sm:grid-cols-2" role="group" aria-label="一级模块">
                            {for info.modules.iter().map(|m| {
                                let selected_module=selected_module.clone();
                                let selected_category=selected_category.clone();
                                let name=m.name.clone();
                                let selected=*selected_module==name;
                                html! {<button type="button" data-testid="question-bank-cross-module"
                                    aria-pressed={selected.to_string()}
                                    class={classes!("btn","min-h-20","h-auto","flex-col","items-stretch","justify-center","gap-1",if selected {"btn-primary"} else {"btn-outline"})}
                                    onclick={Callback::from(move |_|{selected_module.set(name.clone());selected_category.set(String::new());})}>
                                    <span class="flex items-center justify-between gap-2"><strong>{m.name.clone()}</strong><span class="text-xs">{format!("已做 {}/{} 题",m.done,m.count)}</span></span>
                                    <progress class="progress progress-primary w-full" max={m.count.max(1).to_string()} value={m.done.to_string()} aria-label={format!("{}进度",m.name)} />
                                </button>}
                            })}
                        </div>
                        if let Some(module) = info.modules.iter().find(|m|m.name==*selected_module) {
                            <details class="rounded-box border border-base-content/10 p-3">
                                <summary class="cursor-pointer select-none text-sm">{"筛选二级题型（可选）"}</summary>
                                <div class="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2" role="group" aria-label="二级题型">
                                    <button type="button" data-testid="question-bank-cross-category"
                                        class={classes!("btn","justify-between","min-h-12",if selected_category.is_empty() {"btn-primary"} else {"btn-outline"})}
                                        onclick={Callback::from({let selected_category=selected_category.clone(); move |_|selected_category.set(String::new())})}>
                                        <span>{"全部题型"}</span><span class="text-xs">{format!("{}/{}",module.done,module.count)}</span>
                                    </button>
                                    {for module.categories.iter().map(|category|{
                                        let name=category.name.clone();
                                        let selected=*selected_category==name;
                                        let selected_category=selected_category.clone();
                                        html! {<button type="button" data-testid="question-bank-cross-category"
                                            aria-pressed={selected.to_string()}
                                            class={classes!("btn","h-auto","min-h-12","justify-between",if selected {"btn-primary"} else {"btn-outline"})}
                                            onclick={Callback::from(move |_| selected_category.set(name.clone()))}>
                                            <span>{category.name.clone()}</span><span class="text-xs">{format!("已做 {}/{} 题",category.done,category.count)}</span>
                                        </button>}
                                    })}
                                </div>
                            </details>
                        }
                    } else {
                        <p class="text-sm">{"暂无可用模块，请先在题库管理同步原卷。"}</p>
                    }
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
            <section class="space-y-3 rounded-box border border-base-content/10 bg-base-100 p-3"
                data-testid="question-bank-browse-section">
                <div class="flex items-center justify-between gap-2">
                    <h3 class="font-semibold">
                        {if *collection == Some(PracticeMode::Wrong) {"我的错题"}
                            else if *collection == Some(PracticeMode::Favorite) {"我的收藏"}
                            else {"分类浏览题目"}}
                    </h3>
                    <span class="text-xs text-base-content/60" data-testid="question-bank-browse-count">
                        {format!("{}题",browse.len())}
                    </span>
                </div>
                if (*mode==PracticeMode::Paper && selected_paper.is_empty())
                    || (*mode==PracticeMode::Module && selected_module.is_empty()) {
                    <p class="text-sm text-base-content/60">
                        {if *mode==PracticeMode::Paper {"先选择地区、年份和原卷，再浏览这套卷的题目。"}
                            else {"先选择模块，即可浏览对应题目和二级题型。"}}
                    </p>
                } else if browse.is_empty() {
                    <p class="text-sm text-base-content/60">{"当前分类没有题目。"}</p>
                } else {
                    <div class="space-y-2" data-testid="question-bank-browse-list">
                        {for browse.iter().skip(*browse_page*BROWSE_PAGE_SIZE).take(BROWSE_PAGE_SIZE).map(|item|{
                            let action=invoke.clone();
                            let id=item.question_id.clone();
                            let paper_id=item.paper_id.clone();
                            html! {
                                <button type="button" class="flex min-h-14 w-full items-center justify-between gap-3 rounded-box border border-base-content/15 px-3 py-2 text-left"
                                    data-testid="question-bank-browse-item"
                                    onclick={Callback::from(move |_| action.emit(Action::Preview(paper_id.clone(),id.clone())))}>
                                    <span class="min-w-0">
                                        <strong class="block truncate">{format!("{}年 · {} · 第{}题",item.year,item.region,item.number)}</strong>
                                        <span class="block truncate text-xs text-base-content/65">
                                            {format!("{} · {}",item.paper_title,item.category.as_deref().or(item.module.as_deref()).unwrap_or("未分类"))}
                                        </span>
                                    </span>
                                    <span class="shrink-0 text-sm text-primary">{"查看题目"}</span>
                                </button>
                            }
                        })}
                    </div>
                    <div class="flex items-center justify-center gap-3" data-testid="question-bank-browse-pages">
                        <button type="button" class="btn btn-outline btn-sm" disabled={*browse_page==0}
                            onclick={Callback::from({let browse_page=browse_page.clone();move |_|browse_page.set((*browse_page).saturating_sub(1))})}>{"上一页"}</button>
                        <span class="text-xs">{format!("{}/{}",*browse_page+1,browse.len().div_ceil(BROWSE_PAGE_SIZE))}</span>
                        <button type="button" class="btn btn-outline btn-sm"
                            disabled={(*browse_page+1)*BROWSE_PAGE_SIZE>=browse.len()}
                            onclick={Callback::from({let browse_page=browse_page.clone();move |_|browse_page.set(*browse_page+1)})}>{"下一页"}</button>
                    </div>
                }
                if let Some((question,materials))=preview.as_ref() {
                    <article class="space-y-3 rounded-box bg-base-200/50 p-3" data-testid="question-bank-browse-preview">
                        <div class="flex items-center justify-between gap-2">
                            <h4 class="font-semibold">{format!("第{}题 · 题目预览",question.number)}</h4>
                            <button class="btn btn-ghost btn-sm" type="button"
                                onclick={Callback::from({let preview=preview.clone();move |_|preview.set(None)})}>{"收起"}</button>
                        </div>
                        {for materials.iter().map(|material|html! {
                            <div class="study-markdown text-sm leading-7">{render_question_bank_markdown(&material.markdown)}</div>
                        })}
                        <div class="study-markdown text-sm leading-7">{render_question_bank_markdown(&question.stem)}</div>
                        {for answer_options(&question.options_markdown).iter().map(|(label,body)|html!{
                            <div class="study-markdown rounded-box border border-base-content/10 p-2 text-sm">
                                {format!("{label}. ")}{render_question_bank_markdown(body)}
                            </div>
                        })}
                    </article>
                }
            </section>
            if let Some(active) = history.iter().find(|h|h.status=="active") {
                <div class="rounded-box border border-primary/20 bg-base-100 p-3" data-testid="question-bank-resume-card">
                    <p class="text-sm font-semibold">{"继续上次练习"}</p>
                    <div class="mt-2 flex items-center justify-between gap-3">
                        <span class="text-xs text-base-content/60">{format!("{}题 · 已用{}分钟",active.items.len(),active.elapsed_seconds/60)}</span>
                        <button type="button" class="btn btn-outline btn-sm"
                            onclick={Callback::from({let invoke=invoke.clone();let id=active.id.clone();move |_|invoke.emit(Action::Continue(id.clone()))})}>{"继续练习"}</button>
                    </div>
                </div>
            }
        </TrainingSetupLayout>
    }
}
