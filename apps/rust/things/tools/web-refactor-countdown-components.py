#!/usr/bin/env python3
"""Extract countdown presentation into shared components without moving behavior.

The countdown hook continues to own timers, persistence, PiP/browser effects, and
all state transitions. Components receive immutable display values and callbacks.
This migration is deterministic and idempotent and also finishes the Tailscale
SectionCard conversion from the shared-presentation stage.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WEB = ROOT / "apps/rust/things/src/web"
HOOK = WEB / "hooks/countdown.rs"
COMPONENTS = WEB / "components"
COMPONENTS_MOD = COMPONENTS / "mod.rs"
TAILSCALE = WEB / "pages/tailscale.rs"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if new in text:
        return text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


def remove_between(text: str, start_marker: str, end_marker: str, label: str) -> str:
    if start_marker not in text:
        return text
    start = text.index(start_marker)
    end = text.find(end_marker, start)
    if end < 0:
        raise SystemExit(f"{label}: end marker not found")
    return text[:start] + text[end:]


def write_countdown_components() -> None:
    (COMPONENTS / "countdown_card.rs").write_text(
        '''use super::*;

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
'''
    )

    (COMPONENTS / "countdown_editor.rs").write_text(
        '''use super::*;

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
'''
    )


def update_components_mod() -> None:
    text = COMPONENTS_MOD.read_text()
    if "mod countdown_card;" not in text:
        text = replace_once(
            text,
            "mod feedback;\n",
            "mod countdown_card;\nmod countdown_editor;\nmod feedback;\n",
            "countdown component module declarations",
        )
    if "pub(crate) use countdown_card::*;" not in text:
        text = replace_once(
            text,
            "pub(crate) use feedback::*;\n",
            "pub(crate) use countdown_card::*;\npub(crate) use countdown_editor::*;\npub(crate) use feedback::*;\n",
            "countdown component exports",
        )
    COMPONENTS_MOD.write_text(text)


def update_countdown_hook() -> None:
    text = HOOK.read_text()

    text = remove_between(
        text,
        "pub(crate) fn render_countdown_values(",
        "pub(crate) fn custom_duration_input_callback(",
        "countdown values renderer",
    )
    text = remove_between(
        text,
        "pub(crate) fn render_custom_timer_card(",
        "pub(crate) fn render_countdown_card(",
        "custom countdown card renderer",
    )
    text = remove_between(
        text,
        "pub(crate) fn render_countdown_card(",
        "pub(crate) fn render_exam_countdown_card(",
        "exam countdown card renderer",
    )

    adapter_marker = "pub(crate) fn countdown_display(snapshot: CountdownSnapshot) -> CountdownDisplay"
    if adapter_marker not in text:
        start_marker = "pub(crate) fn render_exam_countdown_card("
        end_marker = "#[function_component(CustomCountdownPanel)]"
        start = text.find(start_marker)
        end = text.find(end_marker, start if start >= 0 else 0)
        if start < 0 or end < 0:
            raise SystemExit("exam countdown adapter: migration anchors not found")
        adapter = '''pub(crate) fn countdown_display(snapshot: CountdownSnapshot) -> CountdownDisplay {
    CountdownDisplay {
        remaining_seconds: snapshot.remaining_seconds,
        days: snapshot.days,
        hours: snapshot.hours,
        minutes: snapshot.minutes,
        seconds: snapshot.seconds,
        progress_percent: snapshot.progress_percent,
        finished: snapshot.finished,
    }
}

pub(crate) fn render_exam_countdown_card(
    target: &ExamCountdownTarget,
    snapshot: CountdownSnapshot,
    index: usize,
    on_double_click: Callback<MouseEvent>,
) -> Html {
    let status = exam_status_label(snapshot);
    let tone = exam_card_tone(snapshot);
    let card_label = if snapshot.finished {
        format!("{}，考试已结束", target.title)
    } else {
        format!("{}，距离考试 {} 天", target.title, snapshot.days)
    };
    html! {
        <CountdownCard
            id={target.id}
            title={target.title}
            eyebrow={target.eyebrow}
            target_iso={target.target_iso}
            target_label={target.target_label}
            target_note={target.target_note}
            display={countdown_display(snapshot)}
            index={index}
            status={status}
            tone={classes!(tone)}
            card_label={card_label}
            finished_label="考试日已过"
            progress_label="年度备考进度"
            progress_aria_label={format!("{}冲刺进度 {}%", target.title, snapshot.progress_percent)}
            on_double_click={on_double_click}
        />
    }
}

'''
        text = text[:start] + adapter + text[end:]

    if "<CountdownEditor" not in text:
        call_marker = "            render_custom_timer_card(\n"
        start = text.find(call_marker)
        if start < 0:
            raise SystemExit("custom countdown editor call: migration anchor not found")
        call_end_marker = "\n            )\n"
        end = text.find(call_end_marker, start)
        if end < 0:
            raise SystemExit("custom countdown editor call: closing anchor not found")
        end += len("\n            )")
        replacement = '''            let active = state.timer.is_some();
            let can_pause = state.timer.is_some_and(|timer| !timer.paused) && !snapshot.finished;
            let can_resume = state.timer.is_some_and(|timer| timer.paused) && !snapshot.finished;
            let card_label = if snapshot.finished && active {
                format!("{}倒计时已结束", target.title)
            } else if state.timer.is_some_and(|timer| timer.paused) {
                format!(
                    "{}倒计时已暂停，剩余 {} 秒",
                    target.title, snapshot.remaining_seconds
                )
            } else if active {
                format!(
                    "{}倒计时，剩余 {} 秒",
                    target.title, snapshot.remaining_seconds
                )
            } else {
                format!("{}倒计时，待开始", target.title)
            };
            let start_label = if active {
                format!("重新开始{}倒计时", target.title)
            } else {
                format!("开始{}倒计时", target.title)
            };

            html! {
                <CountdownEditor
                    id={target.id}
                    title={target.title}
                    display={countdown_display(snapshot)}
                    status={status}
                    tone={classes!(tone)}
                    hours={hours}
                    minutes={minutes}
                    seconds={seconds}
                    active={active}
                    can_pause={can_pause}
                    can_resume={can_resume}
                    card_label={card_label}
                    start_label={start_label}
                    on_hours={set_hours}
                    on_minutes={set_minutes}
                    on_seconds={set_seconds}
                    on_restart={restart}
                    on_pause={pause}
                    on_resume={resume}
                    on_open_pip={open_pip}
                />
            }'''
        text = text[:start] + replacement + text[end:]

    HOOK.write_text(text)


def update_tailscale_sections() -> None:
    text = TAILSCALE.read_text()
    text = replace_once(
        text,
        '<section class={SECTION} aria-labelledby="tailscale-title">',
        '<SectionCard title_id="tailscale-title">',
        "Tailscale loading section opening",
    )
    text = replace_once(
        text,
        '''                <div class={SETTINGS_EMPTY} role="status">{"正在读取 Tailscale 状态…"}</div>
            </section>
        };
''',
        '''                <div class={SETTINGS_EMPTY} role="status">{"正在读取 Tailscale 状态…"}</div>
            </SectionCard>
        };
''',
        "Tailscale loading section closing",
    )
    text = replace_once(
        text,
        '<section class={SECTION} aria-labelledby="tailscale-title" aria-busy={busy.to_string()}>',
        '<SectionCard title_id="tailscale-title" busy={Some(busy)}>',
        "Tailscale control section opening",
    )
    text = replace_once(
        text,
        '''                <small class={HELP_TEXT}>{"仅支持固定 RouterOnly / LAN Access 安全模式；浏览器不能输入 URL、auth key、子网、端口或控制参数。"}</small>
            </article>
        </section>
    }
}
''',
        '''                <small class={HELP_TEXT}>{"仅支持固定 RouterOnly / LAN Access 安全模式；浏览器不能输入 URL、auth key、子网、端口或控制参数。"}</small>
            </article>
        </SectionCard>
    }
}
''',
        "Tailscale control section closing",
    )
    TAILSCALE.write_text(text)


def validate() -> None:
    hook = HOOK.read_text()
    components_mod = COMPONENTS_MOD.read_text()
    tailscale = TAILSCALE.read_text()

    for path in [COMPONENTS / "countdown_card.rs", COMPONENTS / "countdown_editor.rs"]:
        if not path.exists():
            raise SystemExit(f"missing countdown component: {path.relative_to(ROOT)}")
        content = path.read_text()
        forbidden = [
            "UseStateHandle",
            "AppState",
            "CustomCountdownState",
            "web_sys::",
            "local_storage",
            "PreparedVideoPip",
            "PipCountdownConfig",
            "spawn_local",
        ]
        leaked = [name for name in forbidden if name in content]
        if leaked:
            raise SystemExit(
                f"{path.relative_to(ROOT)} owns behavior/state dependencies: {', '.join(leaked)}"
            )

    for marker in [
        "mod countdown_card;",
        "mod countdown_editor;",
        "pub(crate) use countdown_card::*;",
        "pub(crate) use countdown_editor::*;",
    ]:
        if marker not in components_mod:
            raise SystemExit(f"components/mod.rs missing countdown wiring: {marker}")

    forbidden_renderers = [
        "pub(crate) fn render_countdown_values(",
        "pub(crate) fn render_custom_timer_card(",
        "pub(crate) fn render_countdown_card(",
    ]
    leaked_renderers = [marker for marker in forbidden_renderers if marker in hook]
    if leaked_renderers:
        raise SystemExit(f"countdown hook still owns card markup: {leaked_renderers}")
    for marker in [adapter_marker := "pub(crate) fn countdown_display(snapshot: CountdownSnapshot) -> CountdownDisplay", "<CountdownEditor", "<CountdownCard"]:
        if marker not in hook:
            raise SystemExit(f"countdown component migration incomplete: {marker}")

    if '<section class={SECTION} aria-labelledby="tailscale-title"' in tailscale:
        raise SystemExit("Tailscale still owns a direct outer SECTION wrapper")
    for marker in [
        '<SectionCard title_id="tailscale-title">',
        '<SectionCard title_id="tailscale-title" busy={Some(busy)}>',
    ]:
        if marker not in tailscale:
            raise SystemExit(f"Tailscale SectionCard migration incomplete: {marker}")


if __name__ == "__main__":
    write_countdown_components()
    update_components_mod()
    update_countdown_hook()
    update_tailscale_sections()
    validate()
    print("extracted CountdownCard/CountdownEditor and completed Tailscale SectionCard migration")
