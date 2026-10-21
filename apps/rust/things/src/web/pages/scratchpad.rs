//! 专项训练共用的透明手写覆盖层。保留原生 pointermove 直线绘制和 Safari Pencil 防吞笔处理。
//! 笔画历史、撤销、重做及清屏都只在浏览器内存中，不上传或影响 FSRS。
use super::*;
use serde::{Deserialize, Serialize};

const SCRATCH_WIDTH: u32 = 900;
const SCRATCH_HEIGHT: u32 = 450;
const INK_COLOR: &str = "#e11d48";
const INK_LINE_WIDTH: f64 = 2.6;

type InkPoint = (f64, f64);

fn saved_ink_key(scope: &str) -> String {
    format!("hyz-analysis-ink-{scope}")
}
fn load_persistent_ink(scope: &str) -> Vec<InkAction> {
    if scope.is_empty() || scope.len() > 120 {
        return Vec::new();
    }
    let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) else {
        return Vec::new();
    };
    let Some(data) = storage.get_item(&saved_ink_key(scope)).ok().flatten() else {
        return Vec::new();
    };
    if data.len() > 524288 {
        return Vec::new();
    }
    serde_json::from_str(&data).unwrap_or_default()
}
fn save_persistent_ink(scope: &str, actions: &[InkAction]) {
    if scope.is_empty() || scope.len() > 120 {
        return;
    }
    if let (Some(storage), Ok(data)) = (
        web_sys::window().and_then(|w| w.local_storage().ok().flatten()),
        serde_json::to_string(actions),
    ) {
        if data.len() <= 524288 {
            let _ = storage.set_item(&saved_ink_key(scope), &data);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum InkAction {
    Stroke(Vec<InkPoint>),
    Clear,
}

#[derive(Default)]
struct InkState {
    history: Vec<InkAction>,
    redo: Vec<InkAction>,
    active: Option<(i32, Vec<InkPoint>)>,
}

impl InkState {
    fn commit_active(&mut self) -> bool {
        if let Some((_, points)) = self.active.take() {
            if !points.is_empty() {
                self.history.push(InkAction::Stroke(points));
                self.redo.clear();
                return true;
            }
        }
        false
    }

    fn undo(&mut self) -> bool {
        if let Some(action) = self.history.pop() {
            self.redo.push(action);
            return true;
        }
        false
    }

    fn redo(&mut self) -> bool {
        if let Some(action) = self.redo.pop() {
            self.history.push(action);
            return true;
        }
        false
    }

    fn has_visible_ink(&self) -> bool {
        self.history
            .iter()
            .rev()
            .take_while(|action| !matches!(action, InkAction::Clear))
            .any(|action| matches!(action, InkAction::Stroke(_)))
    }

    fn clear(&mut self) -> bool {
        if !self.has_visible_ink() {
            return false;
        }
        self.history.push(InkAction::Clear);
        self.redo.clear();
        true
    }
}

fn scratch_context(canvas: &HtmlCanvasElement) -> Option<CanvasRenderingContext2d> {
    canvas
        .get_context("2d")
        .ok()
        .flatten()?
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()
}

fn scratch_clear(canvas: &HtmlCanvasElement) {
    if let Some(context) = scratch_context(canvas) {
        context.clear_rect(0.0, 0.0, SCRATCH_WIDTH as f64, SCRATCH_HEIGHT as f64);
    }
}

fn setup_pen(context: &CanvasRenderingContext2d) {
    // 不增加曲线平滑或压感，保持现有笔尖控制感。
    context.set_line_cap("round");
    context.set_line_join("round");
    context.set_line_width(INK_LINE_WIDTH);
    context.set_stroke_style(&JsValue::from_str(INK_COLOR));
}

fn paint_stroke(context: &CanvasRenderingContext2d, points: &[InkPoint]) {
    let Some(&(start_x, start_y)) = points.first() else {
        return;
    };
    setup_pen(context);
    context.begin_path();
    context.move_to(start_x, start_y);
    context.line_to(start_x + 0.1, start_y + 0.1);
    context.stroke();
    for &(x, y) in points.iter().skip(1) {
        context.line_to(x, y);
        context.stroke();
        context.begin_path();
        context.move_to(x, y);
    }
}

fn redraw_scratch(canvas: &HtmlCanvasElement, actions: &[InkAction]) {
    let Some(context) = scratch_context(canvas) else {
        return;
    };
    context.clear_rect(0.0, 0.0, SCRATCH_WIDTH as f64, SCRATCH_HEIGHT as f64);
    for action in actions {
        match action {
            InkAction::Stroke(points) => paint_stroke(&context, points),
            InkAction::Clear => {
                context.clear_rect(0.0, 0.0, SCRATCH_WIDTH as f64, SCRATCH_HEIGHT as f64);
            }
        }
    }
}

// CSS 覆盖层尺寸可随题目变化；映射到既有 900×450 坐标系。
fn scratch_point(event: &PointerEvent, canvas: &HtmlCanvasElement) -> Option<InkPoint> {
    let width = canvas.client_width();
    let height = canvas.client_height();
    if width <= 0 || height <= 0 {
        return None;
    }
    Some((
        event.offset_x() as f64 * SCRATCH_WIDTH as f64 / width as f64,
        event.offset_y() as f64 * SCRATCH_HEIGHT as f64 / height as f64,
    ))
}

#[derive(Properties, PartialEq)]
pub(crate) struct InkOverlayProps {
    pub(crate) question_index: usize,
    #[prop_or_default]
    pub(crate) persist_key: String,
    pub(crate) children: Children,
}

#[function_component(InkOverlay)]
pub(crate) fn ink_overlay(props: &InkOverlayProps) -> Html {
    let enabled = use_state(|| false);
    let revision = use_state(|| 0_u32);
    let canvas_ref = use_node_ref();
    let strokes = use_mut_ref(InkState::default);

    // Apple Pencil 快速连续落笔会被 iPadOS WebKit 的双击/手写手势识别吞掉。
    // 保留 passive:false 及单指触摸取消，不能改成 Yew 默认 passive 监听。
    {
        let canvas_ref = canvas_ref.clone();
        use_effect_with((), move |_| {
            let canvas = canvas_ref.cast::<HtmlCanvasElement>();
            let handler =
                Closure::<dyn FnMut(web_sys::TouchEvent)>::new(|event: web_sys::TouchEvent| {
                    if event.cancelable() && event.touches().length() < 2 {
                        event.prevent_default();
                    }
                });
            let mut attached = false;
            if let Some(canvas) = canvas.as_ref() {
                let options = web_sys::AddEventListenerOptions::new();
                options.set_passive(false);
                let callback = handler.as_ref().unchecked_ref();
                if canvas
                    .add_event_listener_with_callback_and_add_event_listener_options(
                        "touchstart",
                        callback,
                        &options,
                    )
                    .is_ok()
                {
                    if canvas
                        .add_event_listener_with_callback_and_add_event_listener_options(
                            "touchend", callback, &options,
                        )
                        .is_ok()
                    {
                        attached = true;
                    } else {
                        let _ = canvas.remove_event_listener_with_callback("touchstart", callback);
                    }
                }
            }
            move || {
                if attached {
                    if let Some(canvas) = canvas {
                        let callback = handler.as_ref().unchecked_ref();
                        let _ = canvas.remove_event_listener_with_callback("touchstart", callback);
                        let _ = canvas.remove_event_listener_with_callback("touchend", callback);
                    }
                }
            }
        });
    }

    {
        let canvas_ref = canvas_ref.clone();
        let strokes = strokes.clone();
        let enabled = enabled.clone();
        let revision = revision.clone();
        let scope = props.persist_key.clone();
        use_effect_with((props.question_index, scope.clone()), move |_| {
            *strokes.borrow_mut() = InkState {
                history: load_persistent_ink(&scope),
                ..InkState::default()
            };
            if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                redraw_scratch(&canvas, &strokes.borrow().history);
            }
            enabled.set(false);
            revision.set(0);
            || ()
        });
    }

    {
        let strokes = strokes.clone();
        let key = props.persist_key.clone();
        use_effect_with((*revision, key.clone()), move |_| {
            save_persistent_ink(&key, &strokes.borrow().history);
            || ()
        });
    }

    {
        let canvas_ref = canvas_ref.clone();
        let strokes = strokes.clone();
        use_effect_with(*enabled, move |opened| {
            if *opened {
                if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                    redraw_scratch(&canvas, &strokes.borrow().history);
                }
            }
            || ()
        });
    }

    let toggle = {
        let enabled = enabled.clone();
        let strokes = strokes.clone();
        let revision = revision.clone();
        Callback::from(move |_| {
            if *enabled && strokes.borrow_mut().commit_active() {
                revision.set((*revision).wrapping_add(1));
            }
            enabled.set(!*enabled);
        })
    };
    let close = {
        let enabled = enabled.clone();
        let strokes = strokes.clone();
        let revision = revision.clone();
        Callback::from(move |_| {
            if strokes.borrow_mut().commit_active() {
                revision.set((*revision).wrapping_add(1));
            }
            enabled.set(false);
        })
    };
    let undo = {
        let strokes = strokes.clone();
        let canvas_ref = canvas_ref.clone();
        let revision = revision.clone();
        Callback::from(move |_| {
            let mut state = strokes.borrow_mut();
            state.commit_active();
            if state.undo() {
                if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                    redraw_scratch(&canvas, &state.history);
                }
                revision.set((*revision).wrapping_add(1));
            }
        })
    };
    let redo = {
        let strokes = strokes.clone();
        let canvas_ref = canvas_ref.clone();
        let revision = revision.clone();
        Callback::from(move |_| {
            let mut state = strokes.borrow_mut();
            state.commit_active();
            if state.redo() {
                if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                    redraw_scratch(&canvas, &state.history);
                }
                revision.set((*revision).wrapping_add(1));
            }
        })
    };
    let clear = {
        let strokes = strokes.clone();
        let canvas_ref = canvas_ref.clone();
        let revision = revision.clone();
        Callback::from(move |_| {
            let mut state = strokes.borrow_mut();
            state.commit_active();
            if state.clear() {
                if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                    scratch_clear(&canvas);
                }
                revision.set((*revision).wrapping_add(1));
            }
        })
    };
    let on_down = {
        let canvas_ref = canvas_ref.clone();
        let strokes = strokes.clone();
        let revision = revision.clone();
        Callback::from(move |event: PointerEvent| {
            if event.pointer_type() == "mouse" && event.button() != 0 {
                return;
            }
            let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() else {
                return;
            };
            let mut state = strokes.borrow_mut();
            if let Some((previous, _)) = state.active.as_ref() {
                if event.pointer_type() != "pen" {
                    return;
                }
                if canvas.has_pointer_capture(*previous) {
                    let _ = canvas.release_pointer_capture(*previous);
                }
                // 上一个 pointerup 丢失时先入历史，后续撤销仍能恢复完整笔画。
                if state.commit_active() {
                    revision.set((*revision).wrapping_add(1));
                }
            }
            let Some((x, y)) = scratch_point(&event, &canvas) else {
                return;
            };
            let Some(context) = scratch_context(&canvas) else {
                return;
            };
            event.prevent_default();
            state.active = Some((event.pointer_id(), vec![(x, y)]));
            if event.pointer_type() != "pen" {
                let _ = canvas.set_pointer_capture(event.pointer_id());
            }
            setup_pen(&context);
            context.begin_path();
            context.move_to(x, y);
            context.line_to(x + 0.1, y + 0.1);
            context.stroke();
        })
    };
    let on_move = {
        let canvas_ref = canvas_ref.clone();
        let strokes = strokes.clone();
        Callback::from(move |event: PointerEvent| {
            let mut state = strokes.borrow_mut();
            let Some((id, points)) = state.active.as_mut() else {
                return;
            };
            if *id != event.pointer_id() {
                return;
            }
            let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() else {
                return;
            };
            let Some((x, y)) = scratch_point(&event, &canvas) else {
                return;
            };
            let Some(context) = scratch_context(&canvas) else {
                return;
            };
            event.prevent_default();
            points.push((x, y));
            context.line_to(x, y);
            context.stroke();
            context.begin_path();
            context.move_to(x, y);
        })
    };
    let on_end = {
        let canvas_ref = canvas_ref.clone();
        let strokes = strokes.clone();
        let revision = revision.clone();
        Callback::from(move |event: PointerEvent| {
            let mut state = strokes.borrow_mut();
            if state.active.as_ref().map(|(id, _)| *id) != Some(event.pointer_id()) {
                return;
            }
            if state.commit_active() {
                revision.set((*revision).wrapping_add(1));
            }
            if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                if canvas.has_pointer_capture(event.pointer_id()) {
                    let _ = canvas.release_pointer_capture(event.pointer_id());
                }
            }
        })
    };

    // Revision causes a rerender only after a complete stroke / history command.
    // Pointermove stays direct-to-Canvas and never schedules Yew render work.
    let _ = *revision;
    let has_undo = !strokes.borrow().history.is_empty();
    let has_redo = !strokes.borrow().redo.is_empty();
    let has_visible_ink = strokes.borrow().has_visible_ink();

    html! {
        <section class="calculation-scratchpad relative min-h-80 min-w-0 rounded-box bg-base-100/60" data-testid="calculation-scratchpad" oncontextmenu={Callback::from(|event: web_sys::MouseEvent| event.prevent_default())}>
            <div class="min-h-80 pt-12">{for props.children.iter()}</div>
            <button
                type="button"
                class={if *enabled { "btn btn-primary btn-sm absolute right-2 top-2 z-30" } else { "btn btn-outline btn-sm absolute right-2 top-2 z-30 bg-base-100 shadow-sm" }}
                data-testid="calculation-scratchpad-toggle"
                aria-label={if *enabled { "关闭草稿" } else { "开启草稿" }}
                aria-expanded={(*enabled).to_string()}
                onclick={toggle}
            >
                {"✎"}
            </button>
            <canvas
                ref={canvas_ref}
                width={SCRATCH_WIDTH.to_string()}
                height={SCRATCH_HEIGHT.to_string()}
                class={if *enabled { "absolute inset-0 z-20 block h-full w-full touch-none rounded-box bg-transparent" } else { "hidden" }}
                data-testid="calculation-scratchpad-canvas"
                aria-label="透明草稿层，可用 Apple Pencil 或手指书写"
                onpointerdown={on_down}
                onpointermove={on_move}
                onpointerup={on_end.clone()}
                onpointercancel={on_end.clone()}
                onlostpointercapture={on_end}
            />
            if *enabled {
                <div class="absolute bottom-2 left-2 right-2 z-30 flex flex-wrap items-center justify-center gap-1 rounded-box bg-base-100/95 p-2 shadow-lg" data-testid="calculation-scratchpad-toolbar">
                    <button type="button" class="btn btn-ghost btn-sm" data-testid="calculation-scratchpad-undo" aria-label="撤销" disabled={!has_undo} onclick={undo}>{"↶"}</button>
                    <button type="button" class="btn btn-ghost btn-sm" data-testid="calculation-scratchpad-redo" aria-label="重做" disabled={!has_redo} onclick={redo}>{"↷"}</button>
                    <span class="btn btn-primary btn-sm pointer-events-none" aria-label="铅笔已选中">{"✎ 铅笔"}</span>
                    <button type="button" class="btn btn-ghost btn-sm" data-testid="calculation-scratchpad-clear" aria-label="清屏" disabled={!has_visible_ink} onclick={clear}>{"清屏"}</button>
                    <button type="button" class="btn btn-ghost btn-sm" data-testid="calculation-scratchpad-close" aria-label="关闭手写" onclick={close}>{"✕"}</button>
                </div>
                <p class="pointer-events-none absolute left-2 top-2 z-30 rounded bg-base-100/85 px-2 py-1 text-xs text-base-content/65">{"手写中 · 关闭后可点选答案"}</p>
            }
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(id: i32) -> InkAction {
        InkAction::Stroke(vec![(id as f64, 1.0), (id as f64 + 1.0, 2.0)])
    }

    #[test]
    fn undo_and_redo_replay_strokes_in_order() {
        let mut state = InkState::default();
        state.history.push(stroke(1));
        state.history.push(stroke(2));
        assert!(state.undo());
        assert_eq!(state.history, vec![stroke(1)]);
        assert!(state.redo());
        assert_eq!(state.history, vec![stroke(1), stroke(2)]);
    }

    #[test]
    fn clear_is_undoable_and_a_new_stroke_invalidates_redo() {
        let mut state = InkState::default();
        state.history.push(stroke(1));
        assert!(state.clear());
        assert_eq!(state.history, vec![stroke(1), InkAction::Clear]);
        assert!(state.undo());
        assert_eq!(state.history, vec![stroke(1)]);
        assert!(state.redo());
        assert!(state.undo());
        state.active = Some((5, vec![(4.0, 5.0)]));
        assert!(state.commit_active());
        assert!(state.redo.is_empty());
        assert!(!state.redo());
    }

    #[test]
    fn repeated_empty_clear_is_not_added_to_history() {
        let mut state = InkState::default();
        assert!(!state.clear());
        state.history.push(stroke(1));
        assert!(state.clear());
        assert!(!state.clear());
    }
}
