use super::*;

#[derive(Properties, PartialEq)]
pub(crate) struct AppShellProps {
    pub(crate) overall_text: AttrValue,
    pub(crate) overall_tone: Classes,
    pub(crate) updated: AttrValue,
    pub(crate) notice: Html,
    pub(crate) navigation: Html,
    pub(crate) swipe_surface: NodeRef,
    pub(crate) on_pointer_down: Callback<PointerEvent>,
    pub(crate) on_pointer_up: Callback<PointerEvent>,
    pub(crate) on_pointer_cancel: Callback<PointerEvent>,
    #[prop_or_default]
    pub(crate) children: Children,
}

#[function_component(AppShell)]
pub(crate) fn app_shell(props: &AppShellProps) -> Html {
    html! {
        <main class={PAGE}>
            <header class={APP_HEADER}>
                <div class={BRAND}>
                    <span class={BRAND_MARK} aria-hidden="true">{"HYZ"}</span>
                    <div>
                        <p class={EYEBROW}>{"LOCAL CONTROL PLANE"}</p>
                        <h1 class={PAGE_TITLE}>{"hyz things"}</h1>
                        <p class={SUBTITLE}>{"个人门户 · 设备与应用管理"}</p>
                    </div>
                </div>
                <div class={classes!(OVERALL, props.overall_tone.clone())} role="status" aria-live="polite" aria-atomic="true">
                    <span class={STATUS_DOT} aria-hidden="true"></span>
                    <div class={OVERALL_COPY}>
                        <strong class={OVERALL_TITLE}>{props.overall_text.clone()}</strong>
                        <small class={OVERALL_META}>{format!("最后更新：{}", props.updated)}</small>
                    </div>
                </div>
            </header>
            {props.notice.clone()}
            <nav class={PORTAL_TABS} aria-label="主导航">
                {props.navigation.clone()}
            </nav>
            <div
                ref={props.swipe_surface.clone()}
                id="portal-swipe-surface"
                class={PORTAL_SWIPE_SURFACE}
                onpointerdown={props.on_pointer_down.clone()}
                onpointerup={props.on_pointer_up.clone()}
                onpointercancel={props.on_pointer_cancel.clone()}
            >
                {for props.children.iter()}
            </div>
            <footer class={FOOTER}>{"数据约每 2 秒自动刷新 · 写操作仅接受同源令牌保护的类型化请求"}</footer>
        </main>
    }
}
