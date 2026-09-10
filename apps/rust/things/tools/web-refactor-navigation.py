#!/usr/bin/env python3
"""Stage 3: replace the legacy portal/router tabs with capability-first navigation."""
from __future__ import annotations

from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
MAIN = ROOT / "apps/rust/things/src/web/main.rs"

ROUTES = r'''#[derive(Clone, Copy, PartialEq, Eq)]
enum AppPage {
    Overview,
    Network,
    Proxy,
    Tailscale,
    Camera,
    Apps,
    System,
}

impl AppPage {
    const ALL: [Self; 7] = [
        Self::Overview,
        Self::Network,
        Self::Proxy,
        Self::Tailscale,
        Self::Camera,
        Self::Apps,
        Self::System,
    ];

    const fn tab_id(self) -> &'static str {
        match self {
            Self::Overview => "app-overview-tab",
            Self::Network => "app-network-tab",
            Self::Proxy => "app-proxy-tab",
            Self::Tailscale => "app-tailscale-tab",
            Self::Camera => "app-camera-tab",
            Self::Apps => "app-apps-tab",
            Self::System => "app-system-tab",
        }
    }

    const fn panel_id(self) -> &'static str {
        match self {
            Self::Overview => "app-overview-panel",
            Self::Network => "app-network-panel",
            Self::Proxy => "app-proxy-panel",
            Self::Tailscale => "app-tailscale-panel",
            Self::Camera => "app-camera-panel",
            Self::Apps => "app-apps-panel",
            Self::System => "app-system-panel",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Overview => "总览",
            Self::Network => "网络",
            Self::Proxy => "代理",
            Self::Tailscale => "Tailscale",
            Self::Camera => "摄像头",
            Self::Apps => "应用",
            Self::System => "系统",
        }
    }

    const fn next(self) -> Option<Self> {
        match self {
            Self::Overview => Some(Self::Network),
            Self::Network => Some(Self::Proxy),
            Self::Proxy => Some(Self::Tailscale),
            Self::Tailscale => Some(Self::Camera),
            Self::Camera => Some(Self::Apps),
            Self::Apps => Some(Self::System),
            Self::System => None,
        }
    }

    const fn previous(self) -> Option<Self> {
        match self {
            Self::Overview => None,
            Self::Network => Some(Self::Overview),
            Self::Proxy => Some(Self::Network),
            Self::Tailscale => Some(Self::Proxy),
            Self::Camera => Some(Self::Tailscale),
            Self::Apps => Some(Self::Camera),
            Self::System => Some(Self::Apps),
        }
    }
}

const PORTAL_SWIPE_THRESHOLD_PX: i32 = 48;

fn app_page_for_swipe(
    current: AppPage,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
) -> Option<AppPage> {
    let horizontal = end_x - start_x;
    let vertical = end_y - start_y;
    if horizontal.abs() < PORTAL_SWIPE_THRESHOLD_PX || horizontal.abs() <= vertical.abs() {
        return None;
    }
    if horizontal < 0 { current.next() } else { current.previous() }
}

fn app_nav_button(candidate: AppPage, current: AppPage, selected: UseStateHandle<AppPage>) -> Html {
    let active = candidate == current;
    let onclick = Callback::from(move |_| selected.set(candidate));
    html! {
        <button
            id={candidate.tab_id()}
            class={classes!(PORTAL_TAB, active.then_some(PORTAL_TAB_ACTIVE))}
            type="button"
            aria-pressed={active.to_string()}
            aria-controls={candidate.panel_id()}
            onclick={onclick}
        >
            {candidate.label()}
        </button>
    }
}

'''

