use super::*;

const HIDDEN_RESOURCE_DELAY_MS: u32 = 60_000;

#[derive(Properties, PartialEq)]
struct AdminAuthGateProps {
    state: UseReducerHandle<AppState>,
}

#[function_component(AdminAuthGate)]
fn admin_auth_gate(props: &AdminAuthGateProps) -> Html {
    let state = &props.state;
    let login_password = use_node_ref();
    let current_password = use_node_ref();
    let new_password = use_node_ref();
    let confirm_password = use_node_ref();
    let csrf = state
        .panel
        .as_ref()
        .map(|panel| panel.csrf_token.clone())
        .unwrap_or_default();
    let authenticated = state
        .session
        .as_ref()
        .is_some_and(|session| session.authenticated);
    let must_change = state
        .session
        .as_ref()
        .is_some_and(|session| session.authenticated && session.must_change);
    let busy = state.settings_busy;

    let login = {
        let state = state.clone();
        let csrf = csrf.clone();
        let input = login_password.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let Some(input) = input.cast::<HtmlInputElement>() else {
                return;
            };
            let password = input.value();
            input.set_value("");
            dispatch_auth(
                state.clone(),
                AUTH_LOGIN_ENDPOINT,
                csrf.clone(),
                LoginRequest { password },
                "已登录",
            );
        })
    };
    let change_password = {
        let state = state.clone();
        let csrf = csrf.clone();
        let current = current_password.clone();
        let new = new_password.clone();
        let confirm = confirm_password.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let (Some(current), Some(new), Some(confirm)) = (
                current.cast::<HtmlInputElement>(),
                new.cast::<HtmlInputElement>(),
                confirm.cast::<HtmlInputElement>(),
            ) else {
                return;
            };
            let current_value = current.value();
            let new_value = new.value();
            let confirm_value = confirm.value();
            current.set_value("");
            new.set_value("");
            confirm.set_value("");
            if !(12..=1_024).contains(&new_value.len()) {
                state.dispatch(Action::SettingsNotice(
                    "新密码长度必须为 12–1024 字节".to_owned(),
                ));
                return;
            }
            if new_value != confirm_value {
                state.dispatch(Action::SettingsNotice("两次输入的新密码不一致".to_owned()));
                return;
            }
            dispatch_auth(
                state.clone(),
                AUTH_PASSWORD_ENDPOINT,
                csrf.clone(),
                PasswordRequest {
                    current_password: current_value,
                    new_password: new_value,
                },
                "密码已更新",
            );
        })
    };

    if !state.session_checked {
        return html! { <section class={EMPTY_STATE} role="status"><h2 class={EMPTY_TITLE}>{"正在检查登录状态…"}</h2></section> };
    }
    if !authenticated {
        return html! {
            <SectionCard title_id="admin-login-title" busy={Some(busy)}>
                <PageHeader title_id="admin-login-title" eyebrow="ADMIN" title="管理员登录" centered=true><span class={SECTION_META}>{"登录后继续当前页面"}</span></PageHeader>
                if let Some(notice) = &state.settings_notice { <FeedbackState message={AttrValue::from(notice.clone())} /> }
                if let Some(error) = &state.panel_meta.error { <ErrorState message={format!("登录令牌读取失败：{error}")} /> }
                <form class={AUTH_FORM} onsubmit={login} autocomplete="on">
                    <label class={FIELD}><span class={FIELD_LABEL}>{"用户名"}</span><input class={READONLY_INPUT} value="admin" readonly=true autocomplete="username" /></label>
                    <label class={FIELD}><span class={FIELD_LABEL}>{"密码"}</span><input class={INPUT} ref={login_password} type="password" required=true autocomplete="current-password" /></label>
                    <button class={BUTTON_BLOCK_MOBILE} type="submit" disabled={busy || csrf.is_empty()}>{if busy { "登录中…" } else { "登录" }}</button>
                    <small class={HELP_TEXT}>{"新设备首次登录密码为 admin；登录后必须立即修改。"}</small>
                </form>
            </SectionCard>
        };
    }
    if must_change {
        return html! {
            <SectionCard title_id="admin-password-title" busy={Some(busy)}>
                <PageHeader title_id="admin-password-title" eyebrow="ADMIN" title="修改管理员密码" centered=true><span class={SECTION_META}>{"完成后继续当前页面"}</span></PageHeader>
                if let Some(notice) = &state.settings_notice { <FeedbackState message={AttrValue::from(notice.clone())} /> }
                <div class={FORCED_PASSWORD}>
                    <div class={classes!(RISK_ALERT, "alert-error", "border-error/20")} role="alert"><div><strong>{"必须先修改默认密码"}</strong><p class={RISK_COPY}>{"新密码至少 12 字节，不能继续使用默认密码。完成前其他设置保持锁定。"}</p></div></div>
                    <form class={FORM_GRID} onsubmit={change_password} autocomplete="on" aria-describedby="password-policy">
                        <label class={FIELD}><span class={FIELD_LABEL}>{"当前密码"}</span><input class={INPUT} ref={current_password} type="password" required=true autocomplete="current-password" /></label>
                        <label class={FIELD}><span class={FIELD_LABEL}>{"新密码"}</span><input class={INPUT} ref={new_password} type="password" required=true maxlength="1024" autocomplete="new-password" /></label>
                        <label class={FIELD}><span class={FIELD_LABEL}>{"确认新密码"}</span><input class={INPUT} ref={confirm_password} type="password" required=true maxlength="1024" autocomplete="new-password" /></label>
                        <p id="password-policy" class="sr-only">{"新密码长度必须为 12 至 1024 字节，且两次输入必须一致。"}</p>
                        <div class={FORM_ACTIONS}><button class={BUTTON_PRIMARY} type="submit" disabled={busy || csrf.is_empty()}>{"修改密码"}</button></div>
                    </form>
                </div>
            </SectionCard>
        };
    }
    Html::default()
}

