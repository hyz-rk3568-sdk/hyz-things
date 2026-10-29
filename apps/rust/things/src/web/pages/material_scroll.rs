use gloo_timers::callback::{Interval, Timeout};
use wasm_bindgen::{closure::Closure, JsCast};
use web_sys::{Event, EventTarget, MouseEvent, PointerEvent};
use yew::prelude::*;

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct ScrollEdges {
    pub(super) can_scroll_up: bool,
    pub(super) can_scroll_down: bool,
}

pub(super) fn scroll_edges(element: &web_sys::HtmlElement) -> ScrollEdges {
    let max_scroll = (element.scroll_height() - element.client_height()).max(0);
    let scroll_top = element.scroll_top().clamp(0, max_scroll);
    ScrollEdges {
        can_scroll_up: scroll_top > 0,
        can_scroll_down: scroll_top < max_scroll,
    }
}

const MATERIAL_HOLD_DELAY_MS: u32 = 300;
const MATERIAL_REPEAT_MS: u32 = 80;

#[derive(Default)]
struct MaterialScrollHold {
    pointer_id: Option<i32>,
    suppress_click: bool,
    delay: Option<Timeout>,
    repeat: Option<Interval>,
}

impl MaterialScrollHold {
    fn stop(&mut self) {
        self.pointer_id = None;
        self.delay = None;
        self.repeat = None;
    }

    fn finish_pointer(&mut self, pointer_id: i32, cancelled: bool) {
        if self.pointer_id == Some(pointer_id) {
            self.suppress_click |= cancelled;
            self.stop();
        }
    }
}

fn advance_material_line(passage: &NodeRef, up: bool) -> bool {
    let Some(element) = passage.cast::<web_sys::HtmlElement>() else {
        return false;
    };
    let max_scroll = (element.scroll_height() - element.client_height()).max(0);
    let top = element.scroll_top().clamp(0, max_scroll);
    element.set_scroll_top((top + if up { -32 } else { 32 }).clamp(0, max_scroll));
    if up {
        element.scroll_top() > 0
    } else {
        element.scroll_top() < max_scroll
    }
}

#[derive(Properties, PartialEq)]
pub(super) struct MaterialScrollButtonProps {
    pub(super) passage: NodeRef,
    pub(super) up: bool,
    pub(super) disabled: bool,
    pub(super) question_index: usize,
    #[prop_or("材料")]
    pub(super) target: &'static str,
}

#[function_component(MaterialScrollButton)]
pub(super) fn material_scroll_button(props: &MaterialScrollButtonProps) -> Html {
    let hold = use_mut_ref(MaterialScrollHold::default);
    let button = use_node_ref();
    {
        let hold = hold.clone();
        use_effect_with((props.disabled, props.question_index), move |_| {
            hold.borrow_mut().stop();
            move || hold.borrow_mut().stop()
        });
    }
    {
        let hold = hold.clone();
        use_effect_with((), move |_| {
            let event_hold = hold.clone();
            let handler = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
                let mut state = event_hold.borrow_mut();
                if state.pointer_id.is_none() {
                    return;
                }
                if let Some(pointer) = event.dyn_ref::<PointerEvent>() {
                    state.finish_pointer(pointer.pointer_id(), event.type_() == "pointercancel");
                    return;
                } else {
                    if event.type_() == "visibilitychange"
                        && !web_sys::window()
                            .and_then(|w| w.document())
                            .is_some_and(|d| d.hidden())
                    {
                        return;
                    }
                    state.suppress_click = true;
                }
                state.stop();
            });
            let mut listeners: Vec<(EventTarget, &str)> = Vec::new();
            if let Some(window) = web_sys::window() {
                for name in ["pointerup", "pointercancel", "blur"] {
                    if window
                        .add_event_listener_with_callback(name, handler.as_ref().unchecked_ref())
                        .is_ok()
                    {
                        listeners.push((window.clone().into(), name));
                    }
                }
                if let Some(document) = window.document() {
                    if document
                        .add_event_listener_with_callback(
                            "visibilitychange",
                            handler.as_ref().unchecked_ref(),
                        )
                        .is_ok()
                    {
                        listeners.push((document.into(), "visibilitychange"));
                    }
                }
            }
            move || {
                hold.borrow_mut().stop();
                for (target, name) in listeners {
                    let _ = target.remove_event_listener_with_callback(
                        name,
                        handler.as_ref().unchecked_ref(),
                    );
                }
            }
        });
    }
    let on_down = {
        let button = button.clone();
        let hold = hold.clone();
        let passage = props.passage.clone();
        let up = props.up;
        let disabled = props.disabled;
        Callback::from(move |event: PointerEvent| {
            if disabled || event.button() != 0 || !event.is_primary() {
                return;
            }
            {
                let mut state = hold.borrow_mut();
                state.stop();
                state.suppress_click = false;
                state.pointer_id = Some(event.pointer_id());
            }
            if let Some(target) = button.cast::<web_sys::HtmlElement>() {
                let _ = target.set_pointer_capture(event.pointer_id());
            }
            let weak = std::rc::Rc::downgrade(&hold);
            let passage = passage.clone();
            hold.borrow_mut().delay = Some(Timeout::new(MATERIAL_HOLD_DELAY_MS, move || {
                let Some(hold) = weak.upgrade() else {
                    return;
                };
                hold.borrow_mut().suppress_click = true;
                if !advance_material_line(&passage, up) {
                    hold.borrow_mut().stop();
                    return;
                }
                let weak = std::rc::Rc::downgrade(&hold);
                hold.borrow_mut().repeat = Some(Interval::new(MATERIAL_REPEAT_MS, move || {
                    if let Some(hold) = weak.upgrade() {
                        if !advance_material_line(&passage, up) {
                            hold.borrow_mut().stop();
                        }
                    }
                }));
            }));
        })
    };
    let on_up = {
        let hold = hold.clone();
        Callback::from(move |event: PointerEvent| {
            hold.borrow_mut().finish_pointer(event.pointer_id(), false);
        })
    };
    let on_cancel = {
        let hold = hold.clone();
        Callback::from(move |event: PointerEvent| {
            hold.borrow_mut().finish_pointer(event.pointer_id(), true);
        })
    };
    let on_click = {
        let hold = hold.clone();
        let passage = props.passage.clone();
        let up = props.up;
        Callback::from(move |event: MouseEvent| {
            let suppress = std::mem::take(&mut hold.borrow_mut().suppress_click);
            // Keyboard activation has no pointer press and must remain usable after cancellation.
            if !suppress || event.detail() == 0 {
                advance_material_line(&passage, up);
            }
        })
    };
    let label = format!(
        "{}向{}滚动",
        props.target,
        if props.up { "上" } else { "下" }
    );
    html! {
        // Disabled buttons ignore hit testing; keep their gesture shield interactive.
        <span class="inline-flex touch-none select-none [-webkit-touch-callout:none]"
            onpointerdown={Callback::from(|event: PointerEvent| {
                event.prevent_default();
                event.stop_propagation();
            })}
            oncontextmenu={Callback::from(|event: MouseEvent| event.prevent_default())}>
            <button ref={button} type="button" class="btn btn-ghost btn-sm touch-none select-none px-2 [-webkit-touch-callout:none]"
                aria-label={label} title="单击滚动一行，长按连续滚动" disabled={props.disabled}
                onpointerdown={on_down} onpointerup={on_up} onpointercancel={on_cancel.clone()}
                onlostpointercapture={on_cancel} onclick={on_click}>
                {if props.up { "↑" } else { "↓" }}
            </button>
        </span>
    }
}
