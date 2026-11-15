//! Read-only 申论 source answer for the current question.
use super::*;
use hyz_things::domain::question_bank::ShenlunSourceAnswer;
#[derive(Properties, PartialEq)]
pub(super) struct ShenlunSourceAnswerPanelProps {
    pub(super) question_id: String,
}

#[function_component(ShenlunSourceAnswerPanel)]
pub(super) fn shenlun_source_answer_panel(props: &ShenlunSourceAnswerPanelProps) -> Html {
    let answer = use_state(|| None::<ShenlunSourceAnswer>);
    let error = use_state(|| None::<String>);
    let loading = use_state(|| true);
    {
        let loading = loading.clone();
        let answer = answer.clone();
        let error = error.clone();
        use_effect_with(props.question_id.clone(), move |question_id| {
            loading.set(true);
            answer.set(None);
            error.set(None);
            let alive = Rc::new(Cell::new(true));
            let guard = alive.clone();
            let endpoint =
                format!("/api/v1/question-bank/shenlun/questions/{question_id}/source-answer");
            spawn_local(async move {
                match fetch_json::<ShenlunSourceAnswer>(&endpoint, "题库来源参考答案").await
                {
                    Ok(value) if guard.get() => answer.set(Some(value)),
                    Err(message) if guard.get() => error.set(Some(message)),
                    _ => (),
                }
                if guard.get() {
                    loading.set(false);
                }
            });
            move || alive.set(false)
        });
    }
    html! {
        <section class="mt-3 space-y-3" data-testid="shenlun-source-panel">
            <h3 class="font-semibold">{"题库来源参考答案"}</h3>
            if let Some(value) = (*answer).as_ref() {
                <article class="study-markdown" data-testid="shenlun-source-answer">
                    <p class="text-xs opacity-60">{value.provenance.clone()}</p>
                    {render_question_bank_markdown(&value.text)}
                </article>
            } else if *loading {
                <p class="text-sm text-base-content/60" role="status">{"正在加载题库参考答案…"}</p>
            } else if error.is_none() {
                <p class="text-sm text-base-content/60">{"题库暂无可展示的参考答案。"}</p>
            }
            if let Some(message) = (*error).as_ref() {
                <p role="alert" class="text-sm text-error">{message.clone()}</p>
            }
        </section>
    }
}
