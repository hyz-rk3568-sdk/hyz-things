//! Read-only review of a saved wrong or favorite question. The timed training
//! sessions stay separate; this view only reads one question at a time and
//! lets the reader swipe through the filtered collection.
use super::favorite_flashcard::FavoriteFlashcardAction;
use super::question_bank_practice::answer_options;
use super::*;
use hyz_things::domain::question_bank::{PracticeBrowseItem, PracticeMode, PracticeQuestionDetail};

const ROOT: &str = "/api/v1/question-bank";

#[derive(Properties, PartialEq)]
pub(crate) struct CollectionQuestionReviewProps {
    pub items: Vec<PracticeBrowseItem>,
    pub initial_index: usize,
    pub scope: PracticeMode,
    pub csrf: String,
    pub on_back: Callback<()>,
}

#[function_component(CollectionQuestionReview)]
pub(crate) fn collection_question_review(props: &CollectionQuestionReviewProps) -> Html {
    let index = use_state(|| props.initial_index.min(props.items.len().saturating_sub(1)));
    let detail = use_state(|| None::<PracticeQuestionDetail>);
    let error = use_state(|| None::<String>);
    let retry = use_state(|| 0u32);
    let material_scroll = use_node_ref();
    let question_scroll = use_node_ref();
    let question_id = props.items.get(*index).map(|item| item.question_id.clone());
    {
        let detail = detail.clone();
        let error = error.clone();
        use_effect_with((question_id.clone(), *retry), move |(id, _)| {
            let active = Rc::new(Cell::new(true));
            let current = active.clone();
            detail.set(None);
            error.set(None);
            if let Some(id) = id.clone() {
                spawn_local(async move {
                    let result = fetch_json::<PracticeQuestionDetail>(
                        &format!("{ROOT}/practice/questions/{id}"),
                        "题目复盘",
                    )
                    .await;
                    if current.get() {
                        match result {
                            Ok(value) => detail.set(Some(value)),
                            Err(message) => error.set(Some(message)),
                        }
                    }
                });
            }
            move || active.set(false)
        });
    }
    {
        let question = detail
            .as_ref()
            .as_ref()
            .map(|detail| detail.question.id.clone());
        use_effect_with(question, |_| {
            hydrate_math();
            || ()
        });
    }
    {
        let material_scroll = material_scroll.clone();
        let question_scroll = question_scroll.clone();
        use_effect_with(*index, move |_| {
            for target in [&material_scroll, &question_scroll] {
                if let Some(element) = target.cast::<HtmlElement>() {
                    element.set_scroll_top(0);
                }
            }
            || ()
        });
    }
    let Some(item) = props.items.get(*index).cloned() else {
        return Html::default();
    };
    let scope_label = if props.scope == PracticeMode::Favorite {
        "收藏"
    } else {
        "错题"
    };
    let back = {
        let on_back = props.on_back.clone();
        Callback::from(move |_| on_back.emit(()))
    };
    let previous = {
        let index = index.clone();
        Callback::from(move |_| index.set(index.saturating_sub(1)))
    };
    let next = {
        let index = index.clone();
        let total = props.items.len();
        Callback::from(move |_| index.set((*index + 1).min(total.saturating_sub(1))))
    };
    let main = if let Some(current) = &*detail {
        let shared = !current.materials.is_empty();
        let options = answer_options(&current.question.options_markdown);
        let material_pane = html! {
            <section class="flex min-h-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100"
                data-testid="collection-review-material-pane">
                <div class={classes!(TRAINING_PANE_HEADER,"question-bank-pane-header")}>
                    <strong>{"原卷材料"}</strong>
                    <span class="text-[11px] text-base-content/45">{"复盘"}</span>
                </div>
                <div ref={material_scroll.clone()}
                    class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4"
                    data-testid="collection-review-material-scroll">
                    {for current.materials.iter().map(|material| html!{
                        <section class="mb-5">
                            if current.materials.len()>1 {<h3 class="font-bold">{material.name.clone()}</h3>}
                            <div class="study-markdown question-bank-passage">{render_question_bank_markdown(&material.markdown)}</div>
                        </section>
                    })}
                </div>
            </section>
        };
        let stem_pane = html! {
            <section class="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100"
                data-testid="collection-review-stem-pane">
                <div class={classes!(TRAINING_PANE_HEADER,"question-bank-pane-header")}>
                    <strong>{"题干"}</strong>
                    <span class="text-[11px] text-base-content/45">{"复盘"}</span>
                </div>
                <div ref={material_scroll.clone()}
                    class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4"
                    data-testid="collection-review-material-scroll">
                    <div class="study-markdown question-bank-passage" data-testid="collection-review-stem">
                        {render_question_bank_markdown(&current.question.stem)}
                    </div>
                </div>
            </section>
        };
        let left = if shared { material_pane } else { stem_pane };
        let right = html! {
            <section class="flex h-full min-h-0 flex-col overflow-hidden rounded-box border border-base-content/10 bg-base-100"
                data-testid="collection-review-question-pane">
                <div class={classes!(TRAINING_PANE_HEADER,"question-bank-pane-header")}>
                    <strong class="min-w-0 truncate text-sm" data-testid="collection-review-heading">
                        {format!("第 {} 题", current.item.number)}
                    </strong>
                    <span class="shrink-0 text-[11px] text-base-content/45">
                        {[current.item.module.as_deref(),current.item.category.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" · ")}
                    </span>
                </div>
                <div ref={question_scroll.clone()}
                    class="min-h-0 flex-1 overflow-y-auto overscroll-contain touch-pan-y p-4 space-y-4"
                    data-testid="collection-review-question-scroll">
                    if shared {
                        <div class="study-markdown question-bank-passage" data-testid="collection-review-stem">
                            {render_question_bank_markdown(&current.question.stem)}
                        </div>
                    }
                    <div class="space-y-2">
                        {for options.iter().map(|(label,markdown)|{
                            let correct=current.correct_answer.contains(label);
                            html!{<div class={classes!("flex","items-start","gap-2.5","rounded-xl","border","p-3","text-sm","leading-6",if correct {"border-success bg-success/10 text-success"} else {"border-base-content/20"})}
                                data-testid="collection-review-choice">
                                <span class="shrink-0 font-bold">{format!("{label}.")}</span>
                                <span class="study-markdown min-w-0">{render_question_bank_markdown(markdown)}</span>
                            </div>}
                        })}
                    </div>
                    <section class="rounded-box border border-base-content/15 p-3" aria-label="正确答案">
                        <p class="text-xs opacity-60">{"正确答案"}</p>
                        <p class="font-semibold" data-testid="collection-review-answer">
                            {if current.correct_answer.is_empty() {"暂无可靠答案".into()} else {current.correct_answer.join("、")}}
                        </p>
                    </section>
                    if !current.explanation.is_empty() {
                        <section class="border-t border-base-content/10 pt-3">
                            <h3 class="font-bold">{"原始解析"}</h3>
                            <div class="study-markdown mt-2 text-sm leading-7 text-base-content/75" data-testid="collection-review-explanation">
                                {render_question_bank_markdown(&current.explanation)}
                            </div>
                        </section>
                    }
                    if !current.question.topics.is_empty() {
                        <section>
                            <h3 class="font-bold">{"考点"}</h3>
                            <p class="text-sm text-base-content/60">{current.question.topics.join("、")}</p>
                        </section>
                    }
                    if props.scope == PracticeMode::Favorite {
                        <FavoriteFlashcardAction item={current.item.clone()} csrf={props.csrf.clone()}/>
                    }
                </div>
            </section>
        };
        html! {
            <TrainingSplitWorkspace test_id="collection-review-split" split_at_md={true}
                left={left} right={right}/>
        }
    } else if let Some(message) = &*error {
        html! {
            <div class="flex h-full items-center justify-center rounded-box bg-base-100" data-testid="collection-review-error">
                <div class="space-y-3 text-center">
                    <p role="alert" class="text-error">{message.clone()}</p>
                    <button type="button" class="btn btn-outline btn-sm"
                        onclick={Callback::from({let retry=retry.clone();move |_|retry.set(*retry+1)})}>{"重试加载"}</button>
                </div>
            </div>
        }
    } else {
        html! {
            <div class="flex h-full items-center justify-center rounded-box bg-base-100" data-testid="collection-review-loading">
                <p class="opacity-60" role="status">{"正在读取题目复盘…"}</p>
            </div>
        }
    };
    html! {
        <section class="question-bank-workspace training-workspace fixed inset-0 z-[80] flex h-dvh w-screen flex-col overflow-hidden bg-base-200 px-3 py-2"
            data-testid="collection-review" aria-label={format!("{scope_label}题目复盘")}>
            <header class="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-base-content/10 pb-2">
                <div class="flex min-w-0 items-center gap-3">
                    <button class="btn btn-ghost btn-sm" data-testid="collection-review-back" onclick={back}>
                        {format!("← 返回{scope_label}")}
                    </button>
                    <div class="min-w-0">
                        <strong class="block truncate text-sm">{item.paper_title.clone()}</strong>
                        <span class="block truncate text-[11px] text-base-content/50" data-testid="collection-review-scope">
                            {format!("{scope_label}复盘 · 第 {}/{} 题",*index+1,props.items.len())}
                        </span>
                    </div>
                </div>
                <span class="badge badge-outline">{"复盘"}</span>
            </header>
            <div class="relative min-h-0 flex-1 overflow-hidden">
                <TrainingSwipeSurface current={index.clone()} total={props.items.len()}>
                    {main}
                </TrainingSwipeSurface>
            </div>
            <nav class="flex shrink-0 items-center justify-center gap-3 border-t border-base-content/10 pt-2"
                aria-label="复盘题目导航">
                <button type="button" class="btn btn-outline btn-sm" data-testid="collection-review-previous"
                    disabled={*index==0} onclick={previous}>{"上一题"}</button>
                <span class="min-w-16 text-center text-sm tabular-nums" data-testid="collection-review-position">
                    {format!("{} / {}",*index+1,props.items.len())}
                </span>
                <button type="button" class="btn btn-outline btn-sm" data-testid="collection-review-next"
                    disabled={*index+1>=props.items.len()} onclick={next}>{"下一题"}</button>
            </nav>
        </section>
    }
}
