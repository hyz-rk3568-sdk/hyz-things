//! 专项训练：保留既有速算，读取已同步的 Flashcard 进行手写错题复练。
//! 复练自评只驻留本轮浏览器状态，绝不写入 FSRS review API。
use super::*;
use hyz_things::domain::flashcards::{DeckSummary, FlashcardSummary, TagSummary};
use std::collections::HashSet;

#[wasm_bindgen::prelude::wasm_bindgen(module = "/src/web/training_viewport.js")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = mountTrainingViewport)]
    fn mount_training_viewport(root: &web_sys::Element) -> js_sys::Function;
}

const PRACTICE_PAGE_SIZE: u32 = 200;
const PRACTICE_MAX_CANDIDATES: usize = 1000;

// All active specialized drills share the same fixed viewport contract.
// The inner reading pane scrolls, not the training card or browser document.
pub(super) const FIXED_TRAINING_LAYOUT: &str =
    "fixed inset-0 z-[70] flex h-dvh w-screen flex-col overflow-hidden bg-base-200 px-3 py-2 sm:px-6 sm:py-3";

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrainingMode {
    Calculation,
    Flashcards,
    Comprehensive,
    LogicJudgment,
    QuantitativeRelations,
    Statistics,
}

#[derive(Properties, PartialEq)]
struct TrainingWorkspaceProps {
    route_state: UseStateHandle<PortalRoute>,
    csrf: String,
}

pub(crate) fn render_training(route_state: UseStateHandle<PortalRoute>, csrf: String) -> Html {
    html! { <StudyClockProvider csrf={csrf.clone()}><TrainingWorkspace route_state={route_state} csrf={csrf} /></StudyClockProvider> }
}