fn page_updated(state: &AppState, page: AppPage) -> String {
    let values: Vec<Option<&String>> = match page {
        AppPage::Overview | AppPage::System => vec![
            state.status_meta.last_success.as_ref(),
            state.panel_meta.last_success.as_ref(),
        ],
        AppPage::Study => Vec::new(),
        AppPage::Network => vec![
            state.network_meta.last_success.as_ref(),
            state.pending_meta.last_success.as_ref(),
        ],
        AppPage::Proxy => vec![
            state.status_meta.last_success.as_ref(),
            state.panel_meta.last_success.as_ref(),
            state.subscription_meta.last_success.as_ref(),
        ],
        AppPage::Devices => vec![state.device_meta.last_success.as_ref()],
        AppPage::Tailscale => vec![
            state.tailscale_meta.last_success.as_ref(),
            state.tailscale_peers_meta.last_success.as_ref(),
        ],
        AppPage::Camera => vec![state.panel_meta.last_success.as_ref()],
        AppPage::Apps => vec![state.apps_meta.last_success.as_ref()],
    };
    values
        .into_iter()
        .flatten()
        .max()
        .cloned()
        .unwrap_or_else(|| {
            if page == AppPage::Study {
                "本地状态".to_owned()
            } else {
                "尚未更新".to_owned()
            }
        })
}

fn dispatch_page_resources(state: UseReducerHandle<AppState>, page: AppPage, is_admin: bool) {
    if page.protected()
        || matches!(
            page,
            AppPage::Overview | AppPage::Proxy | AppPage::Camera | AppPage::System
        )
    {
        dispatch_panel_refresh(state.clone());
    }
    match page {
        AppPage::Overview | AppPage::System => {
            dispatch_status_refresh(state.clone());
        }
        AppPage::Study => {}
        AppPage::Network if is_admin => {
            dispatch_network_refresh(state.clone());
            dispatch_pending_refresh(state.clone());
        }
        AppPage::Proxy if is_admin => {
            dispatch_status_refresh(state.clone());
            dispatch_subscription_refresh_read(state.clone());
        }
        AppPage::Devices if is_admin => dispatch_devices_refresh(state.clone()),
        AppPage::Tailscale if is_admin => {
            dispatch_tailscale_refresh(state.clone());
            dispatch_tailscale_peers_refresh(state.clone());
        }
        AppPage::Apps => dispatch_apps_refresh(state.clone()),
        AppPage::Camera
        | AppPage::Network
        | AppPage::Proxy
        | AppPage::Devices
        | AppPage::Tailscale => {}
    }
}

fn dispatch_dynamic_resources(state: UseReducerHandle<AppState>, page: AppPage, is_admin: bool) {
    match page {
        AppPage::Overview | AppPage::System => {
            dispatch_status_refresh(state.clone());
            dispatch_panel_refresh(state.clone());
        }
        AppPage::Network if is_admin => dispatch_pending_refresh(state.clone()),
        AppPage::Proxy if is_admin => {
            dispatch_status_refresh(state.clone());
            dispatch_panel_refresh(state.clone());
        }
        AppPage::Devices if is_admin => dispatch_devices_refresh(state.clone()),
        AppPage::Tailscale if is_admin => {
            dispatch_tailscale_refresh(state.clone());
            dispatch_tailscale_peers_refresh(state.clone());
        }
        _ => {}
    }
}

