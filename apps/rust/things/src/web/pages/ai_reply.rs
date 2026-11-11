use super::*;
use crate::bindings::browser_bridge::FloatingMenu;

#[derive(Properties, PartialEq)]
pub(crate) struct AiReplyProps {
    pub content: String,
}

#[function_component(AiReply)]
pub(crate) fn ai_reply(props: &AiReplyProps) -> Html {
    let showing_source = use_state(|| false);
    let menu_open = use_state(|| false);
    let notice = use_state(|| None::<String>);
    let trigger_ref = use_node_ref();
    let menu_ref = use_node_ref();
    {
        let trigger_ref = trigger_ref.clone();
        let menu_ref = menu_ref.clone();
        let notice = notice.clone();
        use_effect_with(*menu_open, move |open| {
            let active = Rc::new(Cell::new(true));
            let mounted = Rc::new(RefCell::new(None::<FloatingMenu>));
            if *open {
                if let (Some(trigger), Some(menu)) = (
                    trigger_ref.cast::<HtmlElement>(),
                    menu_ref.cast::<HtmlElement>(),
                ) {
                    let active = active.clone();
                    let mounted = mounted.clone();
                    spawn_local(async move {
                        match FloatingMenu::mount(&trigger, &menu).await {
                            Ok(handle) if active.get() => *mounted.borrow_mut() = Some(handle),
                            Ok(_) => (),
                            Err(_) if active.get() => notice.set(Some("菜单定位失败".into())),
                            Err(_) => (),
                        }
                    });
                }
            }
            move || {
                active.set(false);
                mounted.borrow_mut().take();
            }
        });
    }
    let toggle = {
        let showing_source = showing_source.clone();
        let menu_open = menu_open.clone();
        Callback::from(move |_| {
            showing_source.set(!*showing_source);
            menu_open.set(false);
        })
    };
    let copy = {
        let content = props.content.clone();
        let menu_open = menu_open.clone();
        let notice = notice.clone();
        Callback::from(move |_| {
            menu_open.set(false);
            let Some(window) = web_sys::window() else {
                notice.set(Some("复制失败：浏览器不可用".into()));
                return;
            };
            let promise = window.navigator().clipboard().write_text(&content);
            let notice = notice.clone();
            spawn_local(async move {
                let message = if wasm_bindgen_futures::JsFuture::from(promise).await.is_ok() {
                    "源码已复制"
                } else {
                    "复制失败，请检查浏览器剪贴板权限"
                };
                notice.set(Some(message.into()));
            });
        })
    };
    html! {
        <div>
            if *showing_source {
                <pre class="whitespace-pre-wrap break-words text-sm" data-testid="question-bank-ai-source">{props.content.clone()}</pre>
            }
            <div hidden={*showing_source} class="study-markdown text-base leading-7" data-ai-diagram-source="true">
                {render_question_bank_markdown(&props.content)}
            </div>
            <div class="relative flex justify-end items-center gap-2">
                if let Some(message) = notice.as_ref() {
                    <span class="text-xs text-base-content/60" role="status">{message.clone()}</span>
                }
                <button type="button" class="btn btn-ghost btn-sm btn-circle" aria-label="回复操作"
                    ref={trigger_ref} aria-expanded={menu_open.to_string()}
                    onclick={Callback::from({ let menu_open = menu_open.clone(); move |_| menu_open.set(!*menu_open) })}>
                    {"⋮"}
                </button>
                if *menu_open {
                    <div ref={menu_ref} data-testid="ai-reply-menu" class="absolute right-0 bottom-full z-10 min-w-32 rounded-box bg-base-100 p-1 shadow-lg border border-base-content/10">
                        <button type="button" class="btn btn-ghost btn-sm w-full justify-start" onclick={toggle}>
                            {if *showing_source { "查看解析" } else { "查看源码" }}
                        </button>
                        <button type="button" class="btn btn-ghost btn-sm w-full justify-start" onclick={copy}>{"复制源码"}</button>
                    </div>
                }
            </div>
        </div>
    }
}
