//! 申论训练：复用 Question Bank 真题复盘工作区，而非建立第二套考试 Session。
use super::favorite_flashcard::{favorite_can_create, FavoriteFlashcardWorkspace, FavoritePreview};
use super::material_scroll::{use_training_scroll_pair, MaterialScrollButton};
use super::question_bank_practice::{
    use_practice_favorite, QuestionAiReview, QuestionBankQuestionHeader,
};
use super::*;
use hyz_things::domain::question_bank::{
    FavoriteState, PaperSummary, PaperView, QuestionView, SetFavorite,
};

#[derive(Properties, PartialEq)]
pub(super) struct QuestionBankShenlunProps {
    pub(super) active: bool,
    pub(super) on_immersive_change: Callback<bool>,
    pub(super) csrf: String,
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

#[function_component(QuestionBankShenlun)]
pub(super) fn question_bank_shenlun(props: &QuestionBankShenlunProps) -> Html {
    let papers = use_state(|| None::<Vec<PaperSummary>>);
    let paper = use_state(|| None::<PaperView>);
    let selected_paper = use_state(String::new);
    let selected = use_state(|| 0_usize);
    let busy = use_state(|| false);
    let loading = use_state(|| true);
    let error = use_state(|| None::<String>);
    let mode = use_state(|| "ai".to_owned());
    let ai_delete_signal = use_state(|| 0_u32);
    let ai_delete_availability = use_state(|| None::<(String, String, bool)>);
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
    {
        let flashcard_preview = flashcard_preview.clone();
        let flashcard_error = flashcard_error.clone();
        let flashcard_open = flashcard_open.clone();
        use_effect_with(
            (question_key.clone(), *flashcard_revision),
            move |(key, _)| {
                let alive = Rc::new(Cell::new(true));
                let guard = alive.clone();
                flashcard_preview.set(None);
                flashcard_error.set(None);
                flashcard_open.set(false);
                if let Some((_, qid)) = key.clone() {
                    let flashcard_preview = flashcard_preview.clone();
                    let flashcard_error = flashcard_error.clone();
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
                                Ok(preview) => flashcard_preview.set(Some((qid, preview))),
                                Err(message) => flashcard_error.set(Some(message)),
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
    let open = {
        let busy = busy.clone();
        let paper = paper.clone();
        let selected = selected.clone();
        let mode = mode.clone();
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
            let mode = mode.clone();
            let error = error.clone();
            let immersive = immersive.clone();
            spawn_local(async move {
                match fetch_json::<PaperView>(
                    &format!("/api/v1/question-bank/shenlun/papers/{id}"),
                    "申论原卷",
                )
                .await
                {
                    Ok(value) => {
                        selected.set(0);
                        mode.set("ai".into());
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
        let mode = mode.clone();
        let immersive = props.on_immersive_change.clone();
        Callback::from(move |_| {
            immersive.emit(false);
            mode.set("ai".into());
            paper.set(None);
        })
    };
    {
        let availability = ai_delete_availability.clone();
        let reset_key = (question_key.clone(), (*mode).clone());
        use_effect_with(reset_key, move |_| {
            availability.set(None);
            || ()
        });
    }
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
        let mode_value = (*mode).clone();
        let icon_button = super::question_header_actions::QUESTION_HEADER_ICON_BUTTON;

        let left = html! {
            <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100" data-testid="shenlun-materials">
                <div class={classes!(super::question_bank_practice::QUESTION_BANK_PANE_HEADER, "question-bank-pane-header")}>
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
                    data-swipe-ignore="true">
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
            <super::question_header_actions::QuestionHeaderActions
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
                    move |_|flashcard_open.set(true)
                })}/>
        };
        let can_delete = matches!(mode_value.as_str(), "grade" | "ai")
            && ai_delete_availability.as_ref().as_ref().is_some_and(
                |(qid, active_mode, available)| {
                    qid == &question_id && active_mode == &mode_value && *available
                },
            );
        let menu = html! {
            <super::question_bank_practice::QuestionAiMoreMenu
                can_delete={can_delete}
                reset_key={format!("{paper_id}:{question_id}:{mode_value}")}
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
            let mode = mode_value.clone();
            Callback::from(move |available| {
                availability.set(Some((question_id.clone(), mode.clone(), available)));
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
                            <button type="button" class={classes!(icon_button, if mode_value=="grade" {"text-primary"} else {""})}
                                aria-label="AI 批改" title="AI 批改" aria-pressed={(mode_value=="grade").to_string()} data-testid="shenlun-mode-grade"
                                onclick={Callback::from({let mode=mode.clone();move |_|mode.set("grade".into())})}>
                                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="3" width="16" height="18" rx="2"/><path d="M8 8h8M8 12h4m-4 4 2 2 4-4"/></svg>
                            </button>
                            <button type="button" class={classes!(icon_button, if mode_value=="ocr" {"text-primary"} else {""})}
                                aria-label="OCR 转写" title="OCR 转写" aria-pressed={(mode_value=="ocr").to_string()} data-testid="shenlun-mode-ocr"
                                onclick={Callback::from({let mode=mode.clone();move |_|mode.set("ocr".into())})}>
                                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 3H5a2 2 0 0 0-2 2v3M16 3h3a2 2 0 0 1 2 2v3M3 16v3a2 2 0 0 0 2 2h3m8 0h3a2 2 0 0 0 2-2v-3M7 12h10M9 9h6m-6 6h6"/></svg>
                            </button>
                            <super::question_bank_practice::QuestionAiModeTabs
                                active={mode_value=="ai"}
                                original_active={Some(mode_value=="source")}
                                original_test_id="shenlun-mode-source"
                                ai_test_id="shenlun-mode-ai"
                                original_label="来源参考答案"
                                ai_label="AI 解析与追问"
                                original={Callback::from({let mode=mode.clone();move |_|mode.set("source".into())})}
                                ai={Callback::from({let mode=mode.clone();move |_|mode.set("ai".into())})}/>
                        </>
                    }}
                    trailing={menu}/>
                if *flashcard_open {
                    if let Some(preview)=card_preview {
                        <div class="min-h-0 flex-1 overflow-y-auto p-3" data-testid="shenlun-flashcard-workspace">
                            <FavoriteFlashcardWorkspace key={question_id.clone()}
                                question_id={question_id.clone()} preview={preview.clone()}
                                csrf={props.csrf.clone()}
                                on_close={Callback::from({
                                    let flashcard_open=flashcard_open.clone();
                                    move |_|flashcard_open.set(false)
                                })}
                                on_changed={Callback::from({
                                    let flashcard_revision=flashcard_revision.clone();
                                    move |_|flashcard_revision.set(flashcard_revision.wrapping_add(1))
                                })}/>
                        </div>
                    }
                } else if matches!(mode_value.as_str(), "grade"|"ai") {
                    <QuestionAiReview key={format!("{}:{}",question_id,mode_value)}
                        mode={if mode_value=="grade" {"grade"} else {"analysis"}}
                        draft_namespace={Some("shenlun")}
                        csrf={props.csrf.clone()} question_id={question_id.clone()}
                        question_content={question_body.clone()}
                        question_scroll={scroll.second.clone()} on_question_scroll={scroll.on_scroll.clone()}
                        delete_signal={*ai_delete_signal} on_delete_availability={on_delete_availability}/>
                } else if mode_value=="ocr" {
                    <div class="flex min-h-0 flex-1 flex-col overflow-hidden" data-testid="shenlun-ocr-pane">
                        <div class="max-h-[35%] shrink-0 overflow-y-auto border-b border-base-content/10 p-4"
                            data-testid="shenlun-ocr-question-context">
                            {question_body.clone()}
                        </div>
                        <ShenlunAiPanel key={question_id.clone()}
                            question_id={question_id.clone()} csrf={props.csrf.clone()}/>
                    </div>
                } else {
                    <div class="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4" ref={scroll.second.clone()}
                        onscroll={scroll.on_scroll.clone()} data-swipe-ignore="true">
                        {question_body.clone()}
                        if mode_value=="source" {
                            <super::shenlun_ai_panel::ShenlunSourceAnswerPanel key={question_id.clone()}
                                question_id={question_id.clone()}/>
                        }
                        if let Some(message)=(*error).as_ref() {
                            <p role="alert" class="mt-2 text-sm text-error">{message.clone()}</p>
                        }
                    </div>
                }
            </section>
        };
        let navigation_allowed = Callback::from({
            let availability = ai_delete_availability.clone();
            move |_: ()| {
                availability.set(None);
                true
            }
        });
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
            <div class={classes!(super::question_bank_practice::WORKSPACE, "gap-2")} data-testid="shenlun-workspace">
                <TrainingHeader class="flex shrink-0 flex-col gap-1 border-b border-base-content/10 pb-2 sm:flex-row sm:flex-nowrap sm:items-center sm:justify-between sm:gap-2">
                    <div class="flex min-w-0 flex-1 items-center gap-3">
                        <button type="button" class="btn btn-ghost btn-sm" onclick={close}>{"← 返回选题"}</button>
                        <div class="min-w-0 flex-1">
                            <strong class="block truncate text-sm">{current.title.clone()}</strong>
                            <span class="block truncate text-[11px] text-base-content/50">{"申论训练 · 原卷复盘"}</span>
                        </div>
                    </div>
                    <div class="flex min-w-0 flex-wrap items-center gap-2 sm:flex-nowrap sm:justify-end">
                        <StudyGoalBars />
                        <CodexLimitBars />
                    </div>
                </TrainingHeader>
                <div class="relative min-h-0 flex-1 overflow-hidden">
                    <TrainingSwipeSurface current={selected.clone()} total={current.questions.len()}
                        navigation_allowed={Some(navigation_allowed.clone())}>
                        <TrainingSplitWorkspace left={left} right={right}
                            test_id="shenlun-split" resizable={true} split_at_md={true}/>
                    </TrainingSwipeSurface>
                </div>
                <CompactQuestionNavigation current={selected.clone()}
                    navigation_allowed={Some(navigation_allowed)}
                    numbers={current.questions.iter().map(|q|q.number).collect::<Vec<_>>()}
                    answered={answered} grades={None::<Vec<Option<bool>>>} groups={groups}/>
            </div>
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
            let open = open.clone();
            Callback::from(move |_| {
                let id = (*selected_paper).clone();
                if !id.is_empty() {
                    open.emit(id);
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
        html! {
            <TrainingSetupLayout test_id="shenlun-catalog" wide={true}
                action={Some(start_action)} summary="申论复盘" detail={action_detail}>
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
