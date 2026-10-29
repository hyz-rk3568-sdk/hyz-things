//! Shared score-summary cards for data-analysis and logic judgment.
use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct TrainingMetricsProps {
    pub answered: i64,
    pub correct: i64,
    pub prefix: AttrValue,
}

#[function_component(TrainingMetrics)]
pub(crate) fn training_metrics(props: &TrainingMetricsProps) -> Html {
    let accuracy = if props.answered <= 0 {
        "未作答".to_owned()
    } else {
        format!("{:.1}%", 100.0 * props.correct as f64 / props.answered as f64)
    };
    html! {
        <div class="grid min-w-0 grid-cols-3 gap-2" data-testid={format!("{}-answer-summary", props.prefix)}>
            <div class="min-w-0 rounded-box bg-base-200 p-3">
                <p class="text-xs text-base-content/60">{"累计作答"}</p>
                <p class="text-xl font-semibold" data-testid={format!("{}-total-answered", props.prefix)}>{props.answered}</p>
            </div>
            <div class="min-w-0 rounded-box bg-base-200 p-3">
                <p class="text-xs text-base-content/60">{"累计答对"}</p>
                <p class="text-xl font-semibold" data-testid={format!("{}-total-correct", props.prefix)}>{props.correct}</p>
            </div>
            <div class="min-w-0 rounded-box bg-base-200 p-3">
                <p class="text-xs text-base-content/60">{"总体正确率"}</p>
                <p class="text-xl font-semibold" data-testid={format!("{}-total-accuracy", props.prefix)}>{accuracy}</p>
            </div>
        </div>
    }
}
