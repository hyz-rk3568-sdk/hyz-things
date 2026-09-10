#!/usr/bin/env python3
"""Finish the shared-component stage with the portal shell.

AppShell owns only stable portal markup and receives status text, navigation,
notice content, swipe callbacks, and page content as props. App state, polling,
authentication, navigation state, and page lifecycle behavior remain in app.rs.
The Apps page also reuses the shared SectionCard/PageHeader contract.
"""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
WEB = ROOT / "apps/rust/things/src/web"
APP = WEB / "app.rs"
COMPONENTS = WEB / "components"
COMPONENTS_MOD = COMPONENTS / "mod.rs"
APP_SHELL = COMPONENTS / "app_shell.rs"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if new in text:
        return text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


def write_app_shell() -> None:
    APP_SHELL.write_text(
        '''use super::*;

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
'''
    )


def update_components_mod() -> None:
    text = COMPONENTS_MOD.read_text()
    if "mod app_shell;" not in text:
        text = replace_once(
            text,
            "mod countdown_card;\n",
            "mod app_shell;\nmod countdown_card;\n",
            "AppShell module declaration",
        )
    if "pub(crate) use app_shell::*;" not in text:
        text = replace_once(
            text,
            "pub(crate) use countdown_card::*;\n",
            "pub(crate) use app_shell::*;\npub(crate) use countdown_card::*;\n",
            "AppShell module export",
        )
    COMPONENTS_MOD.write_text(text)


def update_app() -> None:
    text = APP.read_text()

    old_shell_open = '''    html! {
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
                <div class={classes!(OVERALL, overall_tone.class())} role="status" aria-live="polite" aria-atomic="true">
                    <span class={STATUS_DOT} aria-hidden="true"></span>
                    <div class={OVERALL_COPY}>
                        <strong class={OVERALL_TITLE}>{overall_text}</strong>
                        <small class={OVERALL_META}>{format!("最后更新：{updated}")}</small>
                    </div>
                </div>
            </header>
            {render_notice(&state)}
            <nav class={PORTAL_TABS} aria-label="主导航">
                {for AppPage::ALL.into_iter().map(|candidate| app_nav_button(candidate, page, app_page.clone()))}
            </nav>
            <div
                ref={portal_swipe_surface}
                id="portal-swipe-surface"
                class={PORTAL_SWIPE_SURFACE}
                onpointerdown={on_portal_pointer_down}
                onpointerup={on_portal_pointer_up}
                onpointercancel={on_portal_pointer_cancel}
            >
'''
    new_shell_open = '''    let navigation = html! {
        <>{for AppPage::ALL.into_iter().map(|candidate| app_nav_button(candidate, page, app_page.clone()))}</>
    };

    html! {
        <AppShell
            overall_text={AttrValue::from(overall_text.to_owned())}
            overall_tone={classes!(overall_tone.class())}
            updated={AttrValue::from(updated.to_owned())}
            notice={render_notice(&state)}
            navigation={navigation}
            swipe_surface={portal_swipe_surface}
            on_pointer_down={on_portal_pointer_down}
            on_pointer_up={on_portal_pointer_up}
            on_pointer_cancel={on_portal_pointer_cancel}
        >
'''
    text = replace_once(text, old_shell_open, new_shell_open, "AppShell opening")

    old_shell_close = '''            </div>
            <footer class={FOOTER}>{"数据约每 2 秒自动刷新 · 写操作仅接受同源令牌保护的类型化请求"}</footer>
        </main>
    }
}
'''
    new_shell_close = '''        </AppShell>
    }
}
'''
    text = replace_once(text, old_shell_close, new_shell_close, "AppShell closing")

    old_apps = '''                            <section class={SECTION} aria-labelledby="apps-title">
                                <div class={SECTION_HEAD}>
                                    <div><p class={EYEBROW}>{"APPS"}</p><h2 id="apps-title" class={SECTION_TITLE}>{"应用"}</h2></div>
                                    <span class={SECTION_META}>{"部署记录与独立能力入口"}</span>
                                </div>
                                <div class={APP_GRID}>
                                    <article class={APP_CARD} aria-labelledby="apps-camera-title">
                                        <div class={CONTROL_TITLE}><h3 id="apps-camera-title" class={CONTROL_HEADING}>{"摄像头直播"}</h3><CameraAvailability /></div>
                                        <p class={HELP_TEXT}>{"实时查看摄像头画面；画面配置需要管理员身份。"}</p>
                                    </article>
                                    <article class={APP_CARD} aria-labelledby="apps-router-title">
                                        <div class={CONTROL_TITLE}><h3 id="apps-router-title" class={CONTROL_HEADING}>{"路由器控制面"}</h3></div>
                                        <p class={HELP_TEXT}>{"网络、代理、Tailscale 与系统能力已拆分为一级页面。"}</p>
                                    </article>
                                </div>
                                {render_deployed_apps(&state)}
                            </section>
'''
    new_apps = '''                            <SectionCard title_id="apps-title">
                                <PageHeader title_id="apps-title" eyebrow="APPS" title="应用">
                                    <span class={SECTION_META}>{"部署记录与独立能力入口"}</span>
                                </PageHeader>
                                <div class={APP_GRID}>
                                    <article class={APP_CARD} aria-labelledby="apps-camera-title">
                                        <div class={CONTROL_TITLE}><h3 id="apps-camera-title" class={CONTROL_HEADING}>{"摄像头直播"}</h3><CameraAvailability /></div>
                                        <p class={HELP_TEXT}>{"实时查看摄像头画面；画面配置需要管理员身份。"}</p>
                                    </article>
                                    <article class={APP_CARD} aria-labelledby="apps-router-title">
                                        <div class={CONTROL_TITLE}><h3 id="apps-router-title" class={CONTROL_HEADING}>{"路由器控制面"}</h3></div>
                                        <p class={HELP_TEXT}>{"网络、代理、Tailscale 与系统能力已拆分为一级页面。"}</p>
                                    </article>
                                </div>
                                {render_deployed_apps(&state)}
                            </SectionCard>
'''
    text = replace_once(text, old_apps, new_apps, "Apps shared section/header")
    APP.write_text(text)