OVERVIEW = r'''fn render_overview(state: &UseReducerHandle<AppState>) -> Html {
    html! {
        <>
            if let Some(snapshot) = &state.snapshot {
                {render_topology(snapshot)}
                {render_kpis(snapshot)}
                {render_issues(snapshot)}
                <div class={VIEW_HEADING}>
                    <div><p class={EYEBROW}>{"DETAILS"}</p><h2 class={SECTION_TITLE}>{"运行详情"}</h2></div>
                    <span class={SECTION_META}>{"保留最近一次成功快照"}</span>
                </div>
                {render_dashboard(snapshot)}
                if let Some(panel) = &state.panel {
                    {render_proxy_groups_read_only(&panel.panel.proxy_groups)}
                }
            } else if state.loading {
                <section class={LOADING_GRID} aria-labelledby="overview-loading-title" aria-busy="true">
                    <h2 id="overview-loading-title" class="sr-only">{"正在加载状态"}</h2>
                    {for (0..3).map(|_| html! { <div class={SKELETON} aria-hidden="true"></div> })}
                </section>
            } else {
                <section class={EMPTY_STATE} role="alert" aria-labelledby="overview-empty-title">
                    <span class={EMPTY_ICON} aria-hidden="true">{"!"}</span>
                    <h2 id="overview-empty-title" class={EMPTY_TITLE}>{"暂时无法读取状态"}</h2>
                    <p class={EMPTY_COPY}>{"面板会自动重试，无需刷新页面。"}</p>
                </section>
            }
            <CustomCountdownPanel />
            <ExamCountdownPanel />
        </>
    }
}

'''

