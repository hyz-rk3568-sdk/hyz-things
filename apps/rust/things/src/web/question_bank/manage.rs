use super::*;
use hyz_things::domain::question_bank::{BankSyncStatus, BankSyncSummary, PaperSummary, PaperView};

#[hook]
pub(super) fn use_question_bank_foreground(enabled: bool) -> u64 {
    let wake = use_state(|| 0_u64);
    let generation = use_mut_ref(|| 0_u64);
    {
        let wake = wake.clone();
        use_effect_with(enabled, move |enabled| {
            let window = web_sys::window().expect("browser window");
            let document = window.document().expect("browser document");
            let pending = Rc::new(RefCell::new(None::<gloo_timers::callback::Timeout>));
            let scheduled = pending.clone();
            let visibility = document.clone();
            let refresh = Closure::<dyn FnMut(Event)>::new(move |_| {
                if visibility.hidden() || scheduled.borrow().is_some() {
                    return;
                }
                let pending = scheduled.clone();
                let generation = generation.clone();
                let wake = wake.clone();
                *scheduled.borrow_mut() = Some(gloo_timers::callback::Timeout::new(0, move || {
                    pending.borrow_mut().take();
                    let next = generation.borrow().wrapping_add(1);
                    *generation.borrow_mut() = next;
                    wake.set(next);
                }));
            });
            if *enabled {
                let _ = document.add_event_listener_with_callback(
                    "visibilitychange",
                    refresh.as_ref().unchecked_ref(),
                );
                for event in ["pageshow", "focus", "online"] {
                    let _ = window
                        .add_event_listener_with_callback(event, refresh.as_ref().unchecked_ref());
                }
            }
            move || {
                pending.borrow_mut().take();
                let _ = document.remove_event_listener_with_callback(
                    "visibilitychange",
                    refresh.as_ref().unchecked_ref(),
                );
                for event in ["pageshow", "focus", "online"] {
                    let _ = window.remove_event_listener_with_callback(
                        event,
                        refresh.as_ref().unchecked_ref(),
                    );
                }
            }
        });
    }
    *wake
}

struct BankRequestScope {
    mounted: bool,
    identity: (bool, u64),
    paper_generation: u64,
}

#[derive(PartialEq)]
enum BankSyncCompletion {
    Task(String),
    Summary(BankSyncSummary),
}

struct BankRequestGuard {
    scope: Rc<RefCell<BankRequestScope>>,
    identity: (bool, u64),
    cancelled: Rc<Cell<bool>>,
}

impl BankRequestGuard {
    fn new(scope: Rc<RefCell<BankRequestScope>>, identity: (bool, u64)) -> Self {
        Self {
            scope,
            identity,
            cancelled: Rc::new(Cell::new(false)),
        }
    }

