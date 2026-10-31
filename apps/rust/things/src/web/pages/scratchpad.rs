//! 专项训练共用的透明手写覆盖层。保留原生 pointermove 直线绘制和 Safari Pencil 防吞笔处理。
//! 综合训练草稿保存在浏览器 IndexedDB，其余专项训练仅保留内存笔迹。
use super::*;
use serde::{Deserialize, Serialize};

const SCRATCH_WIDTH: u32 = 900;
const SCRATCH_HEIGHT: u32 = 450;
const INK_COLOR: &str = "#e11d48";
const INK_LINE_WIDTH: f64 = 2.6;

type InkPoint = (f64, f64);

#[wasm_bindgen::prelude::wasm_bindgen(module = "/src/web/ink_storage.js")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = loadInk)]
    fn load_ink(scope: &str) -> Promise;
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = saveInk)]
    fn save_ink(scope: &str, data: &str) -> Promise;
}

#[wasm_bindgen::prelude::wasm_bindgen(module = "/src/web/ink_recovery.js")]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_name = mountInkRecovery)]
    fn mount_ink_recovery(canvas: &HtmlCanvasElement, redraw: &js_sys::Function) -> js_sys::Function;
}

#[derive(Deserialize)]
struct SavedInk {
    history: Vec<InkAction>,
    persisted: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum InkSaveStatus {
    Loading,
    Saved,
    Saving,
    Unsaved,
    LoadFailed,
}

impl InkSaveStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Loading => "正在读取草稿…",
            Self::Saved => "草稿已保存到本机",
            Self::Saving => "正在保存草稿…",
            Self::Unsaved => "保存失败，仅当前页面暂存",
            Self::LoadFailed => "草稿读取失败，请重试",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum InkPane {
    Material,
    Question,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
enum InkAction {
    // Keep legacy screen-relative ink readable in existing saved sessions.
    Stroke(Vec<InkPoint>),
    // New strokes are stored in each pane's scrollable content coordinates.
    ContentStroke {
        pane: InkPane,
        points: Vec<InkPoint>,
    },
    Clear,
}

#[derive(Default)]
struct InkState {
    history: Vec<InkAction>,
    redo: Vec<InkAction>,
    active: Option<(i32, Vec<InkPoint>)>,
    active_pane: Option<InkPane>,
}

impl InkState {
    fn commit_active(&mut self) -> bool {
        if let Some((_, points)) = self.active.take() {
            let pane = self.active_pane.take();
            if !points.is_empty() {
                self.history.push(match pane {
                    Some(pane) => InkAction::ContentStroke { pane, points },
                    None => InkAction::Stroke(points),
                });
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
            .any(|action| {
                matches!(
                    action,
                    InkAction::Stroke(_) | InkAction::ContentStroke { .. }
                )
            })
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
        let _ = context.set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
        context.clear_rect(0.0, 0.0, canvas.width() as f64, canvas.height() as f64);
    }
}

fn setup_pen(context: &CanvasRenderingContext2d) {
    // Direct pointermove -> lineTo, unchanged pen feel.
    context.set_line_cap("round");
    context.set_line_join("round");
    context.set_line_width(INK_LINE_WIDTH);
    context.set_stroke_style_str(INK_COLOR);
}

// Locate both scrollable content viewports relative to the single, fixed
// workspace canvas. Stored ink is in *content* CSS pixels, not viewport pixels.
#[derive(Clone, Copy)]
struct PaneViewport {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    scroll_x: f64,
    scroll_y: f64,
}

fn pane_selector(pane: InkPane) -> &'static str {
    match pane {
        InkPane::Material => {
            "[data-testid='analysis-material-scroll'], [data-testid='logic-question-scroll'], [data-testid='quant-question-scroll']"
        }
        InkPane::Question => {
            "[data-testid='analysis-question'], [data-testid='logic-answer-scroll'], [data-testid='quant-answer-scroll']"
        }
    }
}

fn pane_viewport(canvas: &HtmlCanvasElement, pane: InkPane) -> Option<PaneViewport> {
    let document = web_sys::window()?.document()?;
    let element = document.query_selector(pane_selector(pane)).ok()??;
    let canvas_rect = canvas.get_bounding_client_rect();
    let rect = element.get_bounding_client_rect();
    Some(PaneViewport {
        x: rect.left() - canvas_rect.left(),
        y: rect.top() - canvas_rect.top(),
        width: rect.width(),
        height: rect.height(),
        scroll_x: element.scroll_left() as f64,
        scroll_y: element.scroll_top() as f64,
    })
}

fn pane_at_event(canvas: &HtmlCanvasElement, event: &PointerEvent) -> Option<InkPane> {
    let rect = canvas.get_bounding_client_rect();
    let x = event.client_x() as f64 - rect.left();
    let y = event.client_y() as f64 - rect.top();
    [InkPane::Material, InkPane::Question]
        .into_iter()
        .find(|pane| {
            pane_viewport(canvas, *pane).is_some_and(|bounds| {
                x >= bounds.x
                    && x < bounds.x + bounds.width
                    && y >= bounds.y
                    && y < bounds.y + bounds.height
            })
        })
}

fn pane_point(canvas: &HtmlCanvasElement, event: &PointerEvent, pane: InkPane) -> Option<InkPoint> {
    let bounds = pane_viewport(canvas, pane)?;
    let rect = canvas.get_bounding_client_rect();
    Some((
        event.client_x() as f64 - rect.left() - bounds.x + bounds.scroll_x,
        event.client_y() as f64 - rect.top() - bounds.y + bounds.scroll_y,
    ))
}

fn canvas_point(point: InkPoint, canvas: &HtmlCanvasElement, responsive: bool) -> InkPoint {
    if responsive {
        (
            point.0 * canvas.client_width() as f64,
            point.1 * canvas.client_height() as f64,
        )
    } else {
        point
    }
}

fn canvas_backing_size(canvas: &HtmlCanvasElement) -> (u32, u32) {
    let scale = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0)
        .clamp(1.0, 3.0);
    (
        (canvas.client_width().max(1) as f64 * scale).round() as u32,
        (canvas.client_height().max(1) as f64 * scale).round() as u32,
    )
}

fn canvas_size_changed(canvas: &HtmlCanvasElement) -> bool {
    let (width, height) = canvas_backing_size(canvas);
    canvas.width() != width || canvas.height() != height
}

fn prepare_canvas(
    canvas: &HtmlCanvasElement,
    responsive: bool,
) -> Option<CanvasRenderingContext2d> {
    if responsive {
        let (width, height) = canvas_backing_size(canvas);
        if canvas.width() != width || canvas.height() != height {
            canvas.set_width(width);
            canvas.set_height(height);
        }
    }
    let context = scratch_context(canvas)?;
    if responsive {
        let scale = canvas.width() as f64 / canvas.client_width().max(1) as f64;
        let _ = context.set_transform(scale, 0.0, 0.0, scale, 0.0, 0.0);
    }
    Some(context)
}

fn paint_stroke(
    context: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    points: &[InkPoint],
    responsive: bool,
) {
    let Some(&first) = points.first() else { return };
    let (x, y) = canvas_point(first, canvas, responsive);
    setup_pen(context);
    context.begin_path();
    context.move_to(x, y);
    context.line_to(x + 0.1, y + 0.1);
    context.stroke();
    for &point in points.iter().skip(1) {
        let (x, y) = canvas_point(point, canvas, responsive);
        context.line_to(x, y);
        context.stroke();
        context.begin_path();
        context.move_to(x, y);
    }
}

// Clip each stroke to its own pane. Both panes can scroll independently
// without ink escaping into the fixed header, the other pane or the toolbar.
fn paint_content_stroke(
    context: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    pane: InkPane,
    points: &[InkPoint],
) {
    let Some(bounds) = pane_viewport(canvas, pane) else {
        return;
    };
    let _ = context.save();
    context.begin_path();
    context.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    context.clip();
    let _ = context.translate(bounds.x - bounds.scroll_x, bounds.y - bounds.scroll_y);
    // Replaying saved strokes must use the exact same segments as live input,
    // otherwise undo/redo changes pixels where line caps overlap.
    setup_pen(context);
    if let Some(&first) = points.first() {
        context.begin_path();
        context.move_to(first.0, first.1);
        context.line_to(first.0 + 0.1, first.1 + 0.1);
        context.stroke();
    }
    for segment in points.windows(2) {
        context.begin_path();
        context.move_to(segment[0].0, segment[0].1);
        context.line_to(segment[1].0, segment[1].1);
        context.stroke();
    }
    let _ = context.restore();
    setup_pen(context);
}

// Live pointermove paints only the new segment, matching history replay
// without repeatedly stamping a new dot at the previous sample.
fn paint_content_segment(
    context: &CanvasRenderingContext2d,
    canvas: &HtmlCanvasElement,
    pane: InkPane,
    from: InkPoint,
    to: InkPoint,
) {
    let Some(bounds) = pane_viewport(canvas, pane) else {
        return;
    };
    let _ = context.save();
    context.begin_path();
    context.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    context.clip();
    let _ = context.translate(bounds.x - bounds.scroll_x, bounds.y - bounds.scroll_y);
    setup_pen(context);
    context.begin_path();
    context.move_to(from.0, from.1);
    context.line_to(to.0, to.1);
    context.stroke();
    let _ = context.restore();
    setup_pen(context);
}

fn redraw_scratch(canvas: &HtmlCanvasElement, actions: &[InkAction], responsive: bool) {
    if responsive {
        let _ = prepare_canvas(canvas, true);
    }
    scratch_clear(canvas);
    let Some(context) = prepare_canvas(canvas, responsive) else {
        return;
    };
    for action in actions {
        match action {
            InkAction::Stroke(points) => paint_stroke(&context, canvas, points, responsive),
            InkAction::ContentStroke { pane, points } => {
                paint_content_stroke(&context, canvas, *pane, points);
            }
            InkAction::Clear => {
                scratch_clear(canvas);
                let _ = prepare_canvas(canvas, responsive);
            }
        }
    }
}

// Legacy iPad ink was normalized to the entire workspace; new iPad strokes
// use pane_point() above. Calculation training keeps its 900x450 coordinates.
fn scratch_point(
    event: &PointerEvent,
    canvas: &HtmlCanvasElement,
    responsive: bool,
) -> Option<InkPoint> {
    let width = canvas.client_width();
    let height = canvas.client_height();
    if width <= 0 || height <= 0 {
        return None;
    }
    Some(if responsive {
        (
            event.offset_x() as f64 / width as f64,
            event.offset_y() as f64 / height as f64,
        )
    } else {
        (
            event.offset_x() as f64 * SCRATCH_WIDTH as f64 / width as f64,
            event.offset_y() as f64 * SCRATCH_HEIGHT as f64 / height as f64,
        )
    })
}

fn redraw_state(canvas: &HtmlCanvasElement, state: &InkState, responsive: bool) {
    redraw_scratch(canvas, &state.history, responsive);
    if responsive {
        if let (Some(pane), Some((_, points))) = (state.active_pane, state.active.as_ref()) {
            if let Some(context) = prepare_canvas(canvas, true) {
                paint_content_stroke(&context, canvas, pane, points);
            }
        }
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct InkOverlayProps {
    pub(crate) question_index: usize,
    #[prop_or_default]
    pub(crate) persist_key: String,
    #[prop_or_default]
    pub(crate) responsive: bool,
    // Content-relative coordinates are used by paper-style study views.
    // Other responsive ink overlays retain their normalized screen coordinates.
    #[prop_or_default]
    pub(crate) scroll_linked: bool,
    pub(crate) children: Children,
}

#[derive(Clone, PartialEq)]
struct InkToolbarContext {
    enabled: bool,
    ready: bool,
    has_undo: bool,
    has_redo: bool,
    has_ink: bool,
    toggle: Callback<MouseEvent>,
    undo: Callback<MouseEvent>,
    redo: Callback<MouseEvent>,
    clear: Callback<MouseEvent>,
}

const INK_ICON_BUTTON: &str = "btn btn-ghost btn-sm btn-square";

#[function_component(InkToolbarSlot)]
pub(crate) fn ink_toolbar_slot() -> Html {
    let Some(tools) = use_context::<InkToolbarContext>() else {
        return Html::default();
    };
    html! {
        <div class="relative z-30 flex shrink-0 items-center gap-1" data-testid="calculation-scratchpad-controls">
            // Keep the pen anchored at the right edge; the expanding tools grow leftwards.
            if tools.enabled {
                <div class="flex items-center gap-1" data-testid="calculation-scratchpad-toolbar">
                    <button type="button" class={INK_ICON_BUTTON} data-testid="calculation-scratchpad-undo" aria-label="撤销" disabled={!tools.has_undo} onclick={tools.undo}>{"↶"}</button>
                    <button type="button" class={INK_ICON_BUTTON} data-testid="calculation-scratchpad-redo" aria-label="重做" disabled={!tools.has_redo} onclick={tools.redo}>{"↷"}</button>
                    <button type="button" class={INK_ICON_BUTTON} data-testid="calculation-scratchpad-clear" aria-label="清空草稿" disabled={!tools.has_ink} onclick={tools.clear}>
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7h18M9 7V4h6v3M5 7l1 13h12l1-13M10 11v5m4-5v5" /></svg>
                    </button>
                </div>
            }
            <button type="button" class={if tools.enabled {"btn btn-primary btn-sm btn-square"} else {INK_ICON_BUTTON}}
                data-testid="calculation-scratchpad-toggle" aria-label={if tools.enabled {"关闭草稿"} else {"开启草稿"}}
                aria-expanded={tools.enabled.to_string()} disabled={!tools.ready} onclick={tools.toggle}>{"✎"}</button>
        </div>
    }
}

#[function_component(InkOverlay)]
pub(crate) fn ink_overlay(props: &InkOverlayProps) -> Html {
    let enabled = use_state(|| false);
    let revision = use_state(|| 0_u32);
    let canvas_ref = use_node_ref();
    let strokes = use_mut_ref(InkState::default);
    let loaded_scope = use_state(|| None::<String>);
    let save_status = use_state(|| InkSaveStatus::Loading);
    let load_attempt = use_state(|| 0_u32);

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
        let strokes = strokes.clone();
        let enabled = enabled.clone();
        let revision = revision.clone();
        let loaded_scope = loaded_scope.clone();
        let save_status = save_status.clone();
        let scope = props.persist_key.clone();
        use_effect_with(
            (props.question_index, scope.clone(), *load_attempt),
            move |_| {
                let cancelled = Rc::new(Cell::new(false));
                enabled.set(false);
                revision.set(0);
                loaded_scope.set(None);
                *strokes.borrow_mut() = InkState::default();
                if scope.is_empty() {
                    loaded_scope.set(Some(scope.clone()));
                } else {
                    save_status.set(InkSaveStatus::Loading);
                    let cancelled = cancelled.clone();
                    spawn_local(async move {
                        let result = JsFuture::from(load_ink(&scope))
                            .await
                            .ok()
                            .and_then(|value| value.as_string())
                            .and_then(|data| serde_json::from_str::<SavedInk>(&data).ok());
                        if cancelled.get() {
                            return;
                        }
                        if let Some(saved) = result {
                            strokes.borrow_mut().history = saved.history;
                            loaded_scope.set(Some(scope));
                            save_status.set(if saved.persisted {
                                InkSaveStatus::Saved
                            } else {
                                InkSaveStatus::Unsaved
                            });
                        } else {
                            save_status.set(InkSaveStatus::LoadFailed);
                        }
                    });
                }
                move || cancelled.set(true)
            },
        );
    }

    {
        let strokes = strokes.clone();
        let scope = props.persist_key.clone();
        let loaded_scope = loaded_scope.clone();
        let save_status = save_status.clone();
        let revision = *revision;
        use_effect_with((revision, scope.clone()), move |_| {
            let cancelled = Rc::new(Cell::new(false));
            if revision > 0 && !scope.is_empty() && loaded_scope.as_deref() == Some(scope.as_str())
            {
                if let Ok(data) = serde_json::to_string(&strokes.borrow().history) {
                    save_status.set(InkSaveStatus::Saving);
                    let promise = save_ink(&scope, &data);
                    let cancelled = cancelled.clone();
                    spawn_local(async move {
                        let result = JsFuture::from(promise).await;
                        if !cancelled.get() {
                            save_status.set(if result.is_ok() {
                                InkSaveStatus::Saved
                            } else {
                                InkSaveStatus::Unsaved
                            });
                        }
                    });
                }
            }
            move || cancelled.set(true)
        });
    }

    {
        let canvas_ref = canvas_ref.clone();
        let strokes = strokes.clone();
        let responsive = props.responsive;
        use_effect_with(*enabled, move |opened| {
            if *opened {
                if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                    redraw_state(&canvas, &strokes.borrow(), responsive);
                }
            }
            || ()
        });
    }

    // ResizeObserver covers pane reflow and visualViewport changes that never
    // fire window.resize on iPad. Also repaint after a foreground return even
    // when dimensions match: WebKit may have dropped the canvas backing buffer.
    {
        let responsive = props.responsive;
        let opened = *enabled;
        let canvas_ref = canvas_ref.clone();
        let strokes = strokes.clone();
        use_effect_with((responsive, opened), move |_| {
            let subscription = if responsive && opened {
                canvas_ref.cast::<HtmlCanvasElement>().map(|canvas| {
                    let callback = Closure::<dyn FnMut()>::new(move || {
                        if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                            redraw_state(&canvas, &strokes.borrow(), true);
                        }
                    });
                    let cleanup = mount_ink_recovery(&canvas, callback.as_ref().unchecked_ref());
                    (cleanup, callback)
                })
            } else {
                None
            };
            move || {
                if let Some((cleanup, _callback)) = subscription {
                    let _ = cleanup.call0(&JsValue::NULL);
                }
            }
        });
    }

    // Scroll while the pencil overlay is open: redraw at the new content offset
    // without changing pen history or triggering a Yew rerender.
    {
        let canvas_ref = canvas_ref.clone();
        let strokes = strokes.clone();
        let scroll_linked = props.scroll_linked;
        use_effect_with((scroll_linked, *enabled), move |(_, opened)| {
            let handler = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                    redraw_state(&canvas, &strokes.borrow(), true);
                }
            });
            let mut listeners = Vec::new();
            if scroll_linked && *opened {
                if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                    for pane in [InkPane::Material, InkPane::Question] {
                        if let Ok(Some(element)) = document.query_selector(pane_selector(pane)) {
                            if element
                                .add_event_listener_with_callback(
                                    "scroll",
                                    handler.as_ref().unchecked_ref(),
                                )
                                .is_ok()
                            {
                                listeners.push(element);
                            }
                        }
                    }
                }
            }
            move || {
                for element in listeners {
                    let _ = element.remove_event_listener_with_callback(
                        "scroll",
                        handler.as_ref().unchecked_ref(),
                    );
                }
            }
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
    let undo = {
        let strokes = strokes.clone();
        let canvas_ref = canvas_ref.clone();
        let revision = revision.clone();
        let responsive = props.responsive;
        Callback::from(move |_| {
            let mut state = strokes.borrow_mut();
            state.commit_active();
            if state.undo() {
                if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                    redraw_scratch(&canvas, &state.history, responsive);
                }
                revision.set((*revision).wrapping_add(1));
            }
        })
    };
    let redo = {
        let strokes = strokes.clone();
        let canvas_ref = canvas_ref.clone();
        let revision = revision.clone();
        let responsive = props.responsive;
        Callback::from(move |_| {
            let mut state = strokes.borrow_mut();
            state.commit_active();
            if state.redo() {
                if let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() {
                    redraw_scratch(&canvas, &state.history, responsive);
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
        let responsive = props.responsive;
        let scroll_linked = props.scroll_linked;
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
            let (point, pane) = if scroll_linked {
                let Some(pane) = pane_at_event(&canvas, &event) else {
                    return;
                };
                let Some(point) = pane_point(&canvas, &event, pane) else {
                    return;
                };
                (point, Some(pane))
            } else {
                let Some(point) = scratch_point(&event, &canvas, responsive) else {
                    return;
                };
                (point, None)
            };
            // Resizing the backing store erases pixels: restore all completed
            // strokes before painting the new pen-down sample.
            if responsive && canvas_size_changed(&canvas) {
                redraw_state(&canvas, &state, responsive);
            }
            let Some(context) = prepare_canvas(&canvas, responsive) else {
                return;
            };
            event.prevent_default();
            state.active_pane = pane;
            state.active = Some((event.pointer_id(), vec![point]));
            if event.pointer_type() != "pen" {
                let _ = canvas.set_pointer_capture(event.pointer_id());
            }
            if let Some(pane) = pane {
                paint_content_stroke(&context, &canvas, pane, &[point]);
            } else {
                let (x, y) = canvas_point(point, &canvas, responsive);
                setup_pen(&context);
                context.begin_path();
                context.move_to(x, y);
                context.line_to(x + 0.1, y + 0.1);
                context.stroke();
            }
        })
    };
    let on_move = {
        let canvas_ref = canvas_ref.clone();
        let strokes = strokes.clone();
        let responsive = props.responsive;
        let scroll_linked = props.scroll_linked;
        Callback::from(move |event: PointerEvent| {
            let mut state = strokes.borrow_mut();
            let Some(canvas) = canvas_ref.cast::<HtmlCanvasElement>() else {
                return;
            };
            if responsive && canvas_size_changed(&canvas) {
                redraw_state(&canvas, &state, responsive);
            }
            let pane = state.active_pane;
            let Some((id, points)) = state.active.as_mut() else {
                return;
            };
            if *id != event.pointer_id() {
                return;
            }
            let point = if scroll_linked {
                let Some(pane) = pane else {
                    return;
                };
                pane_point(&canvas, &event, pane)
            } else {
                scratch_point(&event, &canvas, responsive)
            };
            let Some(point) = point else {
                return;
            };
            let Some(context) = prepare_canvas(&canvas, responsive) else {
                return;
            };
            event.prevent_default();
            let previous = points.last().copied().unwrap_or(point);
            points.push(point);
            if let Some(pane) = pane {
                paint_content_segment(&context, &canvas, pane, previous, point);
            } else {
                let (x, y) = canvas_point(point, &canvas, responsive);
                context.line_to(x, y);
                context.stroke();
                context.begin_path();
                context.move_to(x, y);
            }
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
    let ink_ready =
        props.persist_key.is_empty() || loaded_scope.as_deref() == Some(props.persist_key.as_str());
    let retry = {
        let load_attempt = load_attempt.clone();
        let revision = revision.clone();
        let save_status = save_status.clone();
        Callback::from(move |_| {
            if *save_status == InkSaveStatus::LoadFailed {
                load_attempt.set((*load_attempt).wrapping_add(1));
            } else {
                revision.set((*revision).wrapping_add(1));
            }
        })
    };

    let tools = InkToolbarContext {
        enabled: *enabled,
        ready: ink_ready,
        has_undo,
        has_redo,
        has_ink: has_visible_ink,
        toggle,
        undo,
        redo,
        clear,
    };
    html! {
        <ContextProvider<bool> context={*enabled}>
        <ContextProvider<InkToolbarContext> context={tools}>
        <section class={if props.responsive { "calculation-scratchpad relative flex-1 min-h-0 min-w-0 h-full rounded-box" } else { "calculation-scratchpad relative min-h-80 min-w-0 rounded-box bg-base-100/60" }} data-testid="calculation-scratchpad" oncontextmenu={Callback::from(|event: web_sys::MouseEvent| event.prevent_default())}>
            <div class={if props.responsive { "h-full min-h-0 min-w-0" } else { "min-h-80 pt-12" }}>{for props.children.iter()}</div>
            if !props.persist_key.is_empty() {
                <div class="absolute bottom-2 left-2 z-30 max-w-[calc(100%-1rem)] rounded bg-base-100/95 px-2 py-1 text-xs shadow-sm">
                    <span role="status" data-testid="calculation-scratchpad-save-status">{save_status.label()}</span>
                    if matches!(*save_status, InkSaveStatus::Unsaved | InkSaveStatus::LoadFailed) {
                        <button type="button" class="btn btn-ghost btn-xs ml-1" onclick={retry}>{"重试保存"}</button>
                    }
                </div>
            }
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
        </section>
        </ContextProvider<InkToolbarContext>>
        </ContextProvider<bool>>
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
