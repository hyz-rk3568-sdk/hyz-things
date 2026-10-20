//! 速算练习的本地临时手写草稿区。只写 Canvas，不提交服务端或题目状态。
use super::*;

const SCRATCH_WIDTH: u32 = 900;
const SCRATCH_HEIGHT: u32 = 450;

fn scratch_context(canvas: &HtmlCanvasElement) -> Option<CanvasRenderingContext2d> {
    canvas
        .get_context("2d")
        .ok()
        .flatten()?
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()
}

fn clear_scratch(canvas: &HtmlCanvasElement) {
    if let Some(context) = scratch_context(canvas) {
        context.clear_rect(0.0, 0.0, SCRATCH_WIDTH as f64, SCRATCH_HEIGHT as f64);
    }
}

// CSS 缩放后的指针位置映射回固定 Canvas 像素坐标，手机和 iPad 同样适用。
fn scratch_point(event: &PointerEvent, canvas: &HtmlCanvasElement) -> Option<(f64, f64)> {
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
pub(crate) struct CalculationScratchpadProps {
    pub(crate) question_index: usize,
}

#[function_component(CalculationScratchpad)]
pub(crate) fn calculation_scratchpad(props: &CalculationScratchpadProps) -> Html {
    let expanded = use_state(|| true);
    let canvas_ref = use_node_ref();
    let active_pointer = use_mut_ref(|| None::<i32>);

    // Apple Pencil 快速连续落笔会被 iPadOS WebKit 的双击/手写手势识别吞掉，
    // 第二笔甚至没有 pointerdown 事件。必须在原生触摸事件层阻止单指手势。
    // Yew 的委托触摸监听可能是 passive，故直接注册 { passive: false }。
    {
        let canvas_ref = canvas_ref.clone();
        use_effect_with((), move |_| {
            let canvas = canvas_ref.cast::<HtmlCanvasElement>();
            let handler =
                Closure::<dyn FnMut(web_sys::TouchEvent)>::new(|event: web_sys::TouchEvent| {
                    // 两指手势仍交由浏览器处理；不可取消的事件不强行处理。
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
                // handler 随 effect cleanup 释放，避免切题/离开页面后遗留监听。
            }
        });
    }

    {
        let canvas_ref = canvas_ref.clone();
        let active_pointer = active_pointer.clone();
        use_effect_with(props.question_index, move |_| {
            *active_pointer.borrow_mut() = None;
            if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                clear_scratch(&canvas);
            }
            || ()
        });
    }

    let toggle = {
        let expanded = expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };
    let clear = {
        let canvas_ref = canvas_ref.clone();
        let active_pointer = active_pointer.clone();
        Callback::from(move |_| {
            *active_pointer.borrow_mut() = None;
            if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                clear_scratch(&canvas);
            }
        })
    };
    let on_down = {
        let canvas_ref = canvas_ref.clone();
        let active_pointer = active_pointer.clone();
        Callback::from(move |event: PointerEvent| {
            if event.pointer_type() == "mouse" && event.button() != 0 {
                return;
            }
            let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() else {
                return;
            };
            // Safari 在 Apple Pencil 连续起笔时可能跳过上一笔的终止事件。
            // 新的 pen pointerdown 必须能接管遗留指针，否则第二个字会写不出来。
            if let Some(previous) = *active_pointer.borrow() {
                if event.pointer_type() != "pen" {
                    return;
                }
                if canvas.has_pointer_capture(previous) {
                    let _ = canvas.release_pointer_capture(previous);
                }
            }
            let Some((x, y)) = scratch_point(&event, &canvas) else {
                return;
            };
            let Some(context) = scratch_context(&canvas) else {
                return;
            };
            event.prevent_default();
            *active_pointer.borrow_mut() = Some(event.pointer_id());
            // iPad Safari 对 Pencil 的 pointer capture/释放顺序不够稳定；
            // pen 在画布内正常收事件，无需主动 capture。手指/鼠标仍保留 capture。
            if event.pointer_type() != "pen" {
                let _ = canvas.set_pointer_capture(event.pointer_id());
            }
            context.set_line_cap("round");
            context.set_line_join("round");
            context.set_line_width(3.8);
            context.set_stroke_style(&JsValue::from_str("#f8fafc"));
            context.begin_path();
            context.move_to(x, y);
            context.line_to(x + 0.1, y + 0.1);
            context.stroke();
        })
    };
    let on_move = {
        let canvas_ref = canvas_ref.clone();
        let active_pointer = active_pointer.clone();
        Callback::from(move |event: PointerEvent| {
            if *active_pointer.borrow() != Some(event.pointer_id()) {
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
            context.line_to(x, y);
            context.stroke();
            context.begin_path();
            context.move_to(x, y);
        })
    };
    let on_end = {
        let canvas_ref = canvas_ref.clone();
        let active_pointer = active_pointer.clone();
        Callback::from(move |event: PointerEvent| {
            if *active_pointer.borrow() != Some(event.pointer_id()) {
                return;
            }
            *active_pointer.borrow_mut() = None;
            if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                if canvas.has_pointer_capture(event.pointer_id()) {
                    let _ = canvas.release_pointer_capture(event.pointer_id());
                }
            }
        })
    };

    html! {
        <section class="calculation-scratchpad rounded-box border border-base-content/10 bg-base-100/60 p-3" data-testid="calculation-scratchpad" oncontextmenu={Callback::from(|event: web_sys::MouseEvent| event.prevent_default())}>
            <div class="flex items-center justify-between gap-3">
                <button
                    type="button"
                    class="btn btn-ghost btn-sm"
                    data-testid="calculation-scratchpad-toggle"
                    aria-expanded={(*expanded).to_string()}
                    onclick={toggle}
                >
                    {if *expanded { "✎ 收起草稿" } else { "✎ 展开草稿" }}
                </button>
                if *expanded {
                    <button
                        type="button"
                        class="btn btn-ghost btn-sm"
                        data-testid="calculation-scratchpad-clear"
                        onclick={clear}
                    >
                        {"清空"}
                    </button>
                }
            </div>
            <div class={if *expanded { "mt-2" } else { "hidden" }}>
                <canvas
                    ref={canvas_ref}
                    width={SCRATCH_WIDTH.to_string()}
                    height={SCRATCH_HEIGHT.to_string()}
                    class="block h-64 w-full touch-none rounded-lg bg-slate-950 sm:h-80 lg:h-96"
                    data-testid="calculation-scratchpad-canvas"
                    aria-label="草稿区，可用手指或触控笔书写"
                    onpointerdown={on_down}
                    onpointermove={on_move}
                    onpointerup={on_end.clone()}
                    onpointercancel={on_end.clone()}
                    onlostpointercapture={on_end}
                />
                <p class="mt-2 text-xs text-base-content/60">{"手指或触控笔直接写，草稿只留在本题；切题自动清空。"}</p>
            </div>
        </section>
    }
}