    fn is_current(&self) -> bool {
        let scope = self.scope.borrow();
        !self.cancelled.get() && scope.mounted && self.identity.0 && scope.identity == self.identity
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct QuestionBankPanelProps {
    pub state: UseReducerHandle<AppState>,
    pub is_admin: bool,
}

#[function_component(QuestionBankPanel)]
pub(crate) fn question_bank_panel(props: &QuestionBankPanelProps) -> Html {
    let papers = use_state(|| None::<Vec<PaperSummary>>);
    let paper = use_state(|| None::<PaperView>);
    let notice = use_state(|| None::<String>);
    let busy = use_state(|| false);
    let syncing = use_state(|| false);
    let sync_poll = use_state(|| 0u64);
    let reload = use_state(|| 0u64);
    let reload_generation = use_mut_ref(|| 0u64);
    let request_reload = {
        let reload = reload.clone();
        Callback::from(move |()| {
            let next = reload_generation.borrow().wrapping_add(1);
            *reload_generation.borrow_mut() = next;
            reload.set(next);
        })
    };
    let completed_sync = use_mut_ref(|| None::<BankSyncCompletion>);
    let loading = use_state(|| true);
    let load_error = use_state(|| None::<String>);
    let identity = (props.is_admin, props.state.auth_epoch);
    let private_identity = use_state(|| identity);
    let requests = use_mut_ref(|| BankRequestScope {
        mounted: true,
        identity,
        paper_generation: 0,
    });
    requests.borrow_mut().identity = identity;
    let wake = use_question_bank_foreground(props.is_admin);
    {
        let requests = requests.clone();
        use_effect_with((), move |_| move || requests.borrow_mut().mounted = false);
    }
    {
        let papers = papers.clone();
        let paper = paper.clone();
        let notice = notice.clone();
        let busy = busy.clone();
        let syncing = syncing.clone();
        let loading = loading.clone();
        let load_error = load_error.clone();
        let private_identity = private_identity.clone();
        let completed_sync = completed_sync.clone();
        use_effect_with(identity, move |identity| {
            papers.set(None);
            paper.set(None);
            notice.set(None);
            busy.set(false);
            syncing.set(false);
            loading.set(identity.0);
            load_error.set(None);
            private_identity.set(*identity);
            completed_sync.borrow_mut().take();
            || ()
        });
    }
    {
        let papers = papers.clone();
        let loading = loading.clone();
        let load_error = load_error.clone();
        let requests = requests.clone();
        use_effect_with((identity, *reload, wake), move |(identity, _, _)| {
            let guard = BankRequestGuard::new(requests, *identity);
            let cancel = guard.cancelled.clone();
            if identity.0 {
                loading.set(true);
                load_error.set(None);
                spawn_local(async move {
                    let result =
                        fetch_json::<Vec<PaperSummary>>("/api/v1/question-bank/papers", "题库列表")
                            .await;
                    if !guard.is_current() {
                        return;
                    }
                    match result {
                        Ok(list) => papers.set(Some(list)),
                        Err(error) => {
                            load_error.set(Some(format!("题库加载失败，请重新加载题库。{error}")))
                        }
                    }
                    loading.set(false);
                });
            }
            move || cancel.set(true)
        });
    }
    {
        let current = (*paper).clone();
        use_effect_with(current, move |_| {
            hydrate_math();
            || ()
        });
    }
    // The job belongs to the server, not the page; refreshing resumes polling.
    {
        let notice = notice.clone();
        let syncing = syncing.clone();
        let request_reload = request_reload.clone();
        let requests = requests.clone();
        let completed_sync = completed_sync.clone();
        use_effect_with((identity, *sync_poll, wake), move |(identity, _, _)| {
            let guard = BankRequestGuard::new(requests, *identity);
            let cancel = guard.cancelled.clone();
            if identity.0 {
                spawn_local(async move {
                    while guard.is_current() {
                        let status = fetch_json::<BankSyncStatus>(
                            "/api/v1/question-bank/sync/status",
                            "题库同步状态",
                        )
                        .await;
                        if !guard.is_current() {
                            break;
                        }
                        match status {
                            Ok(status) => {
                                syncing.set(status.running);
                                if status.running {
                                    notice.set(Some(format!("正在同步题库（{}）", status.stage)));
                                } else {
                                    match (status.stage.as_str(), status.summary) {
                                        ("completed", Some(summary)) => {
                                            notice.set(Some(format!(
                                                "已同步 {} 份文件、{} 个题位、{} 份材料",
                                                summary.papers,
                                                summary.questions,
                                                summary.materials,
                                            )));
                                            let completion = status
                                                .task_id
                                                .map(BankSyncCompletion::Task)
                                                .unwrap_or_else(|| {
                                                    BankSyncCompletion::Summary(summary.clone())
                                                });
                                            if completed_sync.borrow().as_ref() != Some(&completion)
                                            {
                                                *completed_sync.borrow_mut() = Some(completion);
                                                request_reload.emit(());
                                            }
                                        }
                                        ("failed", _) => notice.set(Some(format!(
                                            "题库同步失败（{}，阶段：{}），可检查源文件后重试",
                                            status.error_code.unwrap_or_else(|| "unknown".into()),
                                            status.stage,
                                        ))),
                                        _ => {}
                                    }
                                    break;
                                }
                            }
                            Err(error) => {
                                syncing.set(false);
                                notice.set(Some(format!(
                                    "题库同步状态读取失败，返回前台后可重试：{error}"
                                )));
                                break;
                            }
                        }
                        TimeoutFuture::new(900).await;
                    }
                });
            } else {
                syncing.set(false);
            }
            move || cancel.set(true)
        });
    }
    let csrf = props
        .state
        .panel
        .as_ref()
        .map(|panel| panel.csrf_token.clone())
        .unwrap_or_default();
    let on_sync = {
        let busy = busy.clone();
        let syncing = syncing.clone();
        let notice = notice.clone();
        let sync_poll = sync_poll.clone();
        let csrf = csrf.clone();
        let requests = requests.clone();
        Callback::from(move |_| {
            let guard = BankRequestGuard::new(requests.clone(), identity);
            if !guard.is_current() || *busy || *syncing {
                return;
            }
            busy.set(true);
            notice.set(None);
            let busy = busy.clone();
            let notice = notice.clone();
            let sync_poll = sync_poll.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let result: Result<BankSyncStatus, String> = post_json_response(
                    "/api/v1/control/question-bank/sync",
                    &csrf,
                    &EmptyRequest {},
                    "同步题库",
                )
                .await;
                if !guard.is_current() {
                    return;
                }
                match result {
                    Ok(_) => sync_poll.set(*sync_poll + 1),
                    Err(error) => notice.set(Some(error)),
                }
                busy.set(false);
            });
        })
    };
    let on_open = {
        let busy = busy.clone();
        let syncing = syncing.clone();
        let notice = notice.clone();
        let paper = paper.clone();
        let requests = requests.clone();
        Callback::from(move |id: String| {
            let guard = BankRequestGuard::new(requests.clone(), identity);
            if !guard.is_current() || *busy || *syncing {
                return;
            }
            let generation = {
                let mut scope = requests.borrow_mut();
                scope.paper_generation = scope.paper_generation.wrapping_add(1);
                scope.paper_generation
            };
            busy.set(true);
            notice.set(None);
            let busy = busy.clone();
            let notice = notice.clone();
            let paper = paper.clone();
            spawn_local(async move {
                let result = fetch_json::<PaperView>(
                    &format!("/api/v1/question-bank/papers/{id}"),
                    "整卷加载",
                )
                .await;
                if !guard.is_current() || guard.scope.borrow().paper_generation != generation {
                    return;
                }
                match result {
                    Ok(value) => paper.set(Some(value)),
                    Err(error) => notice.set(Some(error)),
                }
                busy.set(false);
            });
        })
    };
    let on_retry = {
        let requests = requests.clone();
        Callback::from(move |_| {
            if BankRequestGuard::new(requests.clone(), identity).is_current() {
                request_reload.emit(());
            }
        })
    };
    html! {
        <SectionCard title_id="question-bank-title" busy={Some(*busy || *syncing || *loading)}>
            <div class={SECTION_HEAD}>
                <div><h3 id="question-bank-title" class={SECTION_TITLE}>{"行测题库"}</h3><p class={SECTION_META}>{"历年真题 · 整卷阅读"}</p></div>
                if props.is_admin {
                    <button class={BUTTON_PRIMARY} type="button" data-testid="question-bank-sync" disabled={*busy || *syncing || csrf.is_empty()} onclick={on_sync}>{if *syncing {"同步中…"} else if *busy {"处理中…"} else {"同步题库"}}</button>
                }
            </div>
            if !props.is_admin {
                <crate::app::AdminAuthGate state={props.state.clone()} />
            } else if *private_identity != identity {
                <p class={HELP_TEXT} role="status" data-testid="question-bank-loading">{"正在加载题库…"}</p>
            } else {
                if let Some(error)=&*load_error {
                    <div class={FEEDBACK} role="alert" data-testid="question-bank-load-error">
                        <p>{error.clone()}</p>
                        <button class={BUTTON} type="button" onclick={on_retry}>{"重新加载题库"}</button>
                    </div>
                }
                if *loading {
                    <p class={HELP_TEXT} role="status" data-testid="question-bank-loading">{"正在加载题库…"}</p>
                }
                if let Some(message)=&*notice {<div class={FEEDBACK} role="status" data-testid="question-bank-notice">{message.clone()}</div>}
                if let Some(current)=&*paper {
                    <button class={BUTTON} type="button" onclick={Callback::from({let paper=paper.clone();move |_|paper.set(None)})}>{"返回试卷列表"}</button>
                    {render_paper(current)}
                } else {
                    <div class="max-h-96 overflow-y-auto space-y-2" data-testid="question-bank-papers">
                        if !*loading && load_error.is_none() && papers.as_ref().is_some_and(Vec::is_empty) {<p class={HELP_TEXT}>{"题库为空。首次使用请先同步题库。"}</p>}
                        {for papers.iter().flatten().map(|entry| {
                            let open=on_open.clone();let id=entry.id.clone();
                            html! {<button class="btn btn-outline h-auto min-h-12 w-full justify-between py-3 text-left" type="button" data-testid="question-bank-open" disabled={*busy || *syncing} onclick={Callback::from(move |_|open.emit(id.clone()))}>
                                <span>{entry.title.clone()}</span><span class="text-xs opacity-60">{format!("{} · {} · {} 个题位",entry.year,entry.region,entry.questions)}</span>
                            </button>}
                        })}
                    </div>
                }
            }
        </SectionCard>
    }
}

