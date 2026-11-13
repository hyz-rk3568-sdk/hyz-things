//! 申论批改：与行测客观题并列的真题选择、材料阅读入口。
//! 不展示来源参考答案；主观题草稿/评分在独立服务就绪后接入。
use super::*;
use hyz_things::domain::question_bank::{PaperSummary, PaperView, QuestionView};

#[derive(Properties, PartialEq)]
pub(super) struct ShenlunTrainingProps {
    pub(super) on_immersive_change: Callback<bool>,
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
                        <div class="mt-6 rounded-lg border border-dashed border-base-300 p-4 text-sm opacity-70" data-testid="shenlun-readonly-notice">
                            {"当前仅开放真题与材料预览。主观题持久草稿、手写校对和 AI 批改需要独立数据链路，尚未开放提交。"}
                        </div>
                    }
                </div>
                <footer class="flex justify-between gap-2 border-t border-base-300 px-3 py-2">
                    <button type="button" class="btn btn-outline btn-sm" disabled={index == 0} onclick={previous}>{"上一题"}</button>
                    <button type="button" class="btn btn-outline btn-sm" disabled={index + 1 >= current.questions.len()} onclick={next}>{"下一题"}</button>
                </footer>
            </section>
        };
        html! {
            <div class="flex h-full min-h-0 flex-col gap-2" data-testid="shenlun-workspace">
                <header class="flex shrink-0 items-center gap-3">
                    <button type="button" class="btn btn-ghost btn-sm" onclick={close}>{"← 返回选题"}</button>
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
