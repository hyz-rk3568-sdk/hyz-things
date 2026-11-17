//! 申论训练：复用 Question Bank 真题复盘工作区，而非建立第二套考试 Session。
use super::ocr_panel::{OcrAttempt, OcrEvent, OcrRunSnapshot};
use super::practice::{use_practice_favorite, QuestionBankQuestionHeader};
use super::*;
use hyz_things::domain::flashcards::FlashcardSummary;
use hyz_things::domain::question_bank::{
    FavoriteState, PaperSummary, PaperView, PracticeBrowseItem, PracticeQuestionDetail,
    QuestionView, SetFavorite,
};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ShenlunMode {
    Analysis,
    Grading,
    Ocr,
    Source,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShenlunCollectionKind {
    Favorites,
    Cards,
}

fn shenlun_card_category(deck_path: &str) -> String {
    let mut parts = deck_path.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("用户整理" | "真题练习"), Some("申论"), Some(category)) => {
            category.to_owned()
        }
        _ => "未分类".into(),
    }
}

fn shenlun_cards_url(deck: &str) -> String {
    study_cards_url(Some(deck), 0)
}

impl ShenlunMode {
    const fn ai_scope(self) -> Option<&'static str> {
        match self {
            Self::Analysis => Some("analysis"),
            Self::Grading => Some("grading"),
            Self::Ocr | Self::Source => None,
        }
    }

    const fn prompt_mode(self) -> Option<&'static str> {
        match self {
            Self::Analysis => Some("analysis"),
            Self::Grading => Some("grade"),
            Self::Ocr | Self::Source => None,
        }
    }

    const fn key(self) -> &'static str {
        match self {
            Self::Analysis => "ai",
            Self::Grading => "grade",
            Self::Ocr => "ocr",
            Self::Source => "source",
        }
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct QuestionBankShenlunProps {
    pub(crate) active: bool,
    pub(crate) on_immersive_change: Callback<bool>,
    pub(crate) csrf: String,
}

fn topic_label(question: &QuestionView) -> &str {
    match question.question_type.as_deref() {
        Some("summary" | "generalization" | "归纳概括") => "归纳概括",
        Some("analysis" | "综合分析") => "综合分析",
        Some("measures" | "countermeasure" | "提出对策") => "提出对策",
        Some("document_writing" | "document" | "official_document" | "公文写作") => "公文写作",
        Some("essay" | "composition" | "大作文") => "大作文",
        _ => "其他",
    }
}

#[derive(serde::Serialize)]
struct OcrStartRequest {
    attachments: Vec<String>,
}

#[derive(serde::Deserialize)]
struct OcrStartReceipt {
    run_id: String,
}

type OcrAttempts = HashMap<String, Vec<OcrAttempt>>;
type OcrAttemptStore = Rc<RefCell<OcrAttempts>>;

fn push_ocr_attempt(
    store: &OcrAttemptStore,
    attempts: &UseStateHandle<OcrAttempts>,
    question_id: &str,
    attempt: OcrAttempt,
) {
    let next = {
        let mut current = store.borrow_mut();
        current
            .entry(question_id.to_owned())
            .or_default()
            .push(attempt);
        current.clone()
    };
    attempts.set(next);
}

fn update_ocr_attempt(
    store: &OcrAttemptStore,
    attempts: &UseStateHandle<OcrAttempts>,
    question_id: &str,
    local_id: u64,
    update: impl FnOnce(&mut OcrAttempt) -> bool,
) -> bool {
    let (terminal, next) = {
        let mut current = store.borrow_mut();
        let Some(attempt) = current.get_mut(question_id).and_then(|values| {
            values
                .iter_mut()
                .find(|attempt| attempt.local_id == local_id)
        }) else {
            return false;
        };
        let terminal = update(attempt);
        (terminal, current.clone())
    };
    attempts.set(next);
    terminal
}

fn apply_ocr_snapshot(
    store: &OcrAttemptStore,
    attempts: &UseStateHandle<OcrAttempts>,
    question_id: &str,
    local_id: u64,
    run_id: &str,
    snapshot: OcrRunSnapshot,
) -> bool {
    if snapshot.version != 1 || snapshot.question_id != question_id || snapshot.run_id != run_id {
        return false;
    }
    update_ocr_attempt(store, attempts, question_id, local_id, |attempt| {
        attempt.run_id = Some(snapshot.run_id);
        attempt.seq = snapshot.seq;
        attempt.status = snapshot.status;
        attempt.events = snapshot.progress;
        attempt.markdown = snapshot.markdown;
        attempt.error = snapshot.error;
        matches!(attempt.status.as_str(), "complete" | "failed" | "stopped")
    })
}

fn apply_ocr_event(
    store: &OcrAttemptStore,
    attempts: &UseStateHandle<OcrAttempts>,
    question_id: &str,
    local_id: u64,
    run_id: &str,
    event: OcrEvent,
) -> bool {
    if event.version != 1 || event.run_id != run_id {
        return false;
    }
    update_ocr_attempt(store, attempts, question_id, local_id, |attempt| {
        if event.seq <= attempt.seq || event.seq != attempt.seq.saturating_add(1) {
            return false;
        }
        attempt.seq = event.seq;
        match event.kind.as_str() {
            "tool_started"
            | "tool_finished"
            | "command_output_delta"
            | "tool_output_delta"
            | "tool_progress" => attempt.events.push(event.clone()),
            "run_finished" => {
                attempt.status = "complete".into();
                attempt.markdown = event
                    .payload
                    .get("markdown")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
            }
            "run_failed" => {
                attempt.status = "failed".into();
                attempt.error = event
                    .payload
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
            }
            _ => {}
        }
        matches!(attempt.status.as_str(), "complete" | "failed" | "stopped")
    })
}

struct OcrEventSource {
    source: web_sys::EventSource,
    handlers: Vec<(String, Closure<dyn FnMut(web_sys::MessageEvent)>)>,
}

impl Drop for OcrEventSource {
    fn drop(&mut self) {
        self.source.close();
        for (name, handler) in &self.handlers {
            let _ = self
                .source
                .remove_event_listener_with_callback(name, handler.as_ref().unchecked_ref());
        }
    }
}