def validate() -> None:
    app = APP.read_text()
    shell = APP_SHELL.read_text()
    components_mod = COMPONENTS_MOD.read_text()

    for marker in ["mod app_shell;", "pub(crate) use app_shell::*;"]:
        if marker not in components_mod:
            raise SystemExit(f"components/mod.rs missing AppShell wiring: {marker}")

    forbidden_shell_dependencies = [
        "AppState",
        "AppPage",
        "UseReducerHandle",
        "StatusSnapshot",
        "AuthSessionDto",
        "Request::",
        "spawn_local",
        "use_effect",
    ]
    leaked = [name for name in forbidden_shell_dependencies if name in shell]
    if leaked:
        raise SystemExit(f"AppShell owns app/capability behavior: {', '.join(leaked)}")

    required_shell_contract = [
        '<main class={PAGE}>',
        '<header class={APP_HEADER}>',
        'role="status" aria-live="polite" aria-atomic="true"',
        '<nav class={PORTAL_TABS} aria-label="主导航">',
        'id="portal-swipe-surface"',
        '<footer class={FOOTER}>',
    ]
    missing_shell = [marker for marker in required_shell_contract if marker not in shell]
    if missing_shell:
        raise SystemExit(f"AppShell lost portal DOM contract: {missing_shell}")

    required_app = [
        "let navigation = html! {",
        "<AppShell",
        "overall_text={AttrValue::from(overall_text.to_owned())}",
        "notice={render_notice(&state)}",
        "on_pointer_down={on_portal_pointer_down}",
        '<SectionCard title_id="apps-title">',
        '<PageHeader title_id="apps-title" eyebrow="APPS" title="应用">',
    ]
    missing_app = [marker for marker in required_app if marker not in app]
    if missing_app:
        raise SystemExit(f"app.rs AppShell migration incomplete: {missing_app}")

    forbidden_app_markup = [
        '<main class={PAGE}>',
        '<header class={APP_HEADER}>',
        '<nav class={PORTAL_TABS} aria-label="主导航">',
        '<footer class={FOOTER}>',
        '<section class={SECTION} aria-labelledby="apps-title">',
    ]
    leaked_app = [marker for marker in forbidden_app_markup if marker in app]
    if leaked_app:
        raise SystemExit(f"app.rs still owns shared shell markup: {leaked_app}")
    if app.count("<AppShell") != 1 or app.count("</AppShell>") != 1:
        raise SystemExit("app.rs must compose exactly one AppShell")


if __name__ == "__main__":
    write_app_shell()
    update_components_mod()
    update_app()
    validate()
    print("extracted AppShell and migrated Apps page to shared section/header components")
