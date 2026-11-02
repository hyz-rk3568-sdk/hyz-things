//! Real-paper practice: a single workspace for paper, paper-module, wrong and favorite.
use super::*;
use hyz_things::domain::question_bank::{
    CreatePracticeRequest, FavoriteState, PaperModules, PaperSummary, PracticeMode,
    PracticeReview, PracticeSession, PracticeWorkspace, SavePracticeAnswer, SetFavorite,
    SetPracticePaused,
};

const ROOT: &str = "/api/v1/question-bank";
const WORKSPACE: &str = "fixed inset-0 z-[70] flex h-dvh w-screen flex-col overflow-hidden bg-base-200 px-3 py-2 sm:px-6 sm:py-3";

#[derive(Properties, PartialEq)]
pub(crate) struct QuestionBankTrainingProps {
    pub csrf: String,
    pub on_immersive_change: Callback<bool>,
    pub on_paper_pause_change: Callback<bool>,
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
    Back,
}

async fn put_bank<T: serde::Serialize, R: serde::de::DeserializeOwned>(
    endpoint: &str, csrf: &str, body: &T, label: &str,
) -> Result<R, String> {
    let request = Request::put(endpoint)
        .credentials(RequestCredentials::SameOrigin)
        .header("Accept", "application/json")
        .header("X-HYZ-CSRF", csrf)
        .json(body)
        .map_err(|e| format!("{label}编码失败：{e}"))?;
    let response = request.send().await.map_err(|e| format!("{label}连接失败：{e}"))?;
    if !response.ok() {
        return Err(format!("{label}失败（HTTP {}）", response.status()));
    }
    response.json::<R>().await.map_err(|e| format!("{label}响应无效：{e}"))
}

async fn load_workspace(id: &str) -> Result<PracticeWorkspace, String> {
    fetch_json(&format!("{ROOT}/practice/sessions/{id}"), "真题练习").await
}

fn answer_options(markdown: &str) -> Vec<(String, String)> {
    markdown.lines().filter_map(|line| {
        let line = line.trim_start().strip_prefix("- ")?;
        let (label, rest) = line.split_once(['.', '．', '、', ' '])?;
        (label.len() == 1 && label.bytes().all(|b| b.is_ascii_uppercase()))
            .then(|| (label.to_string(), rest.trim().to_string()))
    }).collect()
}

