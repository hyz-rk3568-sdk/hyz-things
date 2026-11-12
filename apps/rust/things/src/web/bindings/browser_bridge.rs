use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;
use web_sys::HtmlElement;

#[wasm_bindgen(module = "/src/web/bindings/browser_bridge.js")]
extern "C" {
    #[wasm_bindgen(catch, js_name = mountFloatingMenu)]
    fn mount_floating_menu_js(
        reference: &HtmlElement,
        floating: &HtmlElement,
    ) -> Result<js_sys::Promise, JsValue>;
}

/// Holds the floating menu's listeners. Drop when the menu closes or unmounts.
pub struct FloatingMenu(js_sys::Function);

impl FloatingMenu {
    /// Waits for bridge version 1 and positions the menu. Errors include initialization failure.
    pub async fn mount(reference: &HtmlElement, floating: &HtmlElement) -> Result<Self, JsValue> {
        let cleanup = JsFuture::from(mount_floating_menu_js(reference, floating)?).await?;
        Ok(Self(cleanup.dyn_into()?))
    }
}

impl Drop for FloatingMenu {
    fn drop(&mut self) {
        let _ = self.0.call0(&JsValue::UNDEFINED);
    }
}