APP = r'''#[function_component(App)]
fn app() -> Html {
    let state = use_reducer(AppState::default);
    let brightness = use_state(|| 128u16);
    let delay_refresh_started = use_state(|| false);
    let app_page = use_state(|| AppPage::Overview);
    let camera_stop_generation = use_state(|| 0u32);
    let swipe_start = use_mut_ref(|| None::<(i32, i32)>);
    let portal_swipe_surface = use_node_ref();

    {
        let state = state.clone();
        use_effect_with((), move |_| {
            let cancelled = Rc::new(Cell::new(false));
            let task_cancelled = cancelled.clone();
            state.dispatch(Action::Started);
            spawn_local(async move {
                loop {
                    if task_cancelled.get() {
                        break;
                    }
                    match fetch_dashboard().await {
                        Ok((snapshot, panel)) => state.dispatch(Action::Success(
                            Box::new(snapshot),
                            Box::new(panel),
                            current_time(),
                        )),
                        Err(error) => state.dispatch(Action::Failure(error)),
                    }
                    TimeoutFuture::new(POLL_DELAY_MS).await;
                }
            });
            move || cancelled.set(true)
        });
    }

    {
        let state = state.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let apps = fetch_json::<AppsResponseDto>(APPS_ENDPOINT, "已部署应用").await;
                state.dispatch(Action::AppsFinished(apps.map(|response| response.apps)));
            });
            || ()
        });
    }

    {
        let state = state.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let session = fetch_json::<AuthSessionDto>(AUTH_SESSION_ENDPOINT, "登录状态").await;
                let authenticated = session
                    .as_ref()
                    .is_ok_and(|session| session.authenticated && !session.must_change);
                state.dispatch(Action::SessionFinished(session));
                if authenticated {
                    dispatch_settings_refresh(state.clone());
                }
            });
            || ()
        });
    }

    {
        let state = state.clone();
        let delay_refresh_started = delay_refresh_started.clone();
        let refresh = state.panel.as_ref().and_then(|bootstrap| {
            bootstrap
                .panel
                .proxy_groups
                .data
                .as_ref()
                .filter(|groups| !groups.is_empty())
                .map(|_| bootstrap.csrf_token.clone())
        });
        use_effect_with(refresh, move |csrf| {
            if let Some(csrf) = csrf.as_ref().filter(|_| !*delay_refresh_started) {
                delay_refresh_started.set(true);
                dispatch_delay_refresh(state.clone(), csrf.clone());
            }
        });
    }

    let (overall_text, overall_tone) = overall_status(&state);
    let updated = state.last_update.as_deref().unwrap_or("尚未更新");
    let admin_csrf = state
        .panel
        .as_ref()
        .map(|panel| panel.csrf_token.clone())
        .unwrap_or_default();
    let is_admin = state
        .session
        .as_ref()
        .is_some_and(|session| session.authenticated && !session.must_change);

    let on_portal_pointer_down = {
        let portal_swipe_surface = portal_swipe_surface.clone();
        let swipe_start = swipe_start.clone();
        Callback::from(move |event: PointerEvent| {
            if event.pointer_type() != "touch" || !event.is_primary() {
                return;
            }
            if !swipe_start_allowed(&event) {
                *swipe_start.borrow_mut() = None;
                return;
            }
            *swipe_start.borrow_mut() = Some((event.client_x(), event.client_y()));
            if let Some(target) = portal_swipe_surface.cast::<Element>() {
                let _ = target.set_pointer_capture(event.pointer_id());
            }
        })
    };
    let on_portal_pointer_up = {
        let app_page = app_page.clone();
        let swipe_start = swipe_start.clone();
        Callback::from(move |event: PointerEvent| {
            if event.pointer_type() != "touch" || !event.is_primary() {
                return;
            }
            let Some((start_x, start_y)) = swipe_start.borrow_mut().take() else {
                return;
            };
            if let Some(next) = app_page_for_swipe(
                *app_page,
                start_x,
                start_y,
                event.client_x(),
                event.client_y(),
            ) {
                app_page.set(next);
            }
        })
    };
    let on_portal_pointer_cancel = {
        let swipe_start = swipe_start.clone();
        Callback::from(move |_event: PointerEvent| *swipe_start.borrow_mut() = None)
    };
    let page = *app_page;
    let admin_required = || html! {
        <section class={EMPTY_STATE} role="status">
            <span class={EMPTY_ICON} aria-hidden="true">{"🔒"}</span>
            <h2 class={EMPTY_TITLE}>{"需要管理员登录"}</h2>
            <p class={EMPTY_COPY}>{"请先在“网络”页面完成管理员登录，再使用此配置页面。"}</p>
        </section>
    };

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
                {match page {
                    AppPage::Overview => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            {render_overview(&state)}
                        </section>
                    },
                    AppPage::Network => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            <Settings state={state.clone()} camera_stop_generation={camera_stop_generation.clone()} />
                        </section>
                    },
                    AppPage::Proxy => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            if is_admin { {render_proxy_control(&state)} } else { {admin_required()} }
                        </section>
                    },
                    AppPage::Tailscale => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            if is_admin { {render_tailscale_control(&state, &admin_csrf)} } else { {admin_required()} }
                        </section>
                    },
                    AppPage::Camera => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            <CameraLiveView admin_csrf={admin_csrf.clone()} is_admin={is_admin} stop_generation={*camera_stop_generation} />
                        </section>
                    },
                    AppPage::Apps => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            <section class={SECTION} aria-labelledby="apps-title">
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
                        </section>
                    },
                    AppPage::System => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            if let Some(snapshot) = &state.snapshot {
                                {render_dashboard(snapshot)}
                                {render_issues(snapshot)}
                            }
                            {render_display_control(&state, brightness.clone())}
                        </section>
                    },
                }}
            </div>
            <footer class={FOOTER}>{"数据约每 2 秒自动刷新 · 写操作仅接受同源令牌保护的类型化请求"}</footer>
        </main>
    }
}

'''


def attribute_start(text: str, marker: str) -> int:
    pos = text.index(marker)
    start = text.rfind("\n", 0, pos) + 1
    while start > 0:
        prev_end = start - 1
        prev_start = text.rfind("\n", 0, prev_end) + 1
        if text[prev_start:prev_end].strip().startswith("#["):
            start = prev_start
        else:
            break
    return start


def main() -> None:
    text = MAIN.read_text()
    if "enum AppPage" in text:
        return

    routes_start = attribute_start(text, "enum PortalView")
    routes_end = text.index("fn swipe_start_allowed", routes_start)
    text = text[:routes_start] + ROUTES + text[routes_end:]

    workspace_start = attribute_start(text, "enum WorkspaceView")
    app_start = text.index("#[function_component(App)]", workspace_start)
    text = text[:workspace_start] + OVERVIEW + text[app_start:]

    app_start = text.index("#[function_component(App)]")
    dispatch_start = text.index("fn dispatch_control<T>", app_start)
    text = text[:app_start] + APP + text[dispatch_start:]
    MAIN.write_text(text)


if __name__ == "__main__":
    main()
