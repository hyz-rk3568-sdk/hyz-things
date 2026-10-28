use super::*;
use wasm_bindgen::{closure::Closure, prelude::*, JsCast};

#[wasm_bindgen(module = "/src/web/pwa.js")]
extern "C" {
    #[wasm_bindgen(js_name = mountPwa)]
    fn mount_pwa(root: &web_sys::Element, flush: &js_sys::Function) -> js_sys::Function;
}

#[wasm_bindgen(module = "/src/web/ink_storage.js")]
extern "C" {
    #[wasm_bindgen(js_name = flushInk)]
    fn flush_ink() -> js_sys::Promise;
}

#[function_component(PwaControls)]
pub(crate) fn pwa_controls() -> Html {
    let root = use_node_ref();
    {
        let root = root.clone();
        use_effect_with((), move |_| {
            let flush = Closure::<dyn FnMut() -> js_sys::Promise>::new(flush_ink);
            let cleanup = root
                .cast::<web_sys::Element>()
                .map(|element| mount_pwa(&element, flush.as_ref().unchecked_ref()));
            move || {
                if let Some(cleanup) = cleanup {
                    let _ = cleanup.call0(&JsValue::NULL);
                }
                drop(flush);
            }
        });
    }
    html! {
        <section ref={root} aria-label="应用更新" class="flex min-w-0 max-w-full flex-wrap items-center gap-2 py-2 text-sm">
            <button type="button" class="btn btn-sm btn-ghost" data-pwa-reload="">{"重新加载"}</button>
            <button type="button" class="btn btn-sm btn-primary" data-pwa-update="" hidden=true>{"更新版本"}</button>
            <span role="status" aria-live="polite" class="min-w-0 break-words" data-pwa-status=""></span>
        </section>
    }
}