#[function_component(TrainingWorkspace)]
fn training_workspace(props: &TrainingWorkspaceProps) -> Html {
    let shell = use_node_ref();
    {
        let shell = shell.clone();
        use_effect_with((), move |_| {
            let cleanup = shell
                .cast::<web_sys::Element>()
                .map(|root| mount_training_viewport(&root));
            move || {
                if let Some(cleanup) = cleanup {
                    let _ = cleanup.call0(&JsValue::NULL);
                }
            }
        });
    }
    // iPad WebKit may suspend timers and restore a page without a usable
    // visibilitychange event. Treat every foreground signal as a wake-up,
    // coalescing the events Safari fires together into one study-time resync.
    // Do not stop or credit background study time here: the server's existing
    // 45-second heartbeat grace remains authoritative.
    let resume_generation = use_state(|| 0_u64);
    {
        let last_resume_ms = use_mut_ref(|| 0_f64);
        let resume_counter = use_mut_ref(|| 0_u64);
        let resume_generation = resume_generation.clone();
        use_effect_with((), move |_| {
            let window = web_sys::window();
            let document = window.as_ref().and_then(|w| w.document());
            let document_for_handler = document.clone();
            let handler = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(
                move |_| {
                    if document_for_handler.as_ref().is_some_and(|d| d.hidden()) {
                        return;
                    }
                    let now = Date::now();
                    let previous = *last_resume_ms.borrow();
                    if now >= previous && now - previous < 750.0 {
                        return;
                    }
                    *last_resume_ms.borrow_mut() = now;
                    let next = {
                        let mut counter = resume_counter.borrow_mut();
                        *counter = counter.wrapping_add(1);
                        *counter
                    };
                    resume_generation.set(next);
                },
            );
            if let Some(document) = document.as_ref() {
                let _ = document.add_event_listener_with_callback(
                    "visibilitychange",
                    handler.as_ref().unchecked_ref(),
                );
            }
            if let Some(window) = window.as_ref() {
                for name in ["pageshow", "focus"] {
                    let _ = window.add_event_listener_with_callback(
                        name,
                        handler.as_ref().unchecked_ref(),
                    );
                }
            }
            move || {
                if let Some(document) = document {
                    let _ = document.remove_event_listener_with_callback(
                        "visibilitychange",
                        handler.as_ref().unchecked_ref(),
                    );
                }
                if let Some(window) = window {
                    for name in ["pageshow", "focus"] {
                        let _ = window.remove_event_listener_with_callback(
                            name,
                            handler.as_ref().unchecked_ref(),
                        );
                    }
                }
            }
        });
    }
    let mode = use_state(|| TrainingMode::Calculation);
    let study_clock = use_context::<StudyClockContext>();
    let training_immersive = use_state(|| false);
    // A paused paper hides the questions, so the effective-study clock stops too.
    let paper_paused = use_state(|| false);
    let module = match *mode {
        TrainingMode::Calculation => "calculation",
        TrainingMode::Flashcards => "flashcards",
        TrainingMode::Comprehensive => "comprehensive",
        TrainingMode::LogicJudgment => "logic",
        TrainingMode::QuantitativeRelations => "quantitative",
        TrainingMode::Statistics => "",
    };
    let study_active = *training_immersive && !*paper_paused;
    let csrf_ready = !props.csrf.is_empty();
    {
        let event = study_clock.as_ref().map(|v| v.event.clone());
        use_effect_with(
            (study_active, module, csrf_ready, *resume_generation),
            move |(active, module, _, _)| {
                if let Some(event) = &event {
                    if *active && !module.is_empty() {
                        event.emit((
                            "enter".into(),
                            Some((*module).into()),
                            Some("answer".into()),
                        ));
                    } else {
                        event.emit(("leave".into(), None, None));
                    }
                }
                || ()
            },
        );
    }
    {
        let event = study_clock.as_ref().map(|v| v.event.clone());
        use_effect_with((), move |_| {
            move || {
                if let Some(event) = event {
                    event.emit(("leave".into(), None, None));
                }
            }
        });
    }
    let on_immersive_change = {
        let training_immersive = training_immersive.clone();
        Callback::from(move |active: bool| training_immersive.set(active))
    };
    let on_paper_pause_change = {
        let paper_paused = paper_paused.clone();
        Callback::from(move |paused: bool| paper_paused.set(paused))
    };
    let exit_comprehensive = {
        let training_immersive = training_immersive.clone();
        let mode = mode.clone();
        Callback::from(move |_: ()| {
            training_immersive.set(false);
            mode.set(TrainingMode::Calculation);
        })
    };
    let choose_calculation = {
        let mode = mode.clone();
        let training_immersive = training_immersive.clone();
        Callback::from(move |_| {
            training_immersive.set(false);
            mode.set(TrainingMode::Calculation);
        })
    };
    let choose_comprehensive = {
        let mode = mode.clone();
        let training_immersive = training_immersive.clone();
        Callback::from(move |_| {
            training_immersive.set(false);
            mode.set(TrainingMode::Comprehensive);
        })
    };
    let choose_flashcards = {
        let mode = mode.clone();
        let training_immersive = training_immersive.clone();
        Callback::from(move |_| {
            training_immersive.set(false);
            mode.set(TrainingMode::Flashcards);
        })
    };
    let choose_quantitative = {
        let mode = mode.clone();
        Callback::from(move |_| mode.set(TrainingMode::QuantitativeRelations))
    };
    let choose_logic = {
        let mode = mode.clone();
        Callback::from(move |_| mode.set(TrainingMode::LogicJudgment))
    };
    let choose_statistics = {
        let mode = mode.clone();
        let training_immersive = training_immersive.clone();
        Callback::from(move |_| {
            training_immersive.set(false);
            mode.set(TrainingMode::Statistics);
        })
    };
    let back_to_study = {
        let route = props.route_state.clone();
        Callback::from(move |_| {
            navigate_route(
                &route,
                PortalRoute::study(StudyView::Dashboard, None, Vec::new()),
                false,
            )
        })
    };
    html! {
        <div ref={shell} class="training-shell" data-testid="training-layout">
            if !*training_immersive {
                <header class="training-topbar" data-testid="training-header">
                    <div class="training-topbar-inner">
                        <button type="button" class="btn btn-ghost btn-sm btn-square" aria-label="返回学习中心" data-testid="training-back" onclick={back_to_study}>{"‹"}</button>
                        <h1 class="text-lg font-bold">{"专项训练"}</h1>
                        <div class="ml-auto"><StudyTimeInline /></div>
                        <details class="training-more">
                            <summary class="btn btn-ghost btn-sm btn-square" aria-label="应用选项">{"⋯"}</summary>
                            <div class="training-more-panel"><PwaControls /></div>
                        </details>
                    </div>
                </header>
                <nav class="training-mode-tabs" data-testid="training-modes">
                    <div class="training-modes-inner" role="group" aria-label="训练模式">
                        <button type="button" class="training-mode" data-testid="training-mode-calculation" aria-pressed={(*mode == TrainingMode::Calculation).to_string()} onclick={choose_calculation}>{"速算训练"}</button>
                        <button type="button" class="training-mode" data-testid="training-mode-flashcards" aria-pressed={(*mode == TrainingMode::Flashcards).to_string()} onclick={choose_flashcards}>{"错题复练"}</button>
                        <button type="button" class="training-mode" data-testid="training-mode-comprehensive" aria-pressed={(*mode == TrainingMode::Comprehensive).to_string()} onclick={choose_comprehensive}>{"资料分析"}</button>
                        <button type="button" class="training-mode" data-testid="training-mode-logic-judgment" aria-pressed={(*mode == TrainingMode::LogicJudgment).to_string()} onclick={choose_logic}>{"判断推理"}</button>
                        <button type="button" class="training-mode" data-testid="training-mode-quantitative" aria-pressed={(*mode == TrainingMode::QuantitativeRelations).to_string()} onclick={choose_quantitative}>{"数量关系"}</button>
                        <button type="button" class="training-mode" data-testid="training-mode-statistics" aria-pressed={(*mode == TrainingMode::Statistics).to_string()} onclick={choose_statistics}>{"学习统计"}</button>
                    </div>
                </nav>
            }
            <main class="min-h-0 flex-1 overflow-hidden" data-testid="training-mode-content">
            if *mode == TrainingMode::Statistics {
                <TrainingStatistics />
            } else if *mode == TrainingMode::Calculation {
                {render_calculation_training(props.route_state.clone(),on_immersive_change.clone())}
            } else if *mode == TrainingMode::Flashcards {
                <FlashcardPractice route_state={props.route_state.clone()} on_immersive_change={on_immersive_change.clone()} />
            } else if matches!(*mode, TrainingMode::LogicJudgment | TrainingMode::QuantitativeRelations) {
                <LogicJudgmentTraining
                    key={if *mode == TrainingMode::QuantitativeRelations { "quantitative" } else { "logic" }}
                    csrf={props.csrf.clone()} on_immersive_change={on_immersive_change.clone()}
                    on_paper_pause_change={on_paper_pause_change.clone()}
                    quantitative={*mode == TrainingMode::QuantitativeRelations} />
            } else {
                <ComprehensiveTraining csrf={props.csrf.clone()} on_immersive_change={on_immersive_change} on_paper_pause_change={on_paper_pause_change} on_exit={exit_comprehensive} />
            }
            </main>
        </div>
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PracticeGrade {
    Correct,
    Wrong,
    Skipped,
}

#[derive(Clone)]
struct PracticeAnswer {
    grade: PracticeGrade,
    elapsed_ms: u32,
}

#[derive(Clone)]
struct PracticeSession {
    questions: Vec<FlashcardSummary>,
    answers: Vec<Option<PracticeAnswer>>,
    // Each card owns its reveal state and cumulative thinking time.
    revealed_elapsed_ms: Vec<Option<u32>>,
    thinking_elapsed_ms: Vec<u32>,
    index: usize,
    started_at: f64,
    finished: bool,
}

// Look ahead in a circular order, but never treat navigating away as skipping.
fn next_ungraded_card(answers: &[Option<PracticeAnswer>], index: usize) -> Option<usize> {
    if answers.is_empty() {
        return None;
    }
    (1..=answers.len())
        .map(|offset| (index + offset) % answers.len())
        .find(|candidate| answers[*candidate].is_none())
}

impl PracticeSession {
    // Only unrevealed cards count thinking time; switching away pauses that
    // card's timer without discarding already elapsed time.
    fn record_thinking_time(&mut self, now: f64) {
        if self.revealed_elapsed_ms[self.index].is_none() && self.answers[self.index].is_none() {
            let elapsed = (now - self.started_at).max(0.0) as u32;
            self.thinking_elapsed_ms[self.index] =
                self.thinking_elapsed_ms[self.index].saturating_add(elapsed);
        }
        self.started_at = now;
    }

    fn go_to(&mut self, target: usize, now: f64) {
        if self.finished || target >= self.questions.len() || target == self.index {
            return;
        }
        self.record_thinking_time(now);
        self.index = target;
        self.started_at = now;
    }

    fn reveal_current(&mut self, now: f64) {
        if self.finished || self.revealed_elapsed_ms[self.index].is_some() {
            return;
        }
        self.record_thinking_time(now);
        self.revealed_elapsed_ms[self.index] = Some(self.thinking_elapsed_ms[self.index]);
    }

    fn rate_current(&mut self, grade: PracticeGrade, now: f64) {
        if self.finished
            || self.revealed_elapsed_ms[self.index].is_none()
            || self.answers[self.index].is_some()
        {
            return;
        }
        let index = self.index;
        self.answers[index] = Some(PracticeAnswer {
            grade,
            elapsed_ms: self.revealed_elapsed_ms[index].unwrap_or_default(),
        });
        if let Some(next) = next_ungraded_card(&self.answers, index) {
            self.go_to(next, now);
        } else {
            self.finished = true;
        }
    }
}

#[derive(Properties, PartialEq)]
struct FlashcardPracticeProps {
    route_state: UseStateHandle<PortalRoute>,
    on_immersive_change: Callback<bool>,
}

fn shuffle_practice_cards(cards: &mut [FlashcardSummary], mut seed: u64) {
    if seed == 0 {
        seed = 0x9e37_79b9_7f4a_7c15;
    }
    for i in (1..cards.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        cards.swap(i, (seed % (i as u64 + 1)) as usize);
    }
}

async fn fetch_practice_cards(
    deck: &str,
    tags: &[String],
) -> Result<Vec<FlashcardSummary>, String> {
    let mut cards = Vec::new();
    let mut ids = HashSet::new();
    loop {
        let query = study_query(
            (!deck.is_empty()).then_some(deck),
            tags,
            Some(PRACTICE_PAGE_SIZE),
            Some(cards.len() as u32),
        );
        let response = fetch_json_typed::<StudyCardsResponse>(
            &format!("{STUDY_CARDS_ENDPOINT}{query}"),
            "错题复练题库",
        )
        .await
        .map_err(|error| error.to_string())?;
        let count = response.cards.len();
        if cards.len() + count > PRACTICE_MAX_CANDIDATES {
            return Err(format!(
                "符合条件的卡片超过 {PRACTICE_MAX_CANDIDATES} 张，请缩小 Deck / Tag 范围。"
            ));
        }
        for card in response.cards {
            if !ids.insert(card.id.as_str().to_owned()) {
                return Err("分页结果出现重复卡片，请刷新后重试。".to_owned());
            }
            cards.push(card);
        }
        if count < PRACTICE_PAGE_SIZE as usize {
            break;
        }
    }
    Ok(cards)
}

#[function_component(FlashcardPractice)]
fn flashcard_practice(props: &FlashcardPracticeProps) -> Html {
    let decks = use_state(Vec::<DeckSummary>::new);
    let available_tags = use_state(Vec::<TagSummary>::new);
    let scope_error = use_state(|| None::<String>);
    let deck = use_state(String::new);
    let tags = use_state(|| vec!["错题".to_owned()]);
    let requested_count = use_state(|| 20_usize);
    let candidates = use_state(|| None::<Vec<FlashcardSummary>>);
    let candidate_error = use_state(|| None::<String>);
    let session = use_state(|| None::<PracticeSession>);

    {
        let decks = decks.clone();
        let available_tags = available_tags.clone();
        let scope_error = scope_error.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let mut problems = Vec::new();
                match fetch_json_typed::<StudyDecksResponse>(STUDY_DECKS_ENDPOINT, "Deck 列表")
                    .await
                {
                    Ok(response) => decks.set(response.decks),
                    Err(error) => problems.push(error.to_string()),
                }
                match fetch_json_typed::<StudyTagsResponse>(STUDY_TAGS_ENDPOINT, "Tag 列表").await
                {
                    Ok(response) => available_tags.set(response.tags),
                    Err(error) => problems.push(error.to_string()),
                }
                if !problems.is_empty() {
                    scope_error.set(Some(problems.join("；")));
                }
            });
            || ()
        });
    }

    {
        let candidates = candidates.clone();
        let candidate_error = candidate_error.clone();
        use_effect_with(((*deck).clone(), (*tags).clone()), move |(deck, tags)| {
            let cancelled = std::rc::Rc::new(std::cell::Cell::new(false));
            let token = cancelled.clone();
            let selected_deck = deck.clone();
            let selected_tags = tags.clone();
            candidates.set(None);
            candidate_error.set(None);
            spawn_local(async move {
                let result = fetch_practice_cards(&selected_deck, &selected_tags).await;
                if token.get() {
                    return;
                }
                match result {
                    Ok(cards) => candidates.set(Some(cards)),
                    Err(error) => candidate_error.set(Some(error)),
                }
            });
            move || cancelled.set(true)
        });
    }

    let current = (*session).clone();
    let is_training_immersive = current.as_ref().is_some_and(|state| !state.finished);
    {
        let notify = props.on_immersive_change.clone();
        use_effect_with(is_training_immersive, move |active| {
            notify.emit(*active);
            || ()
        });
    }
    {
        let math_state = current.as_ref().map(|value| {
            (
                value.index,
                value.revealed_elapsed_ms[value.index].is_some(),
                value.finished,
            )
        });
        use_effect_with(math_state, move |_| {
            hydrate_math();
            || ()
        });
    }

    let go_back = {
        let route_state = props.route_state.clone();
        let session = session.clone();
        Callback::from(move |_| {
            if session.is_some() {
                session.set(None);
            } else {
                navigate_route(
                    &route_state,
                    PortalRoute::study(StudyView::Dashboard, None, Vec::new()),
                    false,
                );
            }
        })
    };
    let content = if let Some(live) = current {
        if live.finished {
            let answered = live.answers.iter().flatten().collect::<Vec<_>>();
            let correct = answered
                .iter()
                .filter(|value| value.grade == PracticeGrade::Correct)
                .count();
            let wrong = answered
                .iter()
                .filter(|value| value.grade == PracticeGrade::Wrong)
                .count();
            let skipped = answered
                .iter()
                .filter(|value| value.grade == PracticeGrade::Skipped)
                .count();
            let unanswered = live.answers.len() - answered.len();
            let scored = answered
                .iter()
                .filter(|value| value.grade != PracticeGrade::Skipped)
                .collect::<Vec<_>>();
            let average_ms = if scored.is_empty() {
                0
            } else {
                scored
                    .iter()
                    .map(|value| value.elapsed_ms as u64)
                    .sum::<u64>()
                    / scored.len() as u64
            };
            let restart = {
                let session = session.clone();
                Callback::from(move |_| session.set(None))
            };
            html! {
                <section class="mx-auto grid max-w-4xl gap-4 px-4 pb-8 pt-5" data-testid="practice-summary">
                    <h2 class="text-xl font-bold">{"错题复练 · 本轮结果"}</h2>
                    <div class="grid grid-cols-2 gap-3 sm:grid-cols-6">
                        <div class="stat rounded-box bg-base-100"><div class="stat-title">{"已答"}</div><div class="stat-value text-2xl">{scored.len()}</div></div>
                        <div class="stat rounded-box bg-base-100"><div class="stat-title">{"做对"}</div><div class="stat-value text-2xl">{correct}</div></div>
                        <div class="stat rounded-box bg-base-100"><div class="stat-title">{"做错"}</div><div class="stat-value text-2xl">{wrong}</div></div>
                        <div class="stat rounded-box bg-base-100"><div class="stat-title">{"跳过"}</div><div class="stat-value text-2xl">{skipped}</div></div>
                        <div class="stat rounded-box bg-base-100" data-testid="practice-unanswered"><div class="stat-title">{"未答"}</div><div class="stat-value text-2xl">{unanswered}</div></div>
                        <div class="stat rounded-box bg-base-100"><div class="stat-title">{"平均用时"}</div><div class="stat-value text-2xl">{format!("{:.1}s", average_ms as f64 / 1000.0)}</div></div>
                    </div>
                    <h3 class="font-semibold">{"本轮做错的卡片"}</h3>
                    if wrong == 0 {
                        <p class="text-sm text-base-content/60">{"本轮没有标记为做错的卡片。"}</p>
                    } else {
                        {for live.questions.iter().enumerate().filter_map(|(index, card)| {
                            (live.answers[index].as_ref().is_some_and(|answer| answer.grade == PracticeGrade::Wrong))
                                .then_some(html! {
                                    <article class="rounded-box border border-base-content/10 bg-base-100 p-4" key={card.id.as_str().to_owned()}>
                                        <p class="text-xs text-base-content/60">{format!("卡片 {}", card.id.as_str())}</p>
                                        <div class="study-markdown mt-2">{render_markdown(&card.front_markdown, &card.source_file)}</div>
                                        <details class="mt-2"><summary class="cursor-pointer text-sm">{"展开原解析"}</summary><div class="study-markdown mt-2">{render_markdown(&card.back_markdown, &card.source_file)}</div></details>
                                    </article>
                                })
                        })}
                    }
                    <p class="text-xs text-base-content/60">{"本轮统计仅在页面会话中保留，不写入 FSRS 或训练历史。"}</p>
                    <div class="flex gap-3">
                        <button type="button" class="btn btn-primary" data-testid="practice-restart" onclick={restart}>{"再练一轮"}</button>
                        <button type="button" class="btn btn-ghost" onclick={go_back.clone()}>{"返回专项训练"}</button>
                    </div>
                </section>
            }
        } else {
            let question = live.questions[live.index].clone();
            let progress = live.index + 1;
            let reveal = {
                let session = session.clone();
                Callback::from(move |_| {
                    let Some(mut state) = (*session).clone() else {
                        return;
                    };
                    state.reveal_current(Date::now());
                    session.set(Some(state));
                })
            };
            let rate = |grade: PracticeGrade| {
                let session = session.clone();
                Callback::from(move |_| {
                    let Some(mut state) = (*session).clone() else {
                        return;
                    };
                    state.rate_current(grade, Date::now());
                    session.set(Some(state));
                })
            };
            let go_to = {
                let session = session.clone();
                Callback::from(move |target: usize| {
                    let Some(mut state) = (*session).clone() else {
                        return;
                    };
                    if target >= state.questions.len() || target == state.index {
                        return;
                    }
                    state.go_to(target, Date::now());
                    session.set(Some(state));
                })
            };
            let previous = {
                let go_to = go_to.clone();
                let index = live.index;
                Callback::from(move |_| go_to.emit(index.saturating_sub(1)))
            };
            let next = {
                let go_to = go_to.clone();
                let index = live.index;
                Callback::from(move |_| go_to.emit(index + 1))
            };
            let current_revealed = live.revealed_elapsed_ms[live.index].is_some();
            let current_grade = live.answers[live.index].as_ref().map(|v| v.grade);
            let tabs = live.answers.iter().enumerate().map(|(i, answer)| {
                let (grade, color) = match answer.as_ref().map(|answer| answer.grade) {
                    Some(PracticeGrade::Correct) => ("correct", "btn-success"),
                    Some(PracticeGrade::Wrong) => ("wrong", "btn-error"),
                    Some(PracticeGrade::Skipped) => ("skipped", "btn-warning"),
                    None => ("unanswered", "btn-ghost"),
                };
                let style = if i == live.index {
                    format!("btn btn-xs shrink-0 ring-2 ring-primary ring-offset-1 {color}")
                } else if answer.is_some() {
                    format!("btn btn-xs btn-outline shrink-0 {color}")
                } else {
                    "btn btn-xs btn-ghost shrink-0".to_owned()
                };
                let go_to = go_to.clone();
                html! {
                    <button type="button" class={style} data-testid="practice-question-tab"
                        data-grade={grade} aria-current={if i == live.index { "step" } else { "false" }}
                        aria-label={format!("第{}题",i+1)}
                        onclick={Callback::from(move |_| go_to.emit(i))}>{i+1}</button>
                }
            });
            let skip_to_summary = {
                let session = session.clone();
                Callback::from(move |_| {
                    let Some(mut state) = (*session).clone() else {
                        return;
                    };
                    state.finished = true;
                    session.set(Some(state));
                })
            };
            html! {
                <section class="mx-auto flex h-full min-h-0 w-full max-w-6xl flex-col gap-2" data-testid="practice-question">
                    <div class="min-h-0 flex-1">
                        <InkOverlay question_index={live.index} responsive={true}>
                            <div class="flex h-full min-h-0 flex-col gap-2">
                                <div class="relative z-30 flex shrink-0 flex-wrap items-center justify-between gap-2 py-1 text-sm text-base-content/60" data-testid="practice-card-header">
                                    <span>{format!("第 {progress} / {} 题", live.questions.len())}</span>
                                    <div class="ml-auto flex min-w-0 items-center gap-2"><span class="truncate">{question.deck_path.clone()}</span><InkToolbarSlot /></div>
                                </div>
                                <article class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100">
                                    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4" data-testid="practice-card-scroll">
                                        <div class="study-markdown" data-testid="practice-front">{render_markdown(&question.front_markdown, &question.source_file)}</div>
                                        if current_revealed {
                                            <div class="mt-4 border-t border-base-content/10 pt-3" data-testid="practice-back">
                                                <h3 class="mb-3 font-bold">{"原卡片解析"}</h3>
                                                <div class="study-markdown">{render_markdown(&question.back_markdown, &question.source_file)}</div>
                                            </div>
                                        }
                                    </div>
                                </article>
                            </div>
                        </InkOverlay>
                    </div>
                    <footer class="flex shrink-0 flex-col gap-2 py-1" data-testid="practice-controls">
                        if let Some(grade) = current_grade {
                            <div class="rounded-box bg-base-100 px-3 py-2 text-center text-sm" data-testid="practice-graded">
                                {format!("已评价：{}",match grade {
                                    PracticeGrade::Correct => "做对",
                                    PracticeGrade::Wrong => "做错",
                                    PracticeGrade::Skipped => "跳过",
                                })}
                            </div>
                        } else if !current_revealed {
                            <button type="button" class="btn btn-primary" data-testid="practice-reveal" onclick={reveal}>{"显示解析"}</button>
                        } else {
                            <div class="grid grid-cols-3 gap-2" data-testid="practice-grading">
                                <button type="button" class="btn btn-success" data-testid="practice-correct" onclick={rate(PracticeGrade::Correct)}>{"做对"}</button>
                                <button type="button" class="btn btn-error" data-testid="practice-wrong" onclick={rate(PracticeGrade::Wrong)}>{"做错"}</button>
                                <button type="button" class="btn btn-outline" data-testid="practice-skip" onclick={rate(PracticeGrade::Skipped)}>{"跳过"}</button>
                            </div>
                        }
                        <nav class="flex shrink-0 items-center gap-2" aria-label="错题复练题号导航" data-testid="practice-navigation">
                            <button type="button" class="btn btn-sm btn-outline shrink-0" data-testid="practice-previous"
                                disabled={live.index == 0} onclick={previous}>{"上一题"}</button>
                            <div class="flex min-w-0 flex-1 items-center gap-2 overflow-x-auto px-1 py-1" data-testid="practice-tab-strip">
                                {for tabs}
                            </div>
                            <button type="button" class="btn btn-sm btn-outline shrink-0" data-testid="practice-next"
                                disabled={live.index + 1 == live.questions.len()} onclick={next}>{"下一题"}</button>
                        </nav>
                        <button type="button" class="btn btn-ghost btn-sm self-center" data-testid="practice-finish" onclick={skip_to_summary}>{"提前结束并查看成绩"}</button>
                    </footer>
                </section>
            }
        }
    } else {
        let pick_deck = {
            let deck = deck.clone();
            Callback::from(move |event: Event| {
                deck.set(event.target_unchecked_into::<HtmlSelectElement>().value());
            })
        };
        let count = *requested_count;
        let start = {
            let candidates = candidates.clone();
            let requested_count = requested_count.clone();
            let session = session.clone();
            Callback::from(move |_| {
                let Some(cards) = (*candidates).clone() else {
                    return;
                };
                if cards.is_empty() {
                    return;
                }
                let mut questions = cards;
                shuffle_practice_cards(&mut questions, Date::now() as u64);
                questions.truncate((*requested_count).min(questions.len()));
                session.set(Some(PracticeSession {
                    answers: vec![None; questions.len()],
                    revealed_elapsed_ms: vec![None; questions.len()],
                    thinking_elapsed_ms: vec![0; questions.len()],
                    questions,
                    index: 0,
                    started_at: Date::now(),
                    finished: false,
                }));
            })
        };
        let candidate_count = (*candidates).as_ref().map(Vec::len).unwrap_or(0);
        html! {
            <TrainingSetupLayout test_id="practice-setup" summary={format!("本轮 {} 张",count.min(candidate_count))} detail={format!("{} 张候选卡片",candidate_count)}
                action={Some(html!{<button type="button" class={TRAINING_PRIMARY_BUTTON} data-testid="practice-start" disabled={candidate_count==0 || candidate_error.is_some()} onclick={start}>{"开始复练 →"}</button>})}>
                <div class="training-config">
                    <h2 class="font-bold">{"Flashcard 错题复练"}</h2>
                    <p class="mt-2 text-sm text-base-content/65">{"从已同步卡片读取 Front，手写后看 Back，自评不改变记忆复习计划。"}</p>
                    <div class="mt-4 grid gap-3">
                        <label class="grid gap-2">
                            <span class="font-semibold">{"卡组"}</span>
                            <select class="select select-bordered w-full text-base" data-testid="practice-deck-filter" value={(*deck).clone()} onchange={pick_deck}>
                                <option value="">{"全部卡组"}</option>
                                {for decks.iter().map(|item| html! {
                                    <option value={item.path.clone()} selected={*deck == item.path}>{format!("{} ({})", item.path, item.card_count)}</option>
                                })}
                            </select>
                        </label>
                        <fieldset class="grid gap-2">
                            <legend class="font-semibold">{"标签（多选取交集）"}</legend>
                            <div class="flex flex-wrap gap-2" data-testid="practice-tag-filter">
                                {for available_tags.iter().map(|tag| {
                                    let tag_name = tag.tag.clone();
                                    let selected = tags.contains(&tag_name);
                                    let tags = tags.clone();
                                    html! {
                                        <label class="flex items-center gap-2 rounded-box border border-base-content/10 px-3 py-2">
                                            <input type="checkbox" class="checkbox checkbox-sm" checked={selected} onchange={Callback::from(move |_| {
                                                let mut next = (*tags).clone();
                                                if let Some(index) = next.iter().position(|value| value == &tag_name) {
                                                    next.remove(index);
                                                } else {
                                                    next.push(tag_name.clone());
                                                }
                                                next.sort();
                                                tags.set(next);
                                            })}/>
                                            <span>{format!("{} ({})", tag.tag, tag.card_count)}</span>
                                        </label>
                                    }
                                })}
                            </div>
                            <p class="text-xs text-base-content/60">{"默认选中「错题」，可叠加其他标签或取消以练习全部卡片。"}</p>
                        </fieldset>
                        <div class="training-count-row mt-3"><span class="font-semibold">{"本轮题数"}</span><TrainingQuestionCount value={count} unit="张" test_id="practice-count" on_change={{let count=requested_count.clone();Callback::from(move |value|count.set(value))}} /></div>
                    </div>
                    if let Some(error) = (*scope_error).clone() {
                        <p class="mt-3 text-sm text-error" role="alert">{error}</p>
                    }
                    if let Some(error) = (*candidate_error).clone() {
                        <p class="mt-3 text-sm text-error" role="alert" data-testid="practice-error">{error}</p>
                    } else if candidates.is_none() {
                        <p class="mt-3 text-sm text-base-content/60" data-testid="practice-loading">{"正在读取卡片…"}</p>
                    } else if candidate_count == 0 {
                        <p class="mt-3 text-sm text-base-content/60" data-testid="practice-empty">{"没有符合条件的已同步卡片。请调整 Deck/Tag，或返回 Flashcard 同步。"}</p>
                    } else {
                        <p class="mt-3 text-sm" data-testid="practice-candidates">
                            {format!("共 {candidate_count} 张候选卡片，本轮将抽取 {} 张。", count.min(candidate_count))}
                            if candidate_count < count {
                                <span class="text-warning">{" 候选数量不足，按实际数量练习。"}</span>
                            }
                        </p>
                    }
                </div>
            </TrainingSetupLayout>
        }
    };

    html! {
        <div class={if is_training_immersive { format!("calculation-layout {}",FIXED_TRAINING_LAYOUT) } else { "calculation-layout flex h-full min-h-0 flex-col overflow-hidden".to_owned() }} data-testid="practice-layout" oncontextmenu={Callback::from(|event: web_sys::MouseEvent| event.prevent_default())}>
            if is_training_immersive {
            <div class="mx-auto flex w-full shrink-0 justify-end pb-2">
                <button type="button" class="btn btn-ghost btn-sm" data-testid="practice-back-to-study" onclick={go_back}>{"← 返回专项训练"}</button>
                <StudyTimeInline />
            </div>
            }
            if is_training_immersive {
                <div class="min-h-0 flex-1 overflow-hidden">{content}</div>
            } else if session.as_ref().is_some_and(|state|state.finished) {
                <TrainingSetupLayout test_id="practice-results-layout">{content}</TrainingSetupLayout>
            } else {
                {content}
            }
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyz_things::domain::flashcards::{FlashcardId, ReviewStateKind};

    fn example_card(number: usize) -> FlashcardSummary {
        FlashcardSummary {
            id: FlashcardId::from_trusted(format!("card-{number}")),
            source_file: "cards.md".into(),
            source_order: number as u32,
            deck_path: "资料分析".into(),
            front_markdown: format!("题目{number}"),
            back_markdown: "解析".into(),
            tags: vec!["错题".into()],
            active: true,
            due_at_ms: None,
            last_reviewed_at_ms: None,
            state: ReviewStateKind::New,
            review_count: 0,
            lapse_count: 0,
            is_new: true,
        }
    }

    #[test]
    fn navigation_preserves_unrevealed_time_without_grading_or_skipping() {
        let mut live = PracticeSession {
            questions: (0..3).map(example_card).collect(),
            answers: vec![None; 3],
            revealed_elapsed_ms: vec![None; 3],
            thinking_elapsed_ms: vec![0; 3],
            index: 0,
            started_at: 1000.0,
            finished: false,
        };
        live.go_to(2, 1600.0);
        assert_eq!(live.thinking_elapsed_ms, vec![600, 0, 0]);
        assert!(live.answers.iter().all(Option::is_none));
        live.go_to(0, 2000.0);
        live.reveal_current(2400.0);
        assert_eq!(live.revealed_elapsed_ms, vec![Some(1000), None, None]);
        live.go_to(1, 2600.0);
        live.reveal_current(2900.0);
        live.rate_current(PracticeGrade::Wrong, 3000.0);
        assert_eq!(live.index, 2);
        assert_eq!(live.answers[1].as_ref().unwrap().elapsed_ms, 300);
        assert!(live.answers[0].is_none());
        assert!(live.answers[2].is_none());
        live.go_to(0, 3100.0);
        assert_eq!(live.revealed_elapsed_ms[0], Some(1000));
        live.rate_current(PracticeGrade::Correct, 3200.0);
        assert_eq!(live.index, 2);
        live.reveal_current(3500.0);
        live.rate_current(PracticeGrade::Skipped, 3600.0);
        assert!(live.finished);
        assert_eq!(
            live.answers[2].as_ref().unwrap().grade,
            PracticeGrade::Skipped
        );
    }

    #[test]
    fn find_next_ungraded_card_wraps_and_does_not_mark_unanswered() {
        let graded = Some(PracticeAnswer {
            grade: PracticeGrade::Correct,
            elapsed_ms: 1250,
        });
        let answers = vec![None, graded.clone(), None, graded];
        assert_eq!(next_ungraded_card(&answers, 2), Some(0));
        assert_eq!(next_ungraded_card(&answers, 0), Some(2));
        assert!(answers[0].is_none());
        assert!(next_ungraded_card(&[answers[1].clone()], 0).is_none());
    }

    #[test]
    fn shuffling_preserves_distinct_card_ids() {
        // The exercise queue uses the exact source cards; order changes must never copy or drop cards.
        let mut values = (0..20).collect::<Vec<_>>();
        let mut seed = 17u64;
        for i in (1..values.len()).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            values.swap(i, (seed % (i as u64 + 1)) as usize);
        }
        values.sort();
        assert_eq!(values, (0..20).collect::<Vec<_>>());
    }
}