fn document_hidden() -> bool {
    web_sys::window()
        .and_then(|window| window.document())
        .is_some_and(|document| document.hidden())
}

#[function_component(App)]
pub(crate) fn app() -> Html {
    let state = use_reducer(AppState::default);
    let brightness = use_state(|| 128u16);
    let route = use_state(|| current_route().0);
    let refresh_signal = use_state(|| 0_u64);
    let camera_stop_generation = use_state(|| 0u32);
    let swipe_start = use_mut_ref(|| None::<(i32, i32)>);
    let portal_swipe_surface = use_node_ref();

    {
        let route_state = route.clone();
        use_effect_with((), move |_| {
            let window = web_sys::window().expect("browser window");
            let (initial, canonical) = current_route();
            if !canonical {
                navigate_route(&route_state, initial, true);
            }
            let changed_route = route_state.clone();
            let changed = Closure::<dyn FnMut(Event)>::new(move |_| {
                let (next, canonical) = current_route();
                if canonical {
                    changed_route.set(next);
                } else {
                    navigate_route(&changed_route, next, true);
                }
            });
            let _ = window
                .add_event_listener_with_callback("popstate", changed.as_ref().unchecked_ref());
            let _ = window
                .add_event_listener_with_callback("hashchange", changed.as_ref().unchecked_ref());
            move || {
                let _ = window.remove_event_listener_with_callback(
                    "popstate",
                    changed.as_ref().unchecked_ref(),
                );
                let _ = window.remove_event_listener_with_callback(
                    "hashchange",
                    changed.as_ref().unchecked_ref(),
                );
            }
        });
    }

    {
        let refresh_signal = refresh_signal.clone();
        use_effect_with((), move |_| {
            let window = web_sys::window().expect("browser window");
            let document = window.document().expect("browser document");
            let signal = refresh_signal.clone();
            let refresh = Closure::<dyn FnMut(Event)>::new(move |_| {
                if !document_hidden() {
                    signal.set((*signal).wrapping_add(1));
                }
            });
            let _ = document.add_event_listener_with_callback(
                "visibilitychange",
                refresh.as_ref().unchecked_ref(),
            );
            let _ =
                window.add_event_listener_with_callback("focus", refresh.as_ref().unchecked_ref());
            let _ =
                window.add_event_listener_with_callback("online", refresh.as_ref().unchecked_ref());
            move || {
                let _ = document.remove_event_listener_with_callback(
                    "visibilitychange",
                    refresh.as_ref().unchecked_ref(),
                );
                let _ = window
                    .remove_event_listener_with_callback("focus", refresh.as_ref().unchecked_ref());
                let _ = window.remove_event_listener_with_callback(
                    "online",
                    refresh.as_ref().unchecked_ref(),
                );
            }
        });
    }

    {
        let state = state.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let session = fetch_json::<AuthSessionDto>(AUTH_SESSION_ENDPOINT, "登录状态").await;
                state.dispatch(Action::SessionFinished(session));
            });
            || ()
        });
    }

    let page = route.page;
    let is_admin = state
        .session
        .as_ref()
        .is_some_and(|session| session.authenticated && !session.must_change);
    {
        let state = state.clone();
        let refresh_signal = *refresh_signal;
        use_effect_with((page, is_admin, refresh_signal), move |_| {
            let cancelled = Rc::new(Cell::new(false));
            let task_cancelled = cancelled.clone();
            dispatch_page_resources(state.clone(), page, is_admin);
            spawn_local(async move {
                loop {
                    let delay = if document_hidden() {
                        HIDDEN_RESOURCE_DELAY_MS
                    } else {
                        POLL_DELAY_MS
                    };
                    TimeoutFuture::new(delay).await;
                    if task_cancelled.get() {
                        break;
                    }
                    if document_hidden() {
                        continue;
                    }
                    dispatch_dynamic_resources(state.clone(), page, is_admin);
                }
            });
            move || cancelled.set(true)
        });
    }

    let admin_csrf = state
        .panel
        .as_ref()
        .map(|panel| panel.csrf_token.clone())
        .unwrap_or_default();
    let (overall_text, overall_tone) = if state.snapshot.is_none() && page == AppPage::Study {
        ("按页面加载", Tone::Neutral)
    } else {
        overall_status(&state)
    };
    let updated = page_updated(&state, page);

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
        let route = route.clone();
        let swipe_start = swipe_start.clone();
        Callback::from(move |event: PointerEvent| {
            if event.pointer_type() != "touch" || !event.is_primary() {
                return;
            }
            let Some((start_x, start_y)) = swipe_start.borrow_mut().take() else {
                return;
            };
            if let Some(next) = app_page_for_swipe(
                route.page,
                start_x,
                start_y,
                event.client_x(),
                event.client_y(),
            ) {
                navigate_route(&route, PortalRoute::for_page(next), false);
            }
        })
    };
    let on_portal_pointer_cancel = {
        let swipe_start = swipe_start.clone();
        Callback::from(move |_event: PointerEvent| *swipe_start.borrow_mut() = None)
    };
    let navigation = html! { <>{for AppPage::ALL.into_iter().map(|candidate| app_nav_button(candidate, page, route.clone()))}</> };

    html! {
        <AppShell
            overall_text={AttrValue::from(overall_text.to_owned())}
            overall_tone={classes!(overall_tone.class())}
            updated={AttrValue::from(updated)}
            notice={render_notice(&state)}
            navigation={navigation}
            swipe_surface={portal_swipe_surface}
            on_pointer_down={on_portal_pointer_down}
            on_pointer_up={on_portal_pointer_up}
            on_pointer_cancel={on_portal_pointer_cancel}
        >
            <section id={AppPage::Overview.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={AppPage::Overview.tab_id()} hidden={page != AppPage::Overview}>{render_overview(&state)}</section>
            <section id={AppPage::Study.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={AppPage::Study.tab_id()} hidden={page != AppPage::Study}>{render_study()}</section>
            <section id={AppPage::Network.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={AppPage::Network.tab_id()} hidden={page != AppPage::Network}>
                if is_admin { <Settings state={state.clone()} camera_stop_generation={camera_stop_generation.clone()} /> } else if page == AppPage::Network { <AdminAuthGate state={state.clone()} /> }
            </section>
            {for AppPage::ALL.into_iter().filter(|candidate| *candidate != page && !matches!(candidate, AppPage::Overview | AppPage::Study | AppPage::Network)).map(|candidate| html! {
                <section id={candidate.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={candidate.tab_id()} hidden=true></section>
            })}
            {match page {
                AppPage::Overview | AppPage::Study | AppPage::Network => Html::default(),
                AppPage::Proxy => html! { <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>{if is_admin { render_proxy_control(&state) } else { html! { <AdminAuthGate state={state.clone()} /> } }}</section> },
                AppPage::Devices => html! { <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>{if is_admin { html! { <DevicesPage state={state.clone()} route={(*route).clone()} route_state={route.clone()} /> } } else { html! { <AdminAuthGate state={state.clone()} /> } }}</section> },
                AppPage::Tailscale => html! { <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>{if is_admin { render_tailscale_control(&state, &admin_csrf) } else { html! { <AdminAuthGate state={state.clone()} /> } }}</section> },
                AppPage::Camera => html! { <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}><CameraLiveView admin_csrf={admin_csrf.clone()} is_admin={is_admin} stop_generation={*camera_stop_generation} /></section> },
                AppPage::Apps => html! {
                    <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                        <SectionCard title_id="apps-title">
                            <PageHeader title_id="apps-title" eyebrow="APPS" title="应用"><span class={SECTION_META}>{state.apps_meta.status_text()}</span></PageHeader>
                            if let Some(error) = &state.apps_error { <ErrorState message={format!("部署记录读取失败：{error}")} /> }
                            <div class={APP_GRID}>
                                <article class={APP_CARD} aria-labelledby="apps-camera-title"><div class={CONTROL_TITLE}><h3 id="apps-camera-title" class={CONTROL_HEADING}>{"摄像头直播"}</h3><CameraAvailability /></div><p class={HELP_TEXT}>{"实时查看摄像头画面；画面配置需要管理员身份。"}</p></article>
                                <article class={APP_CARD} aria-labelledby="apps-router-title"><div class={CONTROL_TITLE}><h3 id="apps-router-title" class={CONTROL_HEADING}>{"路由器控制面"}</h3></div><p class={HELP_TEXT}>{"网络、代理、设备、Tailscale 与系统能力已拆分为一级页面。"}</p></article>
                            </div>
                            {render_deployed_apps(&state)}
                        </SectionCard>
                    </section>
                },
                AppPage::System => html! { <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>{render_system(&state, brightness.clone())}</section> },
            }}
        </AppShell>
    }
}
