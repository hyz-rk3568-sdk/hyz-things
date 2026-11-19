use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct QuestionAiModeTabsProps {
    pub(crate) active: bool,
    pub(crate) original: Callback<MouseEvent>,
    pub(crate) ai: Callback<MouseEvent>,
    #[prop_or_default]
    pub(crate) original_active: Option<bool>,
    #[prop_or("question-bank-original-tab")]
    pub(crate) original_test_id: &'static str,
    #[prop_or("question-bank-ai-tab")]
    pub(crate) ai_test_id: &'static str,
    #[prop_or("原始解析")]
    pub(crate) original_label: &'static str,
    #[prop_or("AI 解析")]
    pub(crate) ai_label: &'static str,
}

#[function_component(QuestionAiModeTabs)]
pub(crate) fn question_ai_mode_tabs(props: &QuestionAiModeTabsProps) -> Html {
    let original_active = props.original_active.unwrap_or(!props.active);
    html! {
        <div class="flex shrink-0 items-center gap-0" role="group" aria-label="AI 复盘模式">
            <button type="button" class={classes!(QUESTION_HEADER_ICON_BUTTON, if original_active {"btn-active"} else {""})}
                aria-label={props.original_label} title={props.original_label}
                data-testid={props.original_test_id} aria-pressed={original_active.to_string()} onclick={props.original.clone()}>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 3h9l4 4v14H6zM14 3v5h5M9 12h7M9 16h7"/></svg>
            </button>
            <button type="button" class={classes!(QUESTION_HEADER_ICON_BUTTON, if props.active {"btn-active"} else {""}, if props.active {"text-primary"} else {""})}
                aria-label={props.ai_label} title={props.ai_label}
                data-testid={props.ai_test_id} aria-pressed={props.active.to_string()} onclick={props.ai.clone()}>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v3"/><rect x="4" y="6" width="16" height="12" rx="2"/><path d="M8 18v3M16 18v3"/><circle cx="9" cy="11.5" r="1" fill="currentColor" stroke="none"/><circle cx="15" cy="11.5" r="1" fill="currentColor" stroke="none"/></svg>
            </button>
        </div>
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct ReviewAnswerStatsProps {
    pub correct_answer: Vec<String>,
    pub answer: Option<Vec<String>>,
    pub correct_count: usize,
    pub wrong_count: usize,
    pub answer_test_id: &'static str,
    pub counts_test_id: &'static str,
}

#[function_component(ReviewAnswerStats)]
pub(crate) fn review_answer_stats(props: &ReviewAnswerStatsProps) -> Html {
    html! {
        <section class="rounded-box border border-base-content/15 p-3" aria-label="答案统计">
            if let Some(answer) = &props.answer {
                <p class="text-sm text-base-content/60">{format!("你的答案：{}", if answer.is_empty() {"未答".into()} else {answer.join("、")})}</p>
            }
            <p class="text-xs opacity-60">{"正确答案"}</p>
            <p class="font-semibold" data-testid={props.answer_test_id}>
                {if props.correct_answer.is_empty() {"暂无可靠答案".into()} else {props.correct_answer.join("、")}}
            </p>
            <p class="mt-2 text-sm text-base-content/60" data-testid={props.counts_test_id}
                data-correct={props.correct_count.to_string()} data-wrong={props.wrong_count.to_string()}>
                {format!("本题累计：做对 {} 次 · 做错 {} 次", props.correct_count, props.wrong_count)}
            </p>
        </section>
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct QuestionAiMoreMenuProps {
    pub can_delete: bool,
    pub delete: Callback<MouseEvent>,
    #[prop_or_default]
    pub reset_key: String,
    #[prop_or("question-bank-ai-more")]
    pub button_test_id: &'static str,
    #[prop_or("question-bank-ai-delete")]
    pub delete_test_id: &'static str,
    #[prop_or("更多解析操作")]
    pub button_label: &'static str,
    #[prop_or("删除 AI 解析")]
    pub delete_label: &'static str,
}

#[function_component(QuestionAiMoreMenu)]
pub(crate) fn question_ai_more_menu(props: &QuestionAiMoreMenuProps) -> Html {
    let open = use_state(|| false);
    {
        let open = open.clone();
        use_effect_with(props.reset_key.clone(), move |_| {
            open.set(false);
            || ()
        });
    }
    let delete = {
        let open = open.clone();
        let callback = props.delete.clone();
        Callback::from(move |event: MouseEvent| {
            open.set(false);
            callback.emit(event);
        })
    };
    html! {
        <div class="relative shrink-0">
            <button type="button" class={QUESTION_HEADER_ICON_BUTTON}
                aria-label={props.button_label} title={props.button_label}
                data-testid={props.button_test_id} aria-expanded={open.to_string()}
                onclick={Callback::from({let open=open.clone();move |_|open.set(!*open)})}>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="12" cy="5" r="1.5"/><circle cx="12" cy="12" r="1.5"/><circle cx="12" cy="19" r="1.5"/></svg>
            </button>
            if *open {
                <div class="absolute right-0 top-full z-50 min-w-40 rounded-box border border-base-content/10 bg-base-100 p-1 shadow-lg">
                    if props.can_delete {
                        <button type="button" class="btn btn-ghost btn-sm w-full justify-start text-error"
                            data-testid={props.delete_test_id} onclick={delete}>{props.delete_label}</button>
                    } else {
                        <span class="block px-3 py-2 text-xs text-base-content/60">{"暂无可用操作"}</span>
                    }
                </div>
            }
        </div>
    }
}
