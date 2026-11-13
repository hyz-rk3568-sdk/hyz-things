//! 申论批改：与行测客观题并列的真题选择、材料阅读入口。
//! 不展示来源参考答案；草稿与交卷使用申论独立 API，评分尚待接入。
use super::*;
use hyz_things::domain::question_bank::{
    CreateShenlunAttempt, PaperSummary, PaperView, QuestionView, SaveShenlunDraft, ShenlunAttempt,
};

#[derive(Properties, PartialEq)]
pub(super) struct ShenlunTrainingProps {
    pub(super) on_immersive_change: Callback<bool>,
    pub(super) csrf: String,
}

fn topic_label(question: &QuestionView) -> &str {
    match question.question_type.as_deref() {
        Some("summary" | "generalization" | "归纳概括") => "归纳概括",
        Some("analysis" | "综合分析") => "综合分析",
        Some("countermeasure" | "提出对策") => "提出对策",
        Some("document" | "official_document" | "公文写作") => "公文写作",
        Some("essay" | "composition" | "大作文") => "大作文",
        _ => "其他",
    }
}

pub(super) fn new_shenlun_attempt_id() -> Result<String, String> {
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
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11],
        bytes[12], bytes[13], bytes[14], bytes[15],
    ))
}

#[function_component(ShenlunTraining)]
pub(super) fn shenlun_training(props: &ShenlunTrainingProps) -> Html {
    let papers = use_state(|| None::<Vec<PaperSummary>>);
    let paper = use_state(|| None::<PaperView>);
    let selected = use_state(|| 0_usize);
    let region = use_state(|| "全部".to_string());
    let year = use_state(String::new);
    let busy = use_state(|| false);
    let loading = use_state(|| true);
    let error = use_state(|| None::<String>);
    let refresh = use_state(|| 0_u32);
    let attempt = use_state(|| None::<ShenlunAttempt>);
    let attempt_history = use_state(Vec::<ShenlunAttempt>::new);
    let draft = use_state(String::new);
    let attempt_loading = use_state(|| false);
    let attempt_busy = use_state(|| false);
    let saving = use_state(|| false);
    let draft_error = use_state(|| None::<String>);
    let question_key = (*paper).as_ref().and_then(|p| {
        p.questions
            .get(*selected)
            .map(|question| (p.id.clone(), question.id.clone()))
    });
    {
        let attempt = attempt.clone();
        let attempt_history = attempt_history.clone();
        let draft = draft.clone();
        let attempt_loading = attempt_loading.clone();
        let draft_error = draft_error.clone();
        use_effect_with(question_key.clone(), move |key| {
            let cancelled = Rc::new(Cell::new(false));
            attempt.set(None);
            attempt_history.set(Vec::new());
            draft.set(String::new());
            draft_error.set(None);
            attempt_loading.set(key.is_some());
            if let Some((paper_id, question_id)) = key.clone() {
                let cancelled = cancelled.clone();
                spawn_local(async move {
                    let url = format!(
                        "/api/v1/question-bank/shenlun/attempts?paper_id={paper_id}&question_id={question_id}"
                    );
                    let result = fetch_json::<Vec<ShenlunAttempt>>(&url, "申论作答历史").await;
                    if cancelled.get() {
                        return;
                    }
                    match result {
                        Ok(items) => {
                            let active = items
                                .iter()
                                .find(|item| item.status == "draft")
                                .or_else(|| items.first())
                                .cloned();
                            if let Some(record) = active.as_ref() {
                                draft.set(record.text.clone());
                            }
                            attempt.set(active);
                            attempt_history.set(items);
                        }
                        Err(error) => draft_error.set(Some(error)),
                    }
                    attempt_loading.set(false);
                });
            }
            move || cancelled.set(true)
        });
    }
    {
        let snapshot = (*attempt).clone();
        let text = (*draft).clone();
        let blocked = *saving || (*draft_error).is_some();
        let csrf = props.csrf.clone();
        let attempt = attempt.clone();
        let saving = saving.clone();
        let draft_error = draft_error.clone();
        use_effect_with((snapshot.clone(), text.clone(), blocked), move |_| {
            let pending = if let Some(record) = snapshot
                .filter(|record| record.status == "draft" && record.text != text && !blocked)
            {
                Some(gloo_timers::callback::Timeout::new(900, move || {
                    saving.set(true);
                    let attempt = attempt.clone();
                    let saving = saving.clone();
                    let draft_error = draft_error.clone();
                    spawn_local(async move {
                        let result: Result<ShenlunAttempt, String> = post_json_response(
                            &format!("/api/v1/question-bank/shenlun/attempts/{}/draft", record.id),
                            &csrf,
                            &SaveShenlunDraft {
                                expected_revision: record.revision,
                                text,
                            },
                            "自动保存申论草稿",
                        )
                        .await;
                        match result {
                            Ok(saved) => attempt.set(Some(saved)),
                            Err(message) => draft_error
                                .set(Some(format!("草稿未保存，请修改或重试：{message}"))),
                        }
                        saving.set(false);
                    });
                }))
            } else {
                None
            };
            move || drop(pending)
        });
    }
    {
        let papers = papers.clone();
        let loading = loading.clone();
        let error = error.clone();
        use_effect_with(*refresh, move |_| {
            let cancelled = Rc::new(Cell::new(false));
            let guard = cancelled.clone();
            loading.set(true);
            error.set(None);
            spawn_local(async move {
                let result = fetch_json::<Vec<PaperSummary>>(
                    "/api/v1/question-bank/shenlun/papers",
                    "申论真题列表",
                )
                .await;
                if guard.get() {
                    return;
                }
                match result {
                    Ok(value) => papers.set(Some(value)),
                    Err(message) => error.set(Some(message)),
                }
                loading.set(false);
            });
            move || cancelled.set(true)
        });
    }
    let retry = {
        let refresh = refresh.clone();
        Callback::from(move |_| refresh.set(refresh.wrapping_add(1)))
    };
    let open = {
        let busy = busy.clone();
        let paper = paper.clone();
        let selected = selected.clone();
        let error = error.clone();
        let immersive = props.on_immersive_change.clone();
        Callback::from(move |id: String| {
            if *busy {
                return;
            }
            busy.set(true);
            error.set(None);
            let busy = busy.clone();
            let paper = paper.clone();
            let selected = selected.clone();
            let error = error.clone();
            let immersive = immersive.clone();
            spawn_local(async move {
                match fetch_json::<PaperView>(
                    &format!("/api/v1/question-bank/shenlun/papers/{id}"),
                    "申论真题",
                )
                .await
                {
                    Ok(value) => {
                        selected.set(0);
                        paper.set(Some(value));
                        immersive.emit(true);
                    }
                    Err(message) => error.set(Some(message)),
                }
                busy.set(false);
            });
        })
    };
    let close = {
        let paper = paper.clone();
        let immersive = props.on_immersive_change.clone();
        Callback::from(move |_| {
            immersive.emit(false);
            paper.set(None);
        })
    };
    let set_draft = {
        let draft = draft.clone();
        let draft_error = draft_error.clone();
        Callback::from(move |event: InputEvent| {
            let input: web_sys::HtmlTextAreaElement = event.target_unchecked_into();
            draft_error.set(None);
            draft.set(input.value());
        })
    };
    let new_attempt = {
        let key = question_key.clone();
        let csrf = props.csrf.clone();
        let attempt_busy = attempt_busy.clone();
        let attempt = attempt.clone();
        let draft = draft.clone();
        let draft_error = draft_error.clone();
        let attempt_history = attempt_history.clone();
        Callback::from(move |_| {
            let Some((paper_id, question_id)) = key.clone() else {
                return;
            };
            if *attempt_busy {
                return;
            }
            let request_id = match new_shenlun_attempt_id() {
                Ok(id) => id,
                Err(error) => {
                    draft_error.set(Some(error));
                    return;
                }
            };
            attempt_busy.set(true);
            let attempt_busy = attempt_busy.clone();
            let attempt = attempt.clone();
            let draft = draft.clone();
            let draft_error = draft_error.clone();
            let attempt_history = attempt_history.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let result: Result<ShenlunAttempt, String> = post_json_response(
                    "/api/v1/question-bank/shenlun/attempts",
                    &csrf,
                    &CreateShenlunAttempt {
                        request_id,
                        paper_id,
                        question_id,
                    },
                    "开始申论作答",
                )
                .await;
                match result {
                    Ok(record) => {
                        draft.set(record.text.clone());
                        attempt.set(Some(record.clone()));
                        let mut items = (*attempt_history).clone();
                        items.insert(0, record);
                        attempt_history.set(items);
                        draft_error.set(None);
                    }
                    Err(message) => draft_error.set(Some(message)),
                }
                attempt_busy.set(false);
            });
        })
    };
    let submit = {
        let attempt = attempt.clone();
        let draft_error = draft_error.clone();
        let attempt_busy = attempt_busy.clone();
        let attempt_history = attempt_history.clone();
        let csrf = props.csrf.clone();
        Callback::from(move |_| {
            let Some(record) = (*attempt).clone() else {
                return;
            };
            if record.status != "draft" || record.text.trim().is_empty() || *attempt_busy {
                return;
            }
            attempt_busy.set(true);
            let attempt = attempt.clone();
            let draft_error = draft_error.clone();
            let attempt_busy = attempt_busy.clone();
            let attempt_history = attempt_history.clone();
            let csrf = csrf.clone();
            spawn_local(async move {
                let result: Result<ShenlunAttempt, String> = post_json_response(
                    &format!(
                        "/api/v1/question-bank/shenlun/attempts/{}/submit",
                        record.id
                    ),
                    &csrf,
                    &EmptyRequest {},
                    "提交申论作答",
                )
                .await;
                match result {
                    Ok(submitted) => {
                        let mut history = (*attempt_history).clone();
                        if let Some(row) = history.iter_mut().find(|row| row.id == submitted.id) {
                            *row = submitted.clone();
                        }
                        attempt_history.set(history);
                        attempt.set(Some(submitted));
                        draft_error.set(None);
                    }
                    Err(message) => draft_error.set(Some(message)),
                }
                attempt_busy.set(false);
            });
        })
    };
    let select_attempt = {
        let attempt = attempt.clone();
        let draft = draft.clone();
        Callback::from(move |record: ShenlunAttempt| {
            draft.set(record.text.clone());
            attempt.set(Some(record));
        })
    };
    let set_year = {
        let year = year.clone();
        Callback::from(move |event: InputEvent| {
            let input: web_sys::HtmlInputElement = event.target_unchecked_into();
            year.set(input.value());
        })
    };
    let all_regions = {
        let region = region.clone();
        Callback::from(move |_| region.set("全部".into()))
    };
    let national = {
        let region = region.clone();
        Callback::from(move |_| region.set("国考".into()))
    };
    let hunan = {
        let region = region.clone();
        Callback::from(move |_| region.set("湖南".into()))
    };

    let unsaved = (*attempt)
        .as_ref()
        .is_some_and(|record| record.status == "draft" && record.text != *draft);
    let navigation_locked = *saving || *attempt_busy || unsaved;
    if let Some(current) = (*paper).as_ref() {
        let index = (*selected).min(current.questions.len().saturating_sub(1));
        let question = current.questions.get(index);
        let previous = {
            let selected = selected.clone();
            Callback::from(move |_| selected.set(index.saturating_sub(1)))
        };
        let next = {
            let selected = selected.clone();
            let last = current.questions.len().saturating_sub(1);
            Callback::from(move |_| selected.set((index + 1).min(last)))
        };
        let left = html! {
            <section class="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-base-300 bg-base-100" data-testid="shenlun-materials">
                <header class={TRAINING_PANE_HEADER}>
                    <h2 class="font-semibold">{"给定材料"}</h2>
                    <span class="text-xs opacity-70">{format!("共 {} 份", current.materials.len())}</span>
                </header>
                <div class="min-h-0 flex-1 overflow-y-auto px-4 pb-5" data-swipe-ignore="true">
                    {for current.materials.iter().map(|material| html! {
                        <article class="mb-6" key={material.id.clone()}>
                            <h3 class="mb-2 font-semibold">{material.name.clone()}</h3>
                            <div class="study-markdown">{render_markdown(&material.markdown, &current.id)}</div>
                        </article>
                    })}
                </div>
            </section>
        };
        let right = html! {
            <section class="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-base-300 bg-base-100" data-testid="shenlun-question">
                <header class={TRAINING_PANE_HEADER}>
                    <h2 class="font-semibold">{"作答题目"}</h2>
                    <span class="text-xs opacity-70">{format!("第 {} / {} 题", index + 1, current.questions.len())}</span>
                </header>
                <div class="flex flex-wrap gap-1 px-3 pb-2" aria-label="题号导航">
                    {for current.questions.iter().enumerate().map(|(number, item)| {
                        let selected = selected.clone();
                        html! {
                            <button type="button" key={item.id.clone()} class={classes!("btn","btn-xs", if number == index {"btn-primary"} else {"btn-outline"})}
                                aria-current={(number == index).then_some("step")}
                                disabled={navigation_locked}
                                data-testid="shenlun-question-nav"
                                onclick={Callback::from(move |_| selected.set(number))}>{item.number}</button>
                        }
                    })}
                </div>
                <div class="min-h-0 flex-1 overflow-y-auto px-4 pb-5" data-swipe-ignore="true">
                    if let Some(question) = question {
                        <p class="mb-2 text-xs opacity-70">
                            {topic_label(question)}
                            {" · "}
                            {question.max_score.map(|s| format!("{s} 分")).unwrap_or_else(|| "分值待确认".into())}
                        </p>
                        <div class="study-markdown">{render_markdown(&question.stem, &current.id)}</div>
                        <div class="mt-5 space-y-3" data-testid="shenlun-answer">
                            if *attempt_loading {
                                <p class="text-sm opacity-70">{"正在恢复历史草稿…"}</p>
                            } else if let Some(record) = (*attempt).as_ref() {
                                <div class="flex flex-wrap items-center justify-between gap-2">
                                    if record.after_reference {
                                        <span class="text-xs text-warning">{"参考后修改（非独立二稿）"}</span>
                                    }
                                    <span class="text-xs opacity-70">
                                        {if record.status == "submitted" {"已交卷 · 原稿不可修改"}
                                         else if *saving {"自动保存中…"}
                                         else if unsaved {"待自动保存"}
                                         else {"草稿已保存"}}
                                    </span>
                                    <button type="button" class="btn btn-outline btn-xs" onclick={new_attempt.clone()}
                                        disabled={navigation_locked}>{"重做新稿"}</button>
                                </div>
                                <textarea class="textarea textarea-bordered min-h-48 w-full leading-relaxed" rows="12"
                                    data-testid="shenlun-answer-input" aria-label="申论文字作答"
                                    placeholder="在此输入申论答案（最多 50000 字）"
                                    disabled={record.status != "draft" || *attempt_busy}
                                    value={(*draft).clone()} oninput={set_draft.clone()} />
                                <div class="flex items-center justify-between gap-2">
                                    <span class="text-xs opacity-70">{format!("{} 字", draft.chars().count())}</span>
                                    if record.status == "draft" {
                                        <button type="button" class="btn btn-primary btn-sm"
                                            data-testid="shenlun-submit" onclick={submit.clone()}
                                            disabled={navigation_locked || draft.trim().is_empty() || !draft_error.is_none()}>
                                            {"确认提交"}
                                        </button>
                                    } else {
                                        <span class="text-xs opacity-70">{"已保存正式作答，可开始 AI 批改。"}</span>
                                    }
                                </div>
                            } else {
                                <button type="button" class="btn btn-primary btn-sm"
                                    data-testid="shenlun-start" disabled={*attempt_busy || props.csrf.is_empty()}
                                    onclick={new_attempt.clone()}>{"开始作答"}</button>
                            }
                            if let Some(message) = (*draft_error).as_ref() {
                                <p class="text-error text-sm" role="alert">{message}</p>
                            }
                            if !attempt_history.is_empty() {
                                <div class="border-t border-base-300 pt-2">
                                    <h3 class="mb-2 text-xs font-semibold">{"本题作答历史"}</h3>
                                    <div class="flex flex-wrap gap-2">
                                        {for attempt_history.iter().map(|item| {
                                            let item = item.clone();
                                            let select = select_attempt.clone();
                                            let label = if item.status == "submitted" {"已交卷"} else {"草稿"};
                                            let revision = item.revision;
                                            html! {
                                                <button type="button" key={item.id.clone()}
                                                    class="btn btn-ghost btn-xs"
                                                    disabled={navigation_locked}
                                                    onclick={Callback::from(move |_| select.emit(item.clone()))}>
                                                    {format!("{} · v{}", label, revision)}
                                                </button>
                                            }
                                        })}
                                    </div>
                                </div>
                            }
                            if let Some(record)=(*attempt).as_ref() {
                                <ShenlunAiPanel attempt={record.clone()} csrf={props.csrf.clone()}
                                on_confirm={Callback::from({
                                    let attempt=attempt.clone();
                                    let draft=draft.clone();
                                    let history=attempt_history.clone();
                                    move |next: ShenlunAttempt| {
                                        draft.set(next.text.clone());
                                        let mut all=(*history).clone();
                                        if let Some(item)=all.iter_mut().find(|item|item.id==next.id) {
                                            *item=next.clone();
                                        }
                                        history.set(all);
                                        attempt.set(Some(next));
                                    }
                                })}/>
                            }
                            <p class="text-xs opacity-60">{"已交卷不等于已评分；来源答案需本稿完成批改后解锁。"}</p>
                        </div>
                    }
                </div>
                <footer class="flex justify-between gap-2 border-t border-base-300 px-3 py-2">
                    <button type="button" class="btn btn-outline btn-sm" disabled={index == 0 || navigation_locked} onclick={previous}>{"上一题"}</button>
                    <button type="button" class="btn btn-outline btn-sm" disabled={index + 1 >= current.questions.len() || navigation_locked} onclick={next}>{"下一题"}</button>
                </footer>
            </section>
        };
        html! {
            <div class="flex h-full min-h-0 flex-col gap-2" data-testid="shenlun-workspace">
                <header class="flex shrink-0 items-center gap-3">
                    <button type="button" class="btn btn-ghost btn-sm" disabled={navigation_locked} onclick={close}>{"← 返回选题"}</button>
                    <h2 class="min-w-0 truncate text-sm font-semibold">{current.title.clone()}</h2>
                </header>
                <div class="min-h-0 flex-1">
                    <TrainingSplitWorkspace left={left} right={right} test_id="shenlun-split" resizable={true} split_at_md={true} />
                </div>
            </div>
        }
    } else {
        html! {
            <section class="flex h-full min-h-0 flex-col gap-3 overflow-hidden" data-testid="shenlun-catalog">
                <div class="flex flex-wrap items-center justify-between gap-3">
                    <div>
                        <h2 class="text-lg font-semibold">{"申论批改"}</h2>
                        <p class="text-xs opacity-70">{"从现有国考、湖南省考申论真题中选题。来源答案不提前展示。"}
                        </p>
                    </div>
                    <button type="button" class="btn btn-ghost btn-sm" onclick={retry}>{"刷新题库"}</button>
                </div>
                <div class="flex flex-wrap items-center gap-2">
                    <button type="button" class={classes!("btn","btn-sm",if *region == "全部" {"btn-primary"} else {"btn-outline"})} onclick={all_regions}>{"全部地区"}</button>
                    <button type="button" class={classes!("btn","btn-sm",if *region == "国考" {"btn-primary"} else {"btn-outline"})} onclick={national}>{"国考"}</button>
                    <button type="button" class={classes!("btn","btn-sm",if *region == "湖南" {"btn-primary"} else {"btn-outline"})} onclick={hunan}>{"湖南"}</button>
                    <input class="input input-bordered input-sm w-28" type="search" inputmode="numeric" aria-label="筛选年份"
                        placeholder="年份" value={(*year).clone()} oninput={set_year} />
                </div>
                if let Some(message) = (*error).as_ref() {
                    <p class="text-error" role="alert">{message}</p>
                }
                <div class="min-h-0 flex-1 overflow-y-auto" data-testid="shenlun-paper-list">
                    if *loading {
                        <p class="py-4 text-sm opacity-70">{"正在加载申论真题…"}</p>
                    } else if let Some(all) = (*papers).as_ref() {
                        <div class="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-3">
                            {for all.iter()
                                .filter(|item| (*region == "全部" || item.region.contains(region.as_str())) && (year.is_empty() || item.year.to_string().contains(year.as_str())))
                                .map(|item| {
                                    let id = item.id.clone();
                                    let open = open.clone();
                                    html! {
                                        <button key={id.clone()} type="button" class="rounded-xl border border-base-300 bg-base-100 p-4 text-left hover:border-primary disabled:opacity-50"
                                            disabled={*busy} onclick={Callback::from(move |_| open.emit(id.clone()))}
                                            data-testid="shenlun-open-paper">
                                            <div class="font-semibold">{item.title.clone()}</div>
                                            <p class="mt-2 text-xs opacity-70">{format!("{} · {} · {} · {} 题", item.year, item.region, item.variant, item.questions)}</p>
                                        </button>
                                    }
                                })}
                        </div>
                    }
                </div>
            </section>
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_label_does_not_guess_type_from_question_number() {
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