fn render_paper(paper: &PaperView) -> Html {
    let mut shown = std::collections::BTreeSet::new();
    html! {<div class="space-y-6" data-testid="question-bank-paper">
        <h4 class={SECTION_TITLE}>{paper.title.clone()}</h4>
        <p class={HELP_TEXT}>{"保留来源题号；本页只加载题面和材料。"}</p>
        {for paper.questions.iter().map(|question| {
            let materials=question.materials.iter().filter(|id|shown.insert((*id).clone())).filter_map(|id|paper.materials.iter().find(|m|&m.id==id)).collect::<Vec<_>>();
            html! {<article class="space-y-3 border-t border-base-content/10 pt-5" data-testid="question-bank-question">
                {for materials.iter().map(|material|html!{<section class="rounded-box bg-base-200 p-4" data-testid="question-bank-material"><h5 class="font-bold">{format!("材料 {}",material.name)}</h5><div class="study-markdown mt-3">{render_question_bank_markdown(&material.markdown)}</div></section>})}
                <h5 class="font-bold">{format!("原卷第 {} 题",question.number)}</h5>
                if !question.materials.is_empty() {<p class={HELP_TEXT}>{"引用材料见本题组前文。"}</p>}
                <div class="study-markdown">{render_question_bank_markdown(&question.stem)}</div>
                <div class="study-markdown">{render_question_bank_markdown(&question.options_markdown)}</div>
            </article>}
        })}
    </div>}
}