fn subscribe_ocr_stream(
    question_id: String,
    local_id: u64,
    run_id: String,
    store: OcrAttemptStore,
    attempts: UseStateHandle<OcrAttempts>,
    streams: Rc<RefCell<Vec<OcrEventSource>>>,
) {
    let endpoint =
        format!("/api/v1/question-bank/shenlun/questions/{question_id}/ocr/{run_id}/events");
    let source = match web_sys::EventSource::new(&endpoint) {
        Ok(source) => source,
        Err(_) => {
            update_ocr_attempt(&store, &attempts, &question_id, local_id, |attempt| {
                attempt.status = "failed".into();
                attempt.error = Some("无法连接 OCR 实时进度".into());
                true
            });
            return;
        }
    };
    let snapshot_source = source.clone();
    let snapshot_store = store.clone();
    let snapshot_attempts = attempts.clone();
    let snapshot_question = question_id.clone();
    let snapshot_run = run_id.clone();
    let snapshot = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
        let Some(data) = event.data().as_string() else {
            return;
        };
        let Ok(snapshot) = serde_json::from_str::<OcrRunSnapshot>(&data) else {
            return;
        };
        if apply_ocr_snapshot(
            &snapshot_store,
            &snapshot_attempts,
            &snapshot_question,
            local_id,
            &snapshot_run,
            snapshot,
        ) {
            snapshot_source.close();
        }
    }) as Box<dyn FnMut(_)>);
    let _ = source.add_event_listener_with_callback("snapshot", snapshot.as_ref().unchecked_ref());

    let message_source = source.clone();
    let message_store = store.clone();
    let message_attempts = attempts.clone();
    let message_question = question_id.clone();
    let message_run = run_id.clone();
    let message = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
        let Some(data) = event.data().as_string() else {
            return;
        };
        let Ok(event) = serde_json::from_str::<OcrEvent>(&data) else {
            return;
        };
        if apply_ocr_event(
            &message_store,
            &message_attempts,
            &message_question,
            local_id,
            &message_run,
            event,
        ) {
            message_source.close();
        }
    }) as Box<dyn FnMut(_)>);
    let _ = source.add_event_listener_with_callback("message", message.as_ref().unchecked_ref());
    streams.borrow_mut().push(OcrEventSource {
        source,
        handlers: vec![("snapshot".into(), snapshot), ("message".into(), message)],
    });
}

fn clear_ocr_session(
    store: &OcrAttemptStore,
    attempts: &UseStateHandle<OcrAttempts>,
    streams: &Rc<RefCell<Vec<OcrEventSource>>>,
    generation: &Rc<RefCell<u64>>,
) {
    let next_generation = generation.borrow().wrapping_add(1);
    *generation.borrow_mut() = next_generation;
    streams.borrow_mut().clear();
    *store.borrow_mut() = HashMap::new();
    attempts.set(HashMap::new());
}

