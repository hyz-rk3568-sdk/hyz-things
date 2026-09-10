use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct CountdownDisplay {
    pub(crate) remaining_seconds: u64,
    pub(crate) days: u64,
    pub(crate) hours: u64,
    pub(crate) minutes: u64,
    pub(crate) seconds: u64,
    pub(crate) progress_percent: u8,
    pub(crate) finished: bool,
}

pub(crate) fn countdown_values(display: CountdownDisplay, finished_label: AttrValue) -> Html {
    if display.finished {
        html! { <strong class={EXAM_COUNTER_FINISHED}>{finished_label}</strong> }
    } else {
        html! {
            <>
                <strong data-value="days" class={EXAM_COUNTER_VALUE}>{display.days}</strong><span class={EXAM_COUNTER_UNIT}> {"天"}</span>
                <strong data-value="hours" class={EXAM_COUNTER_VALUE}>{format!("{:02}", display.hours)}</strong><span class={EXAM_COUNTER_UNIT}> {"时"}</span>
                <strong data-value="minutes" class={EXAM_COUNTER_VALUE}>{format!("{:02}", display.minutes)}</strong><span class={EXAM_COUNTER_UNIT}> {"分"}</span>
                <strong data-value="seconds" class={EXAM_COUNTER_VALUE}>{format!("{:02}", display.seconds)}</strong><span class={EXAM_COUNTER_UNIT}> {"秒"}</span>
            </>
        }
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct CountdownCardProps {
    pub(crate) id: AttrValue,
    pub(crate) title: AttrValue,
    pub(crate) eyebrow: AttrValue,
    pub(crate) target_iso: AttrValue,
    pub(crate) target_label: AttrValue,
    pub(crate) target_note: AttrValue,
    pub(crate) display: CountdownDisplay,
    pub(crate) index: usize,
    pub(crate) status: AttrValue,
    pub(crate) tone: Classes,
    pub(crate) card_label: AttrValue,
    pub(crate) finished_label: AttrValue,
    pub(crate) progress_label: AttrValue,
    pub(crate) progress_aria_label: AttrValue,
    pub(crate) on_double_click: Callback<MouseEvent>,
}

#[function_component(CountdownCard)]
pub(crate) fn countdown_card(props: &CountdownCardProps) -> Html {
    html! {
        <article
            class={classes!(EXAM_CARD, props.tone.clone())}
            aria-label={props.card_label.clone()}
            data-exam-id={props.id.clone()}
            data-remaining-seconds={props.display.remaining_seconds.to_string()}
            title="双击开启画中画"
            ondblclick={props.on_double_click.clone()}
        >
            <div class={EXAM_CARD_HEAD}>
                <div class="flex min-w-0 items-start gap-2">
                    <span class={EXAM_CARD_INDEX} aria-hidden="true">{format!("{:02}", props.index + 1)}</span>
                    <div class={EXAM_CARD_COPY}>
                        <p class={EXAM_CARD_EYEBROW}>{props.eyebrow.clone()}</p>
                        <h3 class={EXAM_CARD_TITLE}>{props.title.clone()}</h3>
                    </div>
                </div>
                <span data-status="" class={classes!(EXAM_STATUS, props.display.finished.then_some("text-base-content/60"), (!props.display.finished).then_some("text-base-content/70"))}>{props.status.clone()}</span>
            </div>
            <div class={EXAM_COUNTER} data-countdown-values="" aria-live="polite">
                {countdown_values(props.display, props.finished_label.clone())}
            </div>
            <div class={EXAM_META}>
                <span class="min-w-0 truncate">{props.target_note.clone()}</span>
                <time class={EXAM_DATE} datetime={props.target_iso.clone()}>{props.target_label.clone()}</time>
            </div>
            <progress
                class={EXAM_PROGRESS}
                max="100"
                value={props.display.progress_percent.to_string()}
                aria-label={props.progress_aria_label.clone()}
            ></progress>
            <div class={EXAM_PROGRESS_META} data-progress-meta="">
                <span>{props.progress_label.clone()}</span>
                <span>{format!("{}%", props.display.progress_percent)}</span>
            </div>
        </article>
    }
}
