use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct CountdownEditorProps {
    pub(crate) id: AttrValue,
    pub(crate) title: AttrValue,
    pub(crate) display: CountdownDisplay,
    pub(crate) status: AttrValue,
    pub(crate) tone: Classes,
    pub(crate) hours: u64,
    pub(crate) minutes: u64,
    pub(crate) seconds: u64,
    pub(crate) active: bool,
    pub(crate) can_pause: bool,
    pub(crate) can_resume: bool,
    pub(crate) card_label: AttrValue,
    pub(crate) start_label: AttrValue,
    pub(crate) on_hours: Callback<InputEvent>,
    pub(crate) on_minutes: Callback<InputEvent>,
    pub(crate) on_seconds: Callback<InputEvent>,
    pub(crate) on_restart: Callback<MouseEvent>,
    pub(crate) on_pause: Callback<MouseEvent>,
    pub(crate) on_resume: Callback<MouseEvent>,
    pub(crate) on_open_pip: Callback<MouseEvent>,
}

#[function_component(CountdownEditor)]
pub(crate) fn countdown_editor(props: &CountdownEditorProps) -> Html {
    html! {
        <article
            class={classes!(CUSTOM_TIMER_CARD, props.tone.clone())}
            aria-label={props.card_label.clone()}
            data-exam-id={props.id.clone()}
            data-remaining-seconds={props.display.remaining_seconds.to_string()}
            title="双击开启画中画"
            ondblclick={props.on_open_pip.clone()}
        >
            <div class={CUSTOM_TIMER_HEAD}>
                <h3 class={CUSTOM_TIMER_TITLE}>{props.title.clone()}</h3>
                <span data-status="" class={classes!(EXAM_STATUS, props.display.finished.then_some("text-base-content/60"), (!props.display.finished).then_some("text-base-content/70"))}>{props.status.clone()}</span>
            </div>
            <div class={EXAM_COUNTER} data-countdown-values="" data-custom-countdown-values="" aria-live="polite">
                {countdown_values(props.display, AttrValue::from("时间到"))}
            </div>
            <div class={CUSTOM_TIMER_INPUTS}>
                <label class={FIELD}>
                    <span class={FIELD_LABEL}>{"时"}</span>
                    <input class={INPUT} type="number" min="0" max="99" inputmode="numeric" value={props.hours.to_string()} oninput={props.on_hours.clone()} aria-label={format!("{}小时", props.title)} />
                </label>
                <label class={FIELD}>
                    <span class={FIELD_LABEL}>{"分"}</span>
                    <input class={INPUT} type="number" min="0" max="59" inputmode="numeric" value={props.minutes.to_string()} oninput={props.on_minutes.clone()} aria-label={format!("{}分钟", props.title)} />
                </label>
                <label class={FIELD}>
                    <span class={FIELD_LABEL}>{"秒"}</span>
                    <input class={INPUT} type="number" min="0" max="59" inputmode="numeric" value={props.seconds.to_string()} oninput={props.on_seconds.clone()} aria-label={format!("{}秒", props.title)} />
                </label>
            </div>
            <div class={CUSTOM_TIMER_ACTIONS}>
                <button class={BUTTON_PRIMARY} type="button" onclick={props.on_restart.clone()} aria-label={props.start_label.clone()}>{if props.active { "重新开始" } else { "开始" }}</button>
                if props.can_pause {
                    <button class={BUTTON} type="button" onclick={props.on_pause.clone()} aria-label={format!("暂停{}倒计时", props.title)}> {"暂停"} </button>
                }
                if props.can_resume {
                    <button class={BUTTON} type="button" onclick={props.on_resume.clone()} aria-label={format!("继续{}倒计时", props.title)}> {"继续"} </button>
                }
                <button class={BUTTON_GHOST} type="button" onclick={props.on_open_pip.clone()} disabled={!props.active} aria-label={format!("进入{}画中画", props.title)}>{"画中画"}</button>
            </div>
            <progress
                class={EXAM_PROGRESS}
                max="100"
                value={props.display.progress_percent.to_string()}
                aria-label={format!("{}倒计时进度 {}%", props.title, props.display.progress_percent)}
            ></progress>
        </article>
    }
}