#[function_component(QuestionBankShenlun)]
pub(crate) fn question_bank_shenlun(props: &QuestionBankShenlunProps) -> Html {
    let papers = use_state(|| None::<Vec<PaperSummary>>);
    let paper = use_state(|| None::<PaperView>);
    let selected_paper = use_state(String::new);
    let selected = use_state(|| 0_usize);
    let busy = use_state(|| false);
    let loading = use_state(|| true);
    let error = use_state(|| None::<String>);
    let mode = use_state(|| ShenlunMode::Analysis);
    let ai_delete_signal = use_state(|| 0_u32);
    let ai_delete_availability = use_state(|| HashMap::<(String, ShenlunMode), bool>::new());
    let scroll = use_training_scroll_pair();
    let question_key = (*paper).as_ref().and_then(|p| {
        p.questions
            .get(*selected)
            .map(|question| (p.id.clone(), question.id.clone()))
    });
    let favorite = use_practice_favorite(question_key.as_ref().map(|(_, id)| id.clone()));
    let favorite_busy = use_state(|| false);
    let flashcard_preview = use_state(|| None::<(String, FavoritePreview)>);
    let flashcard_error = use_state(|| None::<String>);
    let flashcard_open = use_state(|| false);
    let flashcard_revision = use_state(|| 0_u32);
    let collection_kind = use_state(|| None::<ShenlunCollectionKind>);
    let collections_revision = use_state(|| 0_u64);
    let favorites = use_state(|| None::<Vec<PracticeBrowseItem>>);
    let favorites_error = use_state(|| None::<String>);
    let favorites_retry = use_state(|| 0_u32);
    let cards = use_state(|| None::<Vec<FlashcardSummary>>);
    let cards_error = use_state(|| None::<String>);
    let cards_retry = use_state(|| 0_u32);
    let collection_busy = use_state(|| false);
    let pending_card_id = use_state(|| None::<String>);
    let initial_card_id = use_state(|| None::<String>);
    let ocr_attempts = use_state(OcrAttempts::new);
    let ocr_attempt_store = use_mut_ref(OcrAttempts::new);
    let ocr_next_id = use_mut_ref(|| 0_u64);
    let ocr_streams = use_mut_ref(Vec::<OcrEventSource>::new);
    let ocr_session_generation = use_mut_ref(|| 0_u64);
    {
        let attempts = ocr_attempts.clone();
        let store = ocr_attempt_store.clone();
        let streams = ocr_streams.clone();
        let generation = ocr_session_generation.clone();
        use_effect_with(props.active, move |active| {
            if !*active {
                clear_ocr_session(&store, &attempts, &streams, &generation);
            }
            || ()
        });
    }
    {
        let attempts = ocr_attempts.clone();
        let store = ocr_attempt_store.clone();
        let streams = ocr_streams.clone();
        let generation = ocr_session_generation.clone();
        use_effect_with((), move |_| {
            move || clear_ocr_session(&store, &attempts, &streams, &generation)
        });
    }
    let on_ocr_start = {
        let attempts = ocr_attempts.clone();
        let store = ocr_attempt_store.clone();
        let next_id = ocr_next_id.clone();
        let streams = ocr_streams.clone();
        let generation = ocr_session_generation.clone();
        let csrf = props.csrf.clone();
        Callback::from(move |(question_id, images): (String, Vec<PendingImage>)| {
            if images.is_empty() {
                return;
            }
            let local_id = {
                let mut next = next_id.borrow_mut();
                *next = next.wrapping_add(1);
                *next
            };
            let session_generation = *generation.borrow();
            push_ocr_attempt(
                &store,
                &attempts,
                &question_id,
                OcrAttempt {
                    local_id,
                    run_id: None,
                    images: images.clone(),
                    status: "starting".into(),
                    seq: 0,
                    events: Vec::new(),
                    markdown: None,
                    error: None,
                },
            );
            let body = OcrStartRequest {
                attachments: images.iter().map(|image| image.id.clone()).collect(),
            };
            let endpoint = format!("/api/v1/question-bank/shenlun/questions/{question_id}/ocr");
            let attempts = attempts.clone();
            let store = store.clone();
            let streams = streams.clone();
            let generation = generation.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let result = post_json_response::<_, OcrStartReceipt>(
                    &endpoint,
                    &csrf,
                    &body,
                    "手写图片转写",
                )
                .await;
                if *generation.borrow() != session_generation {
                    return;
                }
                match result {
                    Ok(receipt) => {
                        update_ocr_attempt(&store, &attempts, &question_id, local_id, |attempt| {
                            attempt.run_id = Some(receipt.run_id.clone());
                            attempt.status = "running".into();
                            false
                        });
                        subscribe_ocr_stream(
                            question_id,
                            local_id,
                            receipt.run_id,
                            store,
                            attempts,
                            streams,
                        );
                    }
                    Err(message) => {
                        update_ocr_attempt(&store, &attempts, &question_id, local_id, |attempt| {
                            attempt.status = "failed".into();
                            attempt.error = Some(message);
                            true
                        });
                    }
                }
            });
        })
    };
    {
        let flashcard_preview = flashcard_preview.clone();
        let flashcard_error = flashcard_error.clone();
        let flashcard_open = flashcard_open.clone();
        let pending_card_id = pending_card_id.clone();
        use_effect_with(
            (question_key.clone(), *flashcard_revision),
            move |(key, _)| {
                let alive = Rc::new(Cell::new(true));
                let guard = alive.clone();
                let qid = key.as_ref().map(|(_, qid)| qid.clone());
                // Opening from the card collection records the target card;
                // keep the editor mounted until its preview arrives.
                let keep_open = pending_card_id.is_some();
                flashcard_preview.set(None);
                flashcard_error.set(None);
                if !keep_open {
                    flashcard_open.set(false);
                }
                if let Some(qid) = qid {
                    let flashcard_preview = flashcard_preview.clone();
                    let flashcard_error = flashcard_error.clone();
                    let flashcard_open = flashcard_open.clone();
                    let pending_card_id = pending_card_id.clone();
                    spawn_local(async move {
                        let result = fetch_json::<FavoritePreview>(
                            &format!(
                                "/api/v1/question-bank/practice/questions/{qid}/flashcard-preview"
                            ),
                            "申论 Flashcard 预览",
                        )
                        .await;
                        if guard.get() {
                            match result {
                                Ok(preview) => {
                                    flashcard_preview.set(Some((qid.clone(), preview)));
                                    if pending_card_id.is_some() {
                                        flashcard_open.set(true);
                                        pending_card_id.set(None);
                                    }
                                }
                                Err(message) => {
                                    flashcard_error.set(Some(message));
                                    flashcard_open.set(false);
                                    pending_card_id.set(None);
                                }
                            }
                        }
                    });
                }
                move || alive.set(false)
            },
        );
    }

    {
        let favorites = favorites.clone();
        let favorites_error = favorites_error.clone();
        use_effect_with(
            (*collections_revision, *favorites_retry, props.active),
            move |(_, _, active)| {
                let alive = Rc::new(Cell::new(true));
                let guard = alive.clone();
                if *active {
                    favorites_error.set(None);
                    spawn_local(async move {
                        let result = fetch_json::<Vec<PracticeBrowseItem>>(
                            "/api/v1/question-bank/shenlun/favorites",
                            "申论收藏",
                        )
                        .await;
                        if guard.get() {
                            match result {
                                Ok(items) => favorites.set(Some(items)),
                                Err(message) => favorites_error.set(Some(message)),
                            }
                        }
                    });
                }
                move || alive.set(false)
            },
        );
    }
    {
        let cards = cards.clone();
        let cards_error = cards_error.clone();
        use_effect_with(
            (*collections_revision, *cards_retry, props.active),
            move |(_, _, active)| {
                let alive = Rc::new(Cell::new(true));
                let guard = alive.clone();
                if *active {
                    cards_error.set(None);
                    spawn_local(async move {
                        let first = fetch_json::<StudyCardsResponse>(
                            &shenlun_cards_url("用户整理/申论"),
                            "申论卡片",
                        )
                        .await;
                        let second = fetch_json::<StudyCardsResponse>(
                            &shenlun_cards_url("真题练习/申论"),
                            "申论卡片",
                        )
                        .await;
                        match (first, second) {
                            (Err(first), Err(_)) => {
                                if guard.get() {
                                    cards_error.set(Some(first));
                                }
                            }
                            (first, second) => {
                                let mut merged = Vec::new();
                                for result in [first, second] {
                                    if let Ok(response) = result {
                                        merged.extend(response.cards);
                                    }
                                }
                                merged.sort_by(|left, right| {
                                    (left.deck_path.as_str(), left.source_order, left.id.as_str())
                                        .cmp(&(
                                            right.deck_path.as_str(),
                                            right.source_order,
                                            right.id.as_str(),
                                        ))
                                });
                                merged.dedup_by(|left, right| left.id == right.id);
                                if guard.get() {
                                    cards.set(Some(merged));
                                }
                            }
                        }
                    });
                }
                move || alive.set(false)
            },
        );
    }

    {
        let papers = papers.clone();
        let loading = loading.clone();
        let error = error.clone();
        use_effect_with((), move |_| {
            let alive = Rc::new(Cell::new(true));
            let guard = alive.clone();
            loading.set(true);
            error.set(None);
            spawn_local(async move {
                let result = fetch_json::<Vec<PaperSummary>>(
                    "/api/v1/question-bank/shenlun/papers",
                    "申论原卷",
                )
                .await;
                if !guard.get() {
                    return;
                }
                match result {
                    Ok(value) => papers.set(Some(value)),
                    Err(message) => error.set(Some(message)),
                }
                loading.set(false);
            });
            move || alive.set(false)
        });
    }
    let open_target = {
        let busy = busy.clone();
        let paper = paper.clone();
        let selected = selected.clone();
        let mode = mode.clone();
        let error = error.clone();
        let availability = ai_delete_availability.clone();
        let immersive = props.on_immersive_change.clone();
        let flashcard_open = flashcard_open.clone();
        let flashcard_error = flashcard_error.clone();
        let pending_card_id = pending_card_id.clone();
        let initial_card_id = initial_card_id.clone();
        Callback::from(
            move |(id, question_id, card_id): (String, Option<String>, Option<String>)| {
                if *busy {
                    return;
                }
                busy.set(true);
                error.set(None);
                flashcard_error.set(None);
                pending_card_id.set(card_id.clone());
                initial_card_id.set(card_id.clone());
                flashcard_open.set(card_id.is_some());
                let busy = busy.clone();
                let paper = paper.clone();
                let selected = selected.clone();
                let mode = mode.clone();
                let error = error.clone();
                let availability = availability.clone();
                let immersive = immersive.clone();
                let flashcard_open = flashcard_open.clone();
                let pending_card_id = pending_card_id.clone();
                availability.set(HashMap::new());
                spawn_local(async move {
                    match fetch_json::<PaperView>(
                        &format!("/api/v1/question-bank/shenlun/papers/{id}"),
                        "申论原卷",
                    )
                    .await
                    {
                        Ok(value) => {
                            let index = question_id
                                .as_deref()
                                .and_then(|question_id| {
                                    value
                                        .questions
                                        .iter()
                                        .position(|question| question.id == question_id)
                                })
                                .unwrap_or(0);
                            selected.set(index);
                            mode.set(ShenlunMode::Analysis);
                            paper.set(Some(value));
                            immersive.emit(true);
                        }
                        Err(message) => {
                            error.set(Some(message));
                            flashcard_open.set(false);
                            pending_card_id.set(None);
                        }
                    }
                    busy.set(false);
                });
            },
        )
    };
    let close = {
        let paper = paper.clone();
        let mode = mode.clone();
        let availability = ai_delete_availability.clone();
        let immersive = props.on_immersive_change.clone();
        let attempts = ocr_attempts.clone();
        let store = ocr_attempt_store.clone();
        let streams = ocr_streams.clone();
        let generation = ocr_session_generation.clone();
        let collections_revision = collections_revision.clone();
        Callback::from(move |_| {
            clear_ocr_session(&store, &attempts, &streams, &generation);
            availability.set(HashMap::new());
            immersive.emit(false);
            mode.set(ShenlunMode::Analysis);
            paper.set(None);
            collections_revision.set(collections_revision.wrapping_add(1));
        })
    };
    if !props.active {
        return Html::default();
    }
    if let Some(current) = (*paper).as_ref() {
        let index = (*selected).min(current.questions.len().saturating_sub(1));
        let question = current.questions.get(index);
        let question_body = question.map(|item| html! {
            <div class="space-y-2">
                <p class="text-xs text-base-content/55">
                    {topic_label(item)}{" · "}
                    {item.max_score.map(|score| format!("{score} 分")).unwrap_or_else(|| "分值待确认".into())}
                </p>
                <div class="study-markdown">{render_markdown(&item.stem, &current.id)}</div>
            </div>
        }).unwrap_or_default();
        let paper_id = current.id.clone();
        let question_id = question.map(|item| item.id.clone()).unwrap_or_default();
        let material_scroll = scroll.first.clone();
        let material_edges = *scroll.first_edges;
        let mode_value = *mode;
        let ocr_question_attempts = (*ocr_attempts)
            .get(&question_id)
            .cloned()
            .unwrap_or_default();
        let ocr_start_for_question = {
            let start = on_ocr_start.clone();
            let question_id = question_id.clone();
            Callback::from(move |images| {
                start.emit((question_id.clone(), images));
            })
        };
        let icon_button = QUESTION_HEADER_ICON_BUTTON;

        let left = html! {
            <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100" data-testid="shenlun-materials">
                <div class={classes!(super::practice::QUESTION_BANK_PANE_HEADER, "question-bank-pane-header")}>
                    <strong class="text-sm">{"原卷材料"}</strong>
                    <div class="ml-auto flex shrink-0 gap-1" role="group" aria-label="材料滚动">
                        <MaterialScrollButton passage={material_scroll.clone()} up={true}
                            disabled={!material_edges.can_scroll_up} question_index={index}/>
                        <MaterialScrollButton passage={material_scroll.clone()} up={false}
                            disabled={!material_edges.can_scroll_down} question_index={index}/>
                    </div>
                </div>
                <div ref={material_scroll} onscroll={scroll.on_scroll.clone()}
                    class="min-h-0 flex-1 space-y-5 overflow-y-auto overscroll-contain px-4 pb-5 pt-3"
                    data-testid="shenlun-material-scroll" data-ink-pane="material">
                    {for current.materials.iter().map(|material| html! {
                        <article key={material.id.clone()} class="space-y-2">
                            <h3 class="font-semibold">{material.name.clone()}</h3>
                            <div class="study-markdown">{render_markdown(&material.markdown, &paper_id)}</div>
                        </article>
                    })}
                </div>
            </section>
        };
        let favorite_value = (*favorite.value)
            .as_ref()
            .filter(|state| state.question_id == question_id);
        let card_preview = (*flashcard_preview)
            .as_ref()
            .and_then(|(id, preview)| (id == &question_id).then_some(preview));
        let starred = favorite_value.is_some_and(|state| state.favorite);
        let favorite_ready = favorite_value.is_some();
        let on_favorite = {
            let csrf = props.csrf.clone();
            let qid = question_id.clone();
            let favorite_value = favorite.value.clone();
            let favorite_source = favorite.source.clone();
            let favorite_generation = favorite.generation.clone();
            let generation = *favorite.generation.borrow();
            let favorite_busy = favorite_busy.clone();
            let error = error.clone();
            Callback::from(move |_| {
                if !favorite_ready || *favorite_busy {
                    return;
                }
                favorite_busy.set(true);
                let csrf = csrf.clone();
                let qid = qid.clone();
                let favorite_value = favorite_value.clone();
                let favorite_source = favorite_source.clone();
                let favorite_generation = favorite_generation.clone();
                let favorite_busy = favorite_busy.clone();
                let error = error.clone();
                spawn_local(async move {
                    let result: Result<FavoriteState, String> = post_json_response(
                        &format!("/api/v1/question-bank/practice/favorites/{qid}"),
                        &csrf,
                        &SetFavorite { favorite: !starred },
                        "收藏申论原题",
                    )
                    .await;
                    match result {
                        Ok(state) if state.question_id == qid => {
                            if favorite_source.borrow().as_ref() == Some(&qid)
                                && *favorite_generation.borrow() == generation
                            {
                                favorite_value.set(Some(state));
                            }
                        }
                        Ok(_) => error.set(Some("收藏响应与当前题目不一致".into())),
                        Err(message) => error.set(Some(message)),
                    }
                    favorite_busy.set(false);
                });
            })
        };
        let leading = html! {
            <QuestionHeaderActions
                favorite={favorite_value.map(|state|state.favorite)}
                favorite_busy={*favorite_busy || !favorite_ready}
                favorite_label={if !favorite_ready {"正在加载收藏状态"} else if starred {"取消收藏"} else {"收藏题目"}}
                favorite_test_id="shenlun-favorite"
                on_favorite={on_favorite}
                flashcard_count={card_preview.map(|preview|preview.existing.len())}
                flashcard_can_create={card_preview.is_some_and(favorite_can_create)}
                flashcard_create_test_id="shenlun-create-flashcard"
                flashcard_manage_test_id="shenlun-manage-flashcard"
                on_flashcard={Callback::from({
                    let flashcard_open=flashcard_open.clone();
                    let pending_card_id=pending_card_id.clone();
                    let initial_card_id=initial_card_id.clone();
                    move |_|{
                        pending_card_id.set(None);
                        initial_card_id.set(None);
                        flashcard_open.set(true);
                    }
                })}/>
        };
        let can_delete = mode_value.ai_scope().is_some()
            && ai_delete_availability
                .get(&(question_id.clone(), mode_value))
                .copied()
                .unwrap_or(false);
        let menu = html! {
            <super::practice::QuestionAiMoreMenu
                can_delete={can_delete}
                reset_key={format!("{paper_id}:{question_id}:{}", mode_value.key())}
                button_test_id="shenlun-more"
                delete_test_id="shenlun-delete-current"
                button_label="更多题目操作"
                delete_label="删除 AI 对话"
                delete={Callback::from({
                    let signal = ai_delete_signal.clone();
                    move |_| signal.set(signal.wrapping_add(1))
                })}/>
        };
        let on_delete_availability = {
            let availability = ai_delete_availability.clone();
            let question_id = question_id.clone();
            let mode = mode_value;
            Callback::from(move |available| {
                let key = (question_id.clone(), mode);
                if availability.get(&key).copied() == Some(available) {
                    return;
                }
                let mut next = (*availability).clone();
                next.insert(key, available);
                availability.set(next);
            })
        };
        let right = html! {
            <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100" data-testid="shenlun-question">
                <QuestionBankQuestionHeader
                    title_test_id="shenlun-question-heading"
                    title={format!("原卷第 {} 题", question.map(|q|q.number).unwrap_or(0))}
                    subtitle={Some(question.map(topic_label).unwrap_or("申论").to_string())}
                    question_index={index}
                    target="题目"
                    label="申论题目与解析滚动"
                    passage={Some(scroll.second.clone())}
                    can_scroll_up={scroll.second_edges.can_scroll_up}
                    can_scroll_down={scroll.second_edges.can_scroll_down}
                    leading={leading}
                    tabs={html! {
                        <>
                            <button type="button" class={classes!(icon_button, if mode_value==ShenlunMode::Grading {"text-primary"} else {""})}
                                aria-label="AI 批改" title="AI 批改" aria-pressed={(mode_value==ShenlunMode::Grading).to_string()} data-testid="shenlun-mode-grade"
                                onclick={Callback::from({let mode=mode.clone();move |_|mode.set(ShenlunMode::Grading)})}>
                                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="3" width="16" height="18" rx="2"/><path d="M8 8h8M8 12h4m-4 4 2 2 4-4"/></svg>
                            </button>
                            <button type="button" class={classes!(icon_button, if mode_value==ShenlunMode::Ocr {"text-primary"} else {""})}
                                aria-label="OCR 转写" title="OCR 转写" aria-pressed={(mode_value==ShenlunMode::Ocr).to_string()} data-testid="shenlun-mode-ocr"
                                onclick={Callback::from({let mode=mode.clone();move |_|mode.set(ShenlunMode::Ocr)})}>
                                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 3H5a2 2 0 0 0-2 2v3M16 3h3a2 2 0 0 1 2 2v3M3 16v3a2 2 0 0 0 2 2h3m8 0h3a2 2 0 0 0 2-2v-3M7 12h10M9 9h6m-6 6h6"/></svg>
                            </button>
                            <super::practice::QuestionAiModeTabs
                                active={mode_value==ShenlunMode::Analysis}
                                original_active={Some(mode_value==ShenlunMode::Source)}
                                original_test_id="shenlun-mode-source"
                                ai_test_id="shenlun-mode-ai"
                                original_label="来源参考答案"
                                ai_label="AI 解析与追问"
                                original={Callback::from({let mode=mode.clone();move |_|mode.set(ShenlunMode::Source)})}
                                ai={Callback::from({let mode=mode.clone();move |_|mode.set(ShenlunMode::Analysis)})}/>
                        </>
                    }}
                    trailing={menu}/>
                if *flashcard_open {
                    if let Some(preview)=card_preview {
                        <div class="min-h-0 flex-1 overflow-y-auto p-3" data-testid="shenlun-flashcard-workspace">
                            <FavoriteFlashcardWorkspace key={question_id.clone()}
                                question_id={question_id.clone()} preview={preview.clone()}
                                csrf={props.csrf.clone()}
                                initial_card_id={(*initial_card_id).clone()}
                                on_close={Callback::from({
                                    let flashcard_open=flashcard_open.clone();
                                    let pending_card_id=pending_card_id.clone();
                                    move |_| {
                                        pending_card_id.set(None);
                                        flashcard_open.set(false);
                                    }
                                })}
                                on_changed={Callback::from({
                                    let flashcard_revision=flashcard_revision.clone();
                                    let pending_card_id=pending_card_id.clone();
                                    move |_| {
                                        pending_card_id.set(None);
                                        flashcard_revision.set(flashcard_revision.wrapping_add(1))
                                    }
                                })}/>
                        </div>
                    } else if let Some(message)=(*flashcard_error).as_ref() {
                        <div class="min-h-0 flex-1 overflow-y-auto p-4" data-testid="shenlun-flashcard-error">
                            <p role="alert" class="text-sm text-error">{message.clone()}</p>
                        </div>
                    } else {
                        <p class="min-h-0 flex-1 p-4 text-sm opacity-60" role="status" data-testid="shenlun-flashcard-loading">
                            {"正在准备闪卡…"}
                        </p>
                    }
                } else if let Some(prompt_mode) = mode_value.prompt_mode() {
                    <QuestionAiReview key={format!("{}:{}",question_id,mode_value.key())}
                        mode={prompt_mode}
                        draft_namespace={Some("shenlun")}
                        csrf={props.csrf.clone()} question_id={question_id.clone()}
                        question_content={question_body.clone()}
                        question_scroll={scroll.second.clone()} on_question_scroll={scroll.on_scroll.clone()}
                        delete_signal={*ai_delete_signal} on_delete_availability={on_delete_availability}/>
                } else if mode_value==ShenlunMode::Ocr {
                    <div class="flex min-h-0 flex-1 flex-col overflow-hidden" data-testid="shenlun-ocr-pane">
                        <super::ocr_panel::ShenlunAiPanel key={question_id.clone()}
                            question_id={question_id.clone()} csrf={props.csrf.clone()}
                            attempts={ocr_question_attempts.clone()} on_start={ocr_start_for_question.clone()}/>
                    </div>
                } else {
                    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4" ref={scroll.second.clone()}
                        onscroll={scroll.on_scroll.clone()} data-testid="shenlun-source-scroll" data-ink-pane="answer">
                        {question_body.clone()}
                        if mode_value==ShenlunMode::Source {
                            <super::source_answer_panel::ShenlunSourceAnswerPanel key={question_id.clone()}
                                question_id={question_id.clone()}/>
                        }
                        if let Some(message)=(*error).as_ref() {
                            <p role="alert" class="mt-2 text-sm text-error">{message.clone()}</p>
                        }
                    </div>
                }
            </section>
        };
        let navigation_allowed = Callback::from(|_: ()| true);
        let groups = vec![NavigationGroup {
            label: "全部题目".into(),
            subtitle: None,
            start: 0,
            end: current.questions.len(),
        }];
        // No formal paper session: do not show a misleading paper-wide "answered" tally.
        // This shared answer sheet is the sole question switcher in review mode.
        let answered = vec![false; current.questions.len()];
        html! {
            <TrainingDetailLayout root_class={classes!(super::practice::WORKSPACE, "gap-2")} test_id="shenlun-workspace"
                leading={html! {<>
                    <button type="button" class="btn btn-ghost btn-sm" onclick={close}>{"← 返回选题"}</button>
                    <div class="min-w-0 flex-1">
                        <strong class="block truncate text-sm">{current.title.clone()}</strong>
                        <span class="block truncate text-[11px] text-base-content/50">{"申论训练 · 原卷复盘"}</span>
                    </div>
                </>}} actions={Html::default()}
                content={html! {
                    <TrainingSwipeSurface current={selected.clone()} total={current.questions.len()}
                        navigation_allowed={Some(navigation_allowed.clone())}>
                        <InkOverlay key={format!("shenlun-{paper_id}-{question_id}")}
                            question_index={index} responsive={true} scroll_linked={true}
                            persist_key={format!("shenlun-{paper_id}-{question_id}")}>
                            <TrainingSplitWorkspace left={left} right={right}
                                test_id="shenlun-split" resizable={true} split_at_md={true}/>
                        </InkOverlay>
                    </TrainingSwipeSurface>
                }}
                navigation={html! {
                    <CompactQuestionNavigation current={selected.clone()}
                        navigation_allowed={Some(navigation_allowed)}
                        state_key={format!("shenlun-{paper_id}")}
                        numbers={current.questions.iter().map(|q|q.number).collect::<Vec<_>>()}
                        answered={answered} grades={None::<Vec<Option<bool>>>} groups={groups}/>
                }} />
        }
    } else {
        let available_papers = (*papers)
            .as_ref()
            .map(|items| {
                items
                    .iter()
                    .filter(|item| item.role == "primary")
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let selected_summary = available_papers
            .iter()
            .find(|item| item.id == *selected_paper);
        let selected_exists = selected_summary.is_some();
        let action_detail = selected_summary
            .map(|item| format!("{} · {} 题", item.title, item.questions))
            .unwrap_or_else(|| "请选择一份申论原卷".into());
        let start_review = {
            let selected_paper = selected_paper.clone();
            let open_target = open_target.clone();
            Callback::from(move |_| {
                let id = (*selected_paper).clone();
                if !id.is_empty() {
                    open_target.emit((id, None, None));
                }
            })
        };
        let start_action = html! {
            <button class={TRAINING_PRIMARY_BUTTON} data-testid="shenlun-start-review"
                disabled={!selected_exists || *busy || props.csrf.is_empty()} onclick={start_review}>
                {if *busy {"正在打开…"} else {"开始申论复盘"}}
            </button>
        };
        let select_paper = {
            let selected_paper = selected_paper.clone();
            Callback::from(move |id: String| selected_paper.set(id))
        };
        let choose_collection = {
            let collection_kind = collection_kind.clone();
            Callback::from(move |kind: ShenlunCollectionKind| {
                collection_kind.set(Some(kind));
            })
        };
        let retry_favorites = {
            let favorites_retry = favorites_retry.clone();
            Callback::from(move |_| favorites_retry.set(favorites_retry.wrapping_add(1)))
        };
        let retry_cards = {
            let cards_retry = cards_retry.clone();
            Callback::from(move |_| cards_retry.set(cards_retry.wrapping_add(1)))
        };
        let open_favorite = {
            let open_target = open_target.clone();
            Callback::from(move |(paper_id, question_id): (String, String)| {
                open_target.emit((paper_id, Some(question_id), None));
            })
        };
        let open_card = {
            let open_target = open_target.clone();
            let collection_busy = collection_busy.clone();
            let cards_error = cards_error.clone();
            Callback::from(move |(card_id, question_id): (String, String)| {
                if *collection_busy {
                    return;
                }
                collection_busy.set(true);
                cards_error.set(None);
                let open_target = open_target.clone();
                let collection_busy = collection_busy.clone();
                let cards_error = cards_error.clone();
                spawn_local(async move {
                    let result = fetch_json::<PracticeQuestionDetail>(
                        &format!("/api/v1/question-bank/practice/questions/{question_id}"),
                        "申论卡片来源题",
                    )
                    .await;
                    match result {
                        Ok(detail) => open_target.emit((
                            detail.item.paper_id,
                            Some(question_id),
                            Some(card_id),
                        )),
                        Err(message) => cards_error.set(Some(message)),
                    }
                    collection_busy.set(false);
                });
            })
        };
        let favorite_groups = {
            let mut groups = BTreeMap::<String, Vec<PracticeBrowseItem>>::new();
            for item in favorites.iter().flatten() {
                let category = item
                    .category
                    .clone()
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "未分类".into());
                groups.entry(category).or_default().push(item.clone());
            }
            groups
        };
        let card_groups = {
            let mut groups = BTreeMap::<String, Vec<FlashcardSummary>>::new();
            for card in cards
                .iter()
                .flatten()
                .filter(|card| card.source_question_id.is_some())
            {
                groups
                    .entry(shenlun_card_category(&card.deck_path))
                    .or_default()
                    .push(card.clone());
            }
            groups
        };
        let favorite_total = favorites
            .as_ref()
            .map(|items| items.len().to_string())
            .unwrap_or_else(|| "…".into());
        let card_total = cards
            .as_ref()
            .map(|cards| {
                cards
                    .iter()
                    .filter(|card| card.source_question_id.is_some())
                    .count()
                    .to_string()
            })
            .unwrap_or_else(|| "…".into());
        html! {
            <TrainingSetupLayout test_id="shenlun-catalog" wide={true}
                action={collection_kind.is_none().then_some(start_action)} summary="申论复盘" detail={action_detail}>
                <div class="question-bank-setup-content space-y-5">
                    <div class="question-bank-heading">
                        <h2 class="text-2xl font-bold">{"申论训练"}</h2>
                        <p class="mt-1 text-sm leading-6 text-base-content/60">
                            {"选择申论原卷，按题目进入批改、解析、OCR 与参考答案复盘。"}
                        </p>
                    </div>
                    if let Some(message)=(*error).as_ref() {
                        <p role="alert" class="text-sm text-error">{message.clone()}</p>
                    }
                    <div class="flex flex-wrap gap-2" role="group" aria-label="申论题目浏览分类"
                        data-testid="shenlun-collections">
                        <button type="button"
                            class={classes!("btn","btn-sm",if collection_kind.is_none() {"btn-primary"} else {"btn-outline"})}
                            data-testid="shenlun-collection-all"
                            aria-pressed={collection_kind.is_none().to_string()}
                            onclick={Callback::from({let collection_kind=collection_kind.clone();move |_|collection_kind.set(None)})}>
                            {"全部题目"}
                        </button>
                        <button type="button"
                            class={classes!("btn","btn-sm",if *collection_kind == Some(ShenlunCollectionKind::Favorites) {"btn-primary"} else {"btn-outline"})}
                            data-testid="shenlun-collection-favorites"
                            aria-pressed={(*collection_kind == Some(ShenlunCollectionKind::Favorites)).to_string()}
                            onclick={Callback::from({let choose=choose_collection.clone();move |_|choose.emit(ShenlunCollectionKind::Favorites)})}>
                            {"我的收藏"}
                            <span class="question-bank-count" data-testid="shenlun-collection-favorites-count">{favorite_total}</span>
                        </button>
                        <button type="button"
                            class={classes!("btn","btn-sm",if *collection_kind == Some(ShenlunCollectionKind::Cards) {"btn-primary"} else {"btn-outline"})}
                            data-testid="shenlun-collection-cards"
                            aria-pressed={(*collection_kind == Some(ShenlunCollectionKind::Cards)).to_string()}
                            onclick={Callback::from({let choose=choose_collection.clone();move |_|choose.emit(ShenlunCollectionKind::Cards)})}>
                            {"我的卡片"}
                            <span class="question-bank-count" data-testid="shenlun-collection-cards-count">{card_total}</span>
                        </button>
                    </div>
                    if collection_kind.is_none() {
                        <section class="space-y-3" aria-label="选择申论原卷">
                            <div class="flex items-end justify-between gap-3">
                                <div>
                                    <h3 class="font-semibold">{"选择原卷"}</h3>
                                    <p class="text-xs text-base-content/60">{"按地区展开年份与原卷；已选原卷高亮显示。"}</p>
                                </div>
                                <span class="text-xs text-base-content/50" data-testid="question-bank-paper-total">
                                    {paper_catalog_label(&available_papers)}
                                </span>
                            </div>
                            if *loading {
                                <p class="rounded-box bg-base-200 p-4 text-sm" role="status" data-testid="question-bank-paper-loading">
                                    {"正在加载申论原卷…"}
                                </p>
                            } else if available_papers.is_empty() {
                                <p class="rounded-box bg-base-200 p-4 text-sm">
                                    {"暂无可用申论原卷，请先在题库管理同步原卷。"}
                                </p>
                            } else {
                                <QuestionBankPaperPicker papers={available_papers}
                                    selected_id={(*selected_paper).clone()} on_select={select_paper}/>
                            }
                        </section>
                    }
                    if *collection_kind == Some(ShenlunCollectionKind::Favorites) {
                        <section class="space-y-3" aria-label="申论收藏" data-testid="shenlun-collections-favorites-panel">
                            if let Some(message)=(*favorites_error).as_ref() {
                                <div class="space-y-2" role="alert" data-testid="shenlun-collection-favorites-error">
                                    <p class="text-sm text-error">{message.clone()}</p>
                                    <button type="button" class="btn btn-outline btn-sm" data-testid="shenlun-collection-favorites-retry" onclick={retry_favorites}>{"重试加载"}</button>
                                </div>
                            } else if favorites.is_none() {
                                <p class="rounded-box bg-base-200 p-4 text-sm" role="status" data-testid="shenlun-collection-favorites-loading">{"正在加载申论收藏…"}</p>
                            } else if favorite_groups.is_empty() {
                                <p class="rounded-box bg-base-200 p-4 text-sm" data-testid="shenlun-collection-favorites-empty">{"还没有收藏的申论题目。"}</p>
                            } else {
                                <div class="space-y-4" data-testid="shenlun-collection-favorites-list">
                                    {for favorite_groups.iter().map(|(category, items)| {
                                        html! {
                                            <div class="space-y-2">
                                                <div class="flex items-center justify-between gap-2">
                                                    <h4 class="text-sm font-semibold">{category.clone()}</h4>
                                                    <span class="text-xs text-base-content/50">{format!("{} 题", items.len())}</span>
                                                </div>
                                                <div class="question-bank-results-list">
                                                    {for items.iter().map(|item| {
                                                        let on_open = {
                                                            let open=open_favorite.clone();
                                                            let paper_id=item.paper_id.clone();
                                                            let question_id=item.question_id.clone();
                                                            Callback::from(move |_|open.emit((paper_id.clone(),question_id.clone())))
                                                        };
                                                        html! {
                                                            <button type="button" class="question-bank-result-row w-full text-left"
                                                                data-testid="shenlun-collection-item" title={item.paper_title.clone()}
                                                                onclick={on_open}>
                                                                <div class="min-w-0">
                                                                    <strong class="block truncate text-sm">{format!("{} {} · 第 {} 题",item.year,item.region,item.number)}</strong>
                                                                    <p class="mt-0.5 truncate text-xs text-base-content/50">{item.paper_title.clone()}</p>
                                                                </div>
                                                                <span class="question-bank-result-year">{item.year}</span>
                                                            </button>
                                                        }
                                                    })}
                                                </div>
                                            </div>
                                        }
                                    })}
                                </div>
                            }
                        </section>
                    } else if *collection_kind == Some(ShenlunCollectionKind::Cards) {
                        <section class="space-y-3" aria-label="申论卡片" data-testid="shenlun-collections-cards-panel">
                            if let Some(message)=(*cards_error).as_ref() {
                                <div class="space-y-2" role="alert" data-testid="shenlun-collection-cards-error">
                                    <p class="text-sm text-error">{message.clone()}</p>
                                    <button type="button" class="btn btn-outline btn-sm" data-testid="shenlun-collection-cards-retry" onclick={retry_cards}>{"重试加载"}</button>
                                </div>
                            } else if cards.is_none() {
                                <p class="rounded-box bg-base-200 p-4 text-sm" role="status" data-testid="shenlun-collection-cards-loading">{"正在加载申论卡片…"}</p>
                            } else if card_groups.is_empty() {
                                <p class="rounded-box bg-base-200 p-4 text-sm" data-testid="shenlun-collection-cards-empty">{"还没有可打开的申论卡片。"}</p>
                            } else {
                                <div class="space-y-4" data-testid="shenlun-collection-cards-list">
                                    {for card_groups.iter().map(|(category, items)| {
                                        html! {
                                            <div class="space-y-2">
                                                <div class="flex items-center justify-between gap-2">
                                                    <h4 class="text-sm font-semibold">{category.clone()}</h4>
                                                    <span class="text-xs text-base-content/50">{format!("{} 张", items.len())}</span>
                                                </div>
                                                <div class="question-bank-results-list">
                                                    {for items.iter().map(|card| {
                                                        let card_id=card.id.to_string();
                                                        let question_id=card.source_question_id.clone();
                                                        let disabled=question_id.is_none() || *collection_busy;
                                                        let on_open={
                                                            let open=open_card.clone();
                                                            let card_id=card_id.clone();
                                                            Callback::from(move |_|{
                                                                if let Some(question_id)=question_id.clone() {
                                                                    open.emit((card_id.clone(),question_id));
                                                                }
                                                            })
                                                        };
                                                        html! {
                                                            <button type="button" class="question-bank-result-row w-full text-left"
                                                                data-testid="shenlun-collection-card"
                                                                disabled={disabled} onclick={on_open}>
                                                                <div class="min-w-0">
                                                                    <strong class="block truncate text-sm">{flashcard_list_title(&card.front_markdown)}</strong>
                                                                    <p class="mt-0.5 truncate text-xs text-base-content/50">{format!("申论 / {category}")}</p>
                                                                </div>
                                                                <span class="question-bank-result-year">
                                                                    {if card.deck_path.starts_with("真题练习/") {"真题"} else {"整理"}}
                                                                </span>
                                                            </button>
                                                        }
                                                    })}
                                                </div>
                                            </div>
                                        }
                                    })}
                                </div>
                            }
                        </section>
                    }
                </div>
            </TrainingSetupLayout>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_label_does_not_guess_from_question_number() {
        let item = QuestionView {
            id: "test".into(),
            number: 5,
            stem: "请概括".into(),
            options_markdown: String::new(),
            materials: vec![],
            module: None,
            category: None,
            topics: vec![],
            answer_type: "subjective".into(),
            question_type: Some("other".into()),
            grading_mode: "ai".into(),
            max_score: None,
        };
        assert_eq!(topic_label(&item), "其他");
    }
}