#[function_component(QuestionBankTraining)]
pub(crate) fn question_bank_training(props: &QuestionBankTrainingProps) -> Html {
    let papers = use_state(Vec::<PaperSummary>::new);
    let history = use_state(Vec::<PracticeSession>::new);
    let selected_paper = use_state(String::new);
    let selected_module = use_state(String::new);
    let modules = use_state(|| None::<PaperModules>);
    let mode = use_state(|| PracticeMode::Paper);
    let workspace = use_state(|| None::<PracticeWorkspace>);
    let review = use_state(|| None::<PracticeReview>);
    let index = use_state(|| 0usize);
    let multi_select = use_state(|| false);
    let favorite = use_state(|| false);
    let busy = use_state(|| false);
    let error = use_state(|| None::<String>);
    let submit_confirm = use_state(|| false);
    let elapsed = use_state(|| 0u64);
    let tick = use_state(|| 0u64);
    let refresh = use_state(|| 0u64);

    {
        let papers = papers.clone();
        let history = history.clone();
        let error = error.clone();
        use_effect_with(*refresh, move |_| {
            spawn_local(async move {
                match fetch_json::<Vec<PaperSummary>>(&format!("{ROOT}/papers"), "真题试卷列表").await {
                    Ok(list) => papers.set(list),
                    Err(e) => error.set(Some(e)),
                }
                if let Ok(items) = fetch_json::<Vec<PracticeSession>>(
                    &format!("{ROOT}/practice/history"), "练习历史",
                ).await {
                    history.set(items);
                }
            });
            || ()
        });
    }
    {
        let id = (*selected_paper).clone();
        let modules = modules.clone();
        use_effect_with(id, move |id| {
            modules.set(None);
            if !id.is_empty() {
                let id = id.clone();
                spawn_local(async move {
                    if let Ok(result) = fetch_json::<PaperModules>(
                        &format!("{ROOT}/papers/{id}/modules"), "试卷模块",
                    ).await {
                        modules.set(Some(result));
                    }
                });
            }
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
        let tick = tick.clone();
        let running = workspace.as_ref().as_ref().is_some_and(|w| w.session.status == "active" && !w.session.paused);
        use_effect_with(running, move |running| {
            if *running {
                let interval = gloo_timers::callback::Interval::new(1000, move || {
                    elapsed.set(*elapsed + 1);
                    tick.set(*tick + 1);
                });
                Some(interval)
            } else {
                None
            }
        });
    }
    {
        let index_value = *index;
        let source = workspace.as_ref().as_ref().and_then(|w| w.questions.get(index_value))
            .map(|q| q.item.question_id.clone());
        let favorite = favorite.clone();
        use_effect_with(source, move |source| {
            favorite.set(false);
            if let Some(id) = source {
                let id = id.clone();
                spawn_local(async move {
                    if let Ok(value) = fetch_json::<FavoriteState>(
                        &format!("{ROOT}/practice/favorites/{id}"), "真题收藏",
                    ).await {
                        favorite.set(value.favorite);
                    }
                });
            }
            || ()
        });
    }
    {
        let current = workspace.as_ref().as_ref().and_then(|w| w.questions.get(*index))
            .map(|q| q.item.question_id.clone());
        use_effect_with((current, review.is_some()), move |_| {
            hydrate_math();
            || ()
        });
    }
    let invoke = {
        let csrf = props.csrf.clone();
        let papers = papers.clone();
        let mode = mode.clone();
        let selected_paper = selected_paper.clone();
        let selected_module = selected_module.clone();
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
            if *busy { return; }
            busy.set(true);
            error.set(None);
            let csrf = csrf.clone();
            let papers = papers.clone();
            let mode = mode.clone();
            let selected_paper = selected_paper.clone();
            let selected_module = selected_module.clone();
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
                            let request = CreatePracticeRequest {
                                mode: (*mode).clone(),
                                paper_id: match *mode {
                                    PracticeMode::Paper | PracticeMode::PaperModule => {
                                        if selected_paper.is_empty() || !papers.iter().any(|p| p.id == **selected_paper) {
                                            return Err("请先选择原始试卷".into());
                                        }
                                        Some((*selected_paper).clone())
                                    }
                                    _ => None,
                                },
                                module: if *mode == PracticeMode::PaperModule {
                                    if selected_module.is_empty() { return Err("请先选择原卷模块".into()); }
                                    Some((*selected_module).clone())
                                } else { None },
                            };
                            let session: PracticeSession = post_json_response(
                                &format!("{ROOT}/practice/sessions"), &csrf, &request, "创建真题练习",
                            ).await?;
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
                                &format!("{ROOT}/practice/sessions/{id}/review"), "真题解析",
                            ).await?;
                            let plain = load_workspace(&id).await?;
                            workspace.set(Some(plain));
                            review.set(Some(loaded));
                            index.set(0);
                        }
                        Action::Answer(choice) => {
                            let active = workspace.as_ref().as_ref().ok_or("训练不存在")?.clone();
                            let q = active.questions.get(*index).ok_or("题位不存在")?;
                            let mut answer = if *multi_select {
                                let mut selected = q.item.answer.clone();
                                if let Some(i) = selected.iter().position(|v| v == &choice) {
                                    selected.remove(i);
                                } else { selected.push(choice); }
                                selected
                            } else { vec![choice] };
                            answer.sort();
                            let _: PracticeSession = put_bank(
                                &format!("{ROOT}/practice/sessions/{}/answers/{}",active.session.id,q.item.question_id),
                                &csrf, &SavePracticeAnswer { answer }, "保存真题答案",
                            ).await?;
                            workspace.set(Some(load_workspace(&active.session.id).await?));
                        }
                        Action::Pause | Action::Exit => {
                            if let Some(active) = workspace.as_ref().as_ref() {
                                if active.session.status == "active" {
                                    let pause = match action {
                                        Action::Exit => true,
                                        _ => !active.session.paused,
                                    };
                                    let _: PracticeSession = put_bank(
                                        &format!("{ROOT}/practice/sessions/{}/pause",active.session.id),
                                        &csrf, &SetPracticePaused { paused: pause, elapsed_seconds: *elapsed },
                                        "暂停训练",
                                    ).await?;
                                    if matches!(action, Action::Exit) {
                                        workspace.set(None);
                                    } else {
                                        workspace.set(Some(load_workspace(&active.session.id).await?));
                                    }
                                } else {
                                    workspace.set(None);
                                    review.set(None);
                                }
                            }
                            if matches!(action, Action::Exit) { refresh.set(*refresh + 1); }
                        }
                        Action::Submit => {
                            let active = workspace.as_ref().as_ref().ok_or("训练不存在")?.clone();
                            if !*submit_confirm && active.session.items.iter().any(|q| q.answer.is_empty()) {
                                submit_confirm.set(true);
                            } else {
                                let completed: PracticeSession = post_json_response(
                                    &format!("{ROOT}/practice/sessions/{}/submit",active.session.id),
                                    &csrf, &EmptyRequest {}, "真题交卷",
                                ).await?;
                                elapsed.set(completed.elapsed_seconds.max(*elapsed));
                                workspace.set(Some(load_workspace(&completed.id).await?));
                                let detail: PracticeReview = fetch_json(
                                    &format!("{ROOT}/practice/sessions/{}/review",completed.id), "真题解析",
                                ).await?;
                                review.set(Some(detail));
                                submit_confirm.set(false);
                                refresh.set(*refresh + 1);
                            }
                        }
                        Action::Favorite(id) => {
                            let updated: FavoriteState = put_bank(
                                &format!("{ROOT}/practice/favorites/{id}"),
                                &csrf, &SetFavorite { favorite: !*favorite }, "收藏真题",
                            ).await?;
                            favorite.set(updated.favorite);
                        }
                        Action::Back => {
                            workspace.set(None);
                            review.set(None);
                            refresh.set(*refresh + 1);
                        }
                    }
                    Ok(())
                }.await;
                if let Err(e) = result { error.set(Some(e)); }
                busy.set(false);
            });
        })
    };
    let alert = if let Some(message) = error.as_ref() {
        html! { <p class="rounded-box bg-error/10 px-3 py-2 text-sm text-error" role="alert" data-testid="question-bank-error">{message.clone()}</p> }
    } else { Html::default() };

    if let Some(current) = workspace.as_ref() {
        let item_index = (*index).min(current.questions.len().saturating_sub(1));
        if let Some(item) = current.questions.get(item_index) {
            let completed = current.session.status == "completed";
            let selected_review = review.as_ref().and_then(|r| r.questions.get(item_index));
            let question = item.question.as_ref();
            let options = question.map(|q| answer_options(&q.options_markdown)).unwrap_or_default();
            let qid = item.item.question_id.clone();
            let starred = *favorite;
            let on_favorite = invoke.clone();
            let material_pane = html! {
                <section class="flex min-h-0 flex-col overflow-hidden rounded-box bg-base-100" data-testid="question-bank-practice-material-pane">
                    <div class={TRAINING_PANE_HEADER}><strong>{"原卷材料"}</strong></div>
                    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4" data-testid="question-bank-practice-material-scroll">
                        {for item.materials.iter().map(|m| html!{
                            <section class="mb-5"><h3 class="font-bold">{m.name.clone()}</h3>
                                <div class="study-markdown">{render_question_bank_markdown(&m.markdown)}</div>
                            </section>
                        })}
                    </div>
                </section>
            };
            let body = html! {
                <section class="flex min-h-0 flex-col overflow-hidden rounded-box bg-base-100" data-testid="question-bank-practice-question-pane">
                    <div class={TRAINING_PANE_HEADER}>
                        <strong>{format!("原卷第 {} 题（{}/{})", item.item.number,item_index+1,current.questions.len())}</strong>
                        <button type="button" class="btn btn-ghost btn-sm" data-testid="question-bank-favorite"
                            onclick={Callback::from(move |_| on_favorite.emit(Action::Favorite(qid.clone())))}>
                            {if starred {"★ 已收藏"} else {"☆ 收藏"}}
                        </button>
                    </div>
                    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4 space-y-4" data-testid="question-bank-practice-question-scroll">
                    if item.content_changed {
                        <p class="text-warning">{"题目源文件已更新或暂不可用，不能用新版题面替代旧题。"}</p>
                    } else if let Some(q) = question {
                        <div class="text-xs opacity-60">{format!("{} · {}",q.module.clone().unwrap_or("未归类".into()),q.category.clone().unwrap_or("未标注二级题型".into()))}</div>
                        if !q.issues.is_empty() {<p class="text-warning text-xs">{format!("来源待核：{}",q.issues.join("、"))}</p>}
                        <div class="study-markdown text-base leading-8" data-testid="question-bank-practice-stem">{render_question_bank_markdown(&q.stem)}</div>
                        <div class="space-y-2">
                            {for options.iter().map(|(label, markdown)| {
                                let chosen = item.item.answer.contains(label);
                                let correct = selected_review.is_some_and(|r|r.correct_answer.contains(label));
                                let style = if completed && correct {"btn-success"} else if completed && chosen {"btn-error"} else if chosen {"btn-primary"} else {"btn-outline"};
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
                            <p class="font-semibold">{format!("你的答案：{} · 正确答案：{} · 状态：{}",
                                if item.item.answer.is_empty() {"未答".into()} else {item.item.answer.join("、")},
                                if r.correct_answer.is_empty() {"不可判分".into()} else {r.correct_answer.join("、")},
                                item.item.grade.clone().unwrap_or_default())}</p>
                            <h3 class="font-bold">{"原始解析"}</h3>
                            <div class="study-markdown">{if r.explanation.is_empty() {html!{"暂无解析"}} else {render_question_bank_markdown(&r.explanation)}}</div>
                            <h3 class="font-bold">{"原始考点 topics"}</h3>
                            <p class="text-sm">{if q.topics.is_empty() {"暂无考点".into()} else {q.topics.join("、")}}</p>
                        }
                    }
                    </div>
                </section>
            };
            let shared = !item.materials.is_empty();
            let main = if shared {
                html! {<TrainingSplitWorkspace test_id="question-bank-practice-split" split_at_md={true}
                    left={material_pane} right={body}/>}
            } else {
                html! {<div class="h-full min-h-0" data-testid="question-bank-practice-single">{body}</div>}
            };
            let paused = current.session.paused && !completed;
            let nav = html! {
                <nav class="shrink-0 flex flex-wrap items-center justify-center gap-1 overflow-y-auto border-t border-base-content/10 py-2" data-testid="question-bank-practice-navigation">
                    <button class="btn btn-sm" disabled={item_index == 0} onclick={Callback::from({
                        let index=index.clone();move |_| index.set(item_index.saturating_sub(1))
                    })}>{"上一题"}</button>
                    {for current.questions.iter().enumerate().map(|(i,question)| {
                        let index=index.clone();
                        html! {<button class={classes!("btn","btn-xs",if i==item_index {"btn-primary"} else if !question.item.answer.is_empty() {"btn-success"} else {"btn-outline"})}
                            data-testid="question-bank-practice-number" onclick={Callback::from(move |_| index.set(i))}>
                            {question.item.number}
                        </button>}
                    })}
                    <button class="btn btn-sm" disabled={item_index+1 >= current.questions.len()} onclick={Callback::from({
                        let index=index.clone();move |_|index.set(item_index+1)
                    })}>{"下一题"}</button>
                </nav>
            };
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
                                    onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Pause)})}>
                                    {if paused {"继续"} else {"暂停"}}
                                </button>
                                <button class="btn btn-sm btn-error" data-testid="question-bank-practice-submit"
                                    onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Submit)})}>{"交卷"}</button>
                            } else if let Some(score) = &current.session.score {
                                <span class="font-semibold">{format!("正确 {} / 有效 {}",score.correct,score.correct+score.wrong+score.unanswered)}</span>
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
                                {main}
                            </InkOverlay>
                        }
                    </div>
                    {nav}
                </section>
            };
        }
    }

    html! {
        <TrainingSetupLayout test_id="question-bank-practice-setup"
            summary="真题训练" detail="原卷整卷 / 原卷完整模块，不跨卷随机抽题">
            <h2 class="text-xl font-bold">{"真题训练"}</h2>
            {alert}
            <div class="flex flex-wrap gap-2" role="group" aria-label="真题训练模式">
                {for [
                    (PracticeMode::Paper,"整卷训练"),
                    (PracticeMode::PaperModule,"原卷模块练习"),
                    (PracticeMode::Wrong,"错题训练"),
                    (PracticeMode::Favorite,"收藏训练"),
                ].into_iter().map(|(value,label)| {
                    let mode=mode.clone();
                    let selected_module=selected_module.clone();
                    html! {<button class={classes!("btn","btn-sm",if *mode==value {"btn-primary"}else{"btn-outline"})}
                        data-testid="question-bank-mode" aria-pressed={(*mode==value).to_string()}
                        onclick={Callback::from(move |_|{
                            mode.set(value.clone());selected_module.set(String::new());
                        })}>{label}</button>}
                })}
            </div>
            if matches!(*mode,PracticeMode::Paper | PracticeMode::PaperModule) {
                <label class="block">{"选择原卷"}
                    <select class="select select-bordered mt-2 w-full" data-testid="question-bank-select-paper"
                        value={(*selected_paper).clone()}
                        onchange={Callback::from({let selected_paper=selected_paper.clone();let selected_module=selected_module.clone();move |e: Event| {
                            let select: HtmlSelectElement=e.target_unchecked_into();
                            selected_paper.set(select.value());
                            selected_module.set(String::new());
                        }})}>
                        <option value="">{"请选择年份 / 地区 / 试卷"}</option>
                        {for papers.iter().map(|p|html!{
                            <option value={p.id.clone()}>{format!("{} · {} · {} · {}（{}题）",p.year,p.region,p.variant,p.title,p.questions)}</option>
                        })}
                    </select>
                </label>
                if *mode == PracticeMode::PaperModule {
                    if let Some(info) = &*modules {
                        <p class="text-sm text-warning">{format!("未归类题位：{}",info.unclassified)}</p>
                        <div class="grid gap-2" role="group" aria-label="原卷模块">
                            {for info.modules.iter().map(|m| {
                                let selected_module=selected_module.clone();
                                let name=m.name.clone();
                                let selected=*selected_module==m.name;
                                html! {<button type="button" data-testid="question-bank-select-module"
                                    class={classes!("btn","justify-start",if selected {"btn-primary"} else {"btn-outline"})}
                                    onclick={Callback::from(move |_|selected_module.set(name.clone()))}>
                                    {format!("{} · {}题（待核/不可判分 {}题）",m.name,m.count,m.ungradable)}
                                </button>}
                            })}
                        </div>
                    }
                }
            }
            <button class={TRAINING_PRIMARY_BUTTON} data-testid="question-bank-practice-start"
                disabled={*busy || props.csrf.is_empty()}
                onclick={Callback::from({let invoke=invoke.clone();move |_|invoke.emit(Action::Start)})}>
                {"开始真题训练"}
            </button>
            <h3 class="text-lg font-bold">{"未完成与练习历史"}</h3>
            <div class="grid gap-2" data-testid="question-bank-practice-history">
                {for history.iter().map(|h| {
                    let active=h.status=="active";
                    let action=invoke.clone();let id=h.id.clone();
                    html! {<button type="button" class="btn btn-outline h-auto min-h-12 justify-between py-2"
                        data-testid="question-bank-practice-history-item"
                        onclick={Callback::from(move |_| action.emit(if active {Action::Continue(id.clone())} else {Action::Review(id.clone())}))}>
                        <span>{format!("{:?} · {} 题 · {}",h.mode,h.items.len(),if active {"继续练习"} else {"查看解析"})}</span>
                        <span>{h.module.clone().unwrap_or_default()}</span>
                    </button>}
                })}
            </div>
        </TrainingSetupLayout>
    }
}
