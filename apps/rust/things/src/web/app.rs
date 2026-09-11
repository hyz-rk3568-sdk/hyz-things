use super::*;

#[derive(Clone, PartialEq, Default)]
pub(super) struct AppState {
    pub(super) snapshot: Option<StatusSnapshot>,
    pub(super) panel: Option<PanelBootstrap>,
    pub(super) last_update: Option<String>,
    pub(super) poll_error: Option<String>,
    pub(super) display_notice: Option<String>,
    pub(super) display_busy: bool,
    pub(super) lan_tun_notice: Option<String>,
    pub(super) lan_tun_busy: bool,
    pub(super) local_system_proxy_notice: Option<String>,
    pub(super) local_system_proxy_busy: bool,
    pub(super) node_notice: Option<String>,
    pub(super) node_busy: bool,
    pub(super) loading: bool,
    pub(super) session_checked: bool,
    pub(super) session: Option<AuthSessionDto>,
    pub(super) settings_notice: Option<String>,
    pub(super) settings_busy: bool,
    pub(super) network: Option<NetworkConfigDto>,
    pub(super) pending_network: Option<NetworkPendingDto>,
    pub(super) scan_entries: Vec<WifiScanDto>,
    pub(super) subscription: Option<SubscriptionDto>,
    pub(super) device_policies: Option<DevicePolicySnapshotDto>,
    pub(super) tailscale: Option<TailscaleStatus>,
    pub(super) tailscale_peers: Option<TailscalePeerSnapshot>,
    pub(super) tailscale_peers_error: Option<String>,
    pub(super) tailscale_login_url: Option<String>,
    pub(super) apps: Option<Vec<InstalledAppDto>>,
    pub(super) apps_error: Option<String>,
}

#[derive(Clone, Copy)]
pub(super) enum ControlArea {
    Display,
    LanTun,
    LocalSystemProxy,
    Nodes,
}

pub(super) type SettingsData = (
    NetworkConfigDto,
    NetworkPendingDto,
    SubscriptionDto,
    DevicePolicySnapshotDto,
    TailscaleStatus,
);

pub(super) enum Action {
    Started,
    Success(Box<StatusSnapshot>, Box<PanelBootstrap>, String),
    Failure(String),
    ControlStarted(ControlArea),
    ControlFinished(ControlArea, Result<String, String>),
    ProxyDelaysFinished(Result<Vec<ProxyGroup>, String>),
    SessionFinished(Result<AuthSessionDto, String>),
    AuthFinished(Result<(AuthSessionDto, String), String>),
    SettingsStarted,
    SettingsFinished(
        Result<SettingsData, String>,
        Result<TailscalePeerSnapshot, String>,
    ),
    TailscaleMutationFinished(Result<(TailscaleStatus, Option<String>, String), String>),
    TailscalePeersFinished(Result<TailscalePeerSnapshot, String>),
    SettingsMutationFinished(Result<String, String>),
    ScanFinished(Result<Vec<WifiScanDto>, String>),
    SettingsNotice(String),
    AppsFinished(Result<Vec<InstalledAppDto>, String>),
}

impl Reducible for AppState {
    type Action = Action;

    fn reduce(self: Rc<Self>, action: Self::Action) -> Rc<Self> {
        match action {
            Action::Started => Self {
                loading: true,
                ..(*self).clone()
            }
            .into(),
            Action::Success(snapshot, panel, last_update) => Self {
                snapshot: Some(*snapshot),
                panel: Some(*panel),
                last_update: Some(last_update),
                poll_error: None,
                loading: false,
                ..(*self).clone()
            }
            .into(),
            Action::Failure(error) => Self {
                poll_error: Some(error),
                loading: false,
                ..(*self).clone()
            }
            .into(),
            Action::ControlStarted(area) => {
                let mut next = (*self).clone();
                match area {
                    ControlArea::Display => {
                        next.display_busy = true;
                        next.display_notice = None;
                    }
                    ControlArea::LanTun => {
                        next.lan_tun_busy = true;
                        next.lan_tun_notice = None;
                    }
                    ControlArea::LocalSystemProxy => {
                        next.local_system_proxy_busy = true;
                        next.local_system_proxy_notice = None;
                    }
                    ControlArea::Nodes => {
                        next.node_busy = true;
                        next.node_notice = None;
                    }
                }
                next.into()
            }
            Action::ControlFinished(area, result) => {
                let mut next = (*self).clone();
                let notice = Some(match result {
                    Ok(message) => message,
                    Err(error) => format!("操作失败：{error}"),
                });
                match area {
                    ControlArea::Display => {
                        next.display_busy = false;
                        next.display_notice = notice;
                    }
                    ControlArea::LanTun => {
                        next.lan_tun_busy = false;
                        next.lan_tun_notice = notice;
                    }
                    ControlArea::LocalSystemProxy => {
                        next.local_system_proxy_busy = false;
                        next.local_system_proxy_notice = notice;
                    }
                    ControlArea::Nodes => {
                        next.node_busy = false;
                        next.node_notice = notice;
                    }
                }
                next.into()
            }
            Action::ProxyDelaysFinished(result) => {
                let mut next = (*self).clone();
                next.node_busy = false;
                match result {
                    Ok(groups) => {
                        if let Some(bootstrap) = &mut next.panel {
                            bootstrap.panel.proxy_groups = Component::available(groups);
                        }
                        next.node_notice = None;
                    }
                    Err(error) => {
                        next.node_notice = Some(format!("测速失败：{error}"));
                    }
                }
                next.into()
            }
            Action::SessionFinished(result) => {
                let mut next = (*self).clone();
                next.session_checked = true;
                match result {
                    Ok(session) => {
                        next.session = Some(session);
                        next.settings_notice = None;
                    }
                    Err(error) => {
                        next.session = None;
                        next.settings_notice = Some(format!("无法检查登录状态：{error}"));
                    }
                }
                next.into()
            }
            Action::AuthFinished(result) => {
                let mut next = (*self).clone();
                next.settings_busy = false;
                match result {
                    Ok((session, message)) => {
                        if !session.authenticated {
                            next.network = None;
                            next.pending_network = None;
                            next.subscription = None;
                            next.device_policies = None;
                            next.tailscale = None;
                            next.tailscale_peers = None;
                            next.tailscale_peers_error = None;
                            next.tailscale_login_url = None;
                            next.scan_entries.clear();
                        }
                        next.session = Some(session);
                        next.settings_notice = Some(message);
                    }
                    Err(error) => next.settings_notice = Some(format!("操作失败：{error}")),
                }
                next.into()
            }
            Action::SettingsStarted => Self {
                settings_busy: true,
                settings_notice: None,
                ..(*self).clone()
            }
            .into(),
            Action::SettingsFinished(result, peers_result) => {
                let mut next = (*self).clone();
                next.settings_busy = false;
                match result {
                    Ok((network, pending, subscription, device_policies, tailscale)) => {
                        next.network = Some(network);
                        next.pending_network = Some(pending);
                        next.subscription = Some(subscription);
                        next.device_policies = Some(device_policies);
                        next.tailscale = Some(tailscale);
                    }
                    Err(error) => {
                        next.settings_notice = Some(format!("设置数据读取失败：{error}"));
                    }
                }
                match peers_result {
                    Ok(peers) => {
                        next.tailscale_peers = Some(peers);
                        next.tailscale_peers_error = None;
                    }
                    Err(error) => next.tailscale_peers_error = Some(error),
                }
                next.into()
            }
            Action::TailscaleMutationFinished(result) => {
                let mut next = (*self).clone();
                next.settings_busy = false;
                match result {
                    Ok((tailscale, login_url, message)) => {
                        let logged_out = tailscale.authenticated == Some(false)
                            && tailscale.desired_mode == Some(TailscaleMode::Disabled);
                        next.tailscale = Some(tailscale);
                        next.tailscale_login_url = login_url;
                        next.settings_notice = Some(message);
                        if logged_out {
                            next.tailscale_peers = None;
                            next.tailscale_peers_error = None;
                        }
                    }
                    Err(error) => {
                        next.settings_notice = Some(format!("操作失败：{error}"));
                    }
                }
                next.into()
            }
            Action::TailscalePeersFinished(result) => {
                let mut next = (*self).clone();
                match result {
                    Ok(peers) => {
                        next.tailscale_peers = Some(peers);
                        next.tailscale_peers_error = None;
                    }
                    Err(error) => next.tailscale_peers_error = Some(error),
                }
                next.into()
            }
            Action::SettingsMutationFinished(result) => Self {
                settings_busy: false,
                settings_notice: Some(match result {
                    Ok(message) => message,
                    Err(error) => format!("操作失败：{error}"),
                }),
                ..(*self).clone()
            }
            .into(),
            Action::ScanFinished(result) => {
                let mut next = (*self).clone();
                next.settings_busy = false;
                match result {
                    Ok(entries) => {
                        next.scan_entries = entries;
                        next.settings_notice = Some("STA 扫描已完成".to_owned());
                    }
                    Err(error) => next.settings_notice = Some(format!("扫描失败：{error}")),
                }
                next.into()
            }
            Action::SettingsNotice(message) => Self {
                settings_notice: Some(message),
                ..(*self).clone()
            }
            .into(),
            Action::AppsFinished(result) => match result {
                Ok(apps) => Self {
                    apps: Some(apps),
                    apps_error: None,
                    ..(*self).clone()
                }
                .into(),
                Err(error) => Self {
                    apps_error: Some(error),
                    ..(*self).clone()
                }
                .into(),
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AppPage {
    Overview,
    Network,
    Proxy,
    Activity,
    Tailscale,
    Camera,
    Apps,
    System,
}

impl AppPage {
    const ALL: [Self; 8] = [
        Self::Overview,
        Self::Network,
        Self::Proxy,
        Self::Activity,
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
            Self::Activity => "app-activity-tab",
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
            Self::Activity => "app-activity-panel",
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
            Self::Activity => "活动",
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
            Self::Proxy => Some(Self::Activity),
            Self::Activity => Some(Self::Tailscale),
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
            Self::Activity => Some(Self::Proxy),
            Self::Tailscale => Some(Self::Activity),
            Self::Camera => Some(Self::Tailscale),
            Self::Apps => Some(Self::Camera),
            Self::System => Some(Self::Apps),
        }
    }
}

pub(super) const PORTAL_SWIPE_THRESHOLD_PX: i32 = 48;

pub(super) fn app_page_for_swipe(
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
    if horizontal < 0 {
        current.next()
    } else {
        current.previous()
    }
}

pub(super) fn app_nav_button(
    candidate: AppPage,
    current: AppPage,
    selected: UseStateHandle<AppPage>,
) -> Html {
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

pub(super) fn swipe_start_allowed(event: &PointerEvent) -> bool {
    let Some(mut element) = event
        .target()
        .and_then(|target| target.dyn_into::<Element>().ok())
    else {
        return true;
    };

    loop {
        if matches!(
            element.tag_name().as_str(),
            "A" | "AUDIO" | "BUTTON" | "INPUT" | "SELECT" | "TEXTAREA" | "VIDEO"
        ) || element.get_attribute("data-swipe-ignore").is_some()
        {
            return false;
        }
        let Some(parent) = element.parent_element() else {
            return true;
        };
        element = parent;
    }
}

#[function_component(App)]
pub(super) fn app() -> Html {
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
    let admin_required = || {
        html! {
            <section class={EMPTY_STATE} role="status">
                <span class={EMPTY_ICON} aria-hidden="true">{"🔒"}</span>
                <h2 class={EMPTY_TITLE}>{"需要管理员登录"}</h2>
                <p class={EMPTY_COPY}>{"请先在“网络”页面完成管理员登录，再使用此配置页面。"}</p>
            </section>
        }
    };

    let navigation = html! {
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
                // Overview and Network stay mounted so local drafts/timers survive page switches.
                // The other inactive pages keep empty panel targets in the DOM so every aria-controls
                // relationship remains valid. Camera content itself is still mounted only while active,
                // preserving the existing stop-on-page-leave session lifecycle.
                <section
                    id={AppPage::Overview.panel_id()}
                    class={WORKSPACE_PANEL}
                    aria-labelledby={AppPage::Overview.tab_id()}
                    hidden={page != AppPage::Overview}
                >
                    {render_overview(&state)}
                </section>
                <section
                    id={AppPage::Network.panel_id()}
                    class={WORKSPACE_PANEL}
                    aria-labelledby={AppPage::Network.tab_id()}
                    hidden={page != AppPage::Network}
                >
                    <Settings state={state.clone()} camera_stop_generation={camera_stop_generation.clone()} />
                </section>
                {for AppPage::ALL.into_iter()
                    .filter(|candidate| {
                        *candidate != page
                            && !matches!(candidate, AppPage::Overview | AppPage::Network)
                    })
                    .map(|candidate| html! {
                        <section
                            id={candidate.panel_id()}
                            class={WORKSPACE_PANEL}
                            aria-labelledby={candidate.tab_id()}
                            hidden=true
                        ></section>
                    })}
                {match page {
                    AppPage::Overview | AppPage::Network => Html::default(),
                    AppPage::Proxy => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            if is_admin { {render_proxy_control(&state)} } else { {admin_required()} }
                        </section>
                    },
                    AppPage::Activity => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            if is_admin { {render_activity()} } else { {admin_required()} }
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
                            <SectionCard title_id="apps-title">
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
                        </section>
                    },
                    AppPage::System => html! {
                        <section id={page.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={page.tab_id()}>
                            {render_system(&state, brightness.clone())}
                        </section>
                    },
                }}
        </AppShell>
    }
}

pub(super) fn dispatch_control<T>(
    state: UseReducerHandle<AppState>,
    area: ControlArea,
    endpoint: &'static str,
    csrf_token: String,
    body: T,
    success: String,
) where
    T: serde::Serialize + 'static,
{
    state.dispatch(Action::ControlStarted(area));
    spawn_local(async move {
        let request = match Request::post(endpoint)
            .credentials(RequestCredentials::SameOrigin)
            .header("Accept", "application/json")
            .header("X-HYZ-CSRF", &csrf_token)
            .json(&body)
        {
            Ok(request) => request,
            Err(error) => {
                state.dispatch(Action::ControlFinished(
                    area,
                    Err(format!("无法编码请求：{error}")),
                ));
                return;
            }
        };
        let result = match request.send().await {
            Ok(response) if response.ok() => Ok(success),
            Ok(response) => Err(format!("控制接口返回 HTTP {}", response.status())),
            Err(error) => Err(format!("无法连接控制接口：{error}")),
        };
        state.dispatch(Action::ControlFinished(area, result));
    });
}

pub(super) fn dispatch_delay_refresh(state: UseReducerHandle<AppState>, csrf_token: String) {
    state.dispatch(Action::ControlStarted(ControlArea::Nodes));
    spawn_local(async move {
        let request = match Request::post(PROXY_DELAYS_ENDPOINT)
            .credentials(RequestCredentials::SameOrigin)
            .header("Accept", "application/json")
            .header("X-HYZ-CSRF", &csrf_token)
            .json(&ProxyDelayRefreshRequest {})
        {
            Ok(request) => request,
            Err(error) => {
                state.dispatch(Action::ProxyDelaysFinished(Err(format!(
                    "无法编码测速请求：{error}"
                ))));
                return;
            }
        };
        let result = match request.send().await {
            Ok(response) if response.ok() => {
                match response.json::<DelayRefreshControlResponse>().await {
                    Ok(DelayRefreshControlResponse::ProxyDelays { groups }) => Ok(groups),
                    Err(error) => Err(format!("测速结果格式无效：{error}")),
                }
            }
            Ok(response) => Err(format!("测速接口返回 HTTP {}", response.status())),
            Err(error) => Err(format!("无法连接测速接口：{error}")),
        };
        state.dispatch(Action::ProxyDelaysFinished(result));
    });
}

pub(super) async fn fetch_settings_data() -> Result<SettingsData, String> {
    let network = fetch_json::<NetworkConfigResponseDto>(NETWORK_CONFIG_ENDPOINT, "网络配置")
        .await?
        .config;
    let pending =
        fetch_json::<NetworkPendingDto>(NETWORK_PENDING_ENDPOINT, "待确认网络配置").await?;
    let subscription = fetch_json::<SubscriptionResponseDto>(SUBSCRIPTION_ENDPOINT, "订阅状态")
        .await?
        .subscription;
    let device_policies =
        fetch_json::<DevicePolicySnapshotDto>(DEVICE_POLICIES_ENDPOINT, "设备代理策略").await?;
    let tailscale = fetch_json::<TailscaleResponseDto>(TAILSCALE_ENDPOINT, "Tailscale 状态")
        .await?
        .tailscale;
    Ok((network, pending, subscription, device_policies, tailscale))
}

pub(super) async fn fetch_tailscale_peers() -> Result<TailscalePeerSnapshot, String> {
    fetch_json::<TailscalePeersResponseDto>(TAILSCALE_PEERS_ENDPOINT, "Tailscale 设备列表")
        .await
        .map(|response| response.peers)
}

pub(super) fn dispatch_settings_refresh(state: UseReducerHandle<AppState>) {
    state.dispatch(Action::SettingsStarted);
    spawn_local(async move {
        let settings = fetch_settings_data().await;
        let peers = fetch_tailscale_peers().await;
        state.dispatch(Action::SettingsFinished(settings, peers));
    });
}

pub(super) fn dispatch_auth<T: serde::Serialize + 'static>(
    state: UseReducerHandle<AppState>,
    endpoint: &'static str,
    csrf: String,
    body: T,
    success: &'static str,
) {
    state.dispatch(Action::SettingsStarted);
    spawn_local(async move {
        let result = post_json_response::<_, AuthSessionDto>(endpoint, &csrf, &body, "认证")
            .await
            .map(|session| (session, success.to_owned()));
        let refresh = result
            .as_ref()
            .is_ok_and(|(session, _)| session.authenticated && !session.must_change);
        state.dispatch(Action::AuthFinished(result));
        if refresh {
            dispatch_settings_refresh(state.clone());
        }
    });
}

pub(super) fn dispatch_settings_mutation<T: serde::Serialize + 'static>(
    state: UseReducerHandle<AppState>,
    endpoint: &'static str,
    csrf: String,
    body: T,
    label: &'static str,
    success: &'static str,
) {
    state.dispatch(Action::SettingsStarted);
    spawn_local(async move {
        let result = post_json(endpoint, &csrf, &body, label).await;
        if result.is_ok() {
            let settings = fetch_settings_data().await;
            let peers = fetch_tailscale_peers().await;
            state.dispatch(Action::SettingsFinished(settings, peers));
        }
        state.dispatch(Action::SettingsMutationFinished(
            result.map(|_| success.to_owned()),
        ));
    });
}

pub(super) fn dispatch_tailscale_mutation<T: serde::Serialize + 'static>(
    state: UseReducerHandle<AppState>,
    endpoint: &'static str,
    csrf: String,
    body: T,
    success: &'static str,
) {
    state.dispatch(Action::SettingsStarted);
    spawn_local(async move {
        let result = post_json_response::<_, TailscaleMutationResponseDto>(
            endpoint,
            &csrf,
            &body,
            "Tailscale",
        )
        .await
        .map(|response| (response.tailscale, response.login_url, success.to_owned()));
    let refresh_peers = result.is_ok() && endpoint != TAILSCALE_LOGOUT_ENDPOINT;
        state.dispatch(Action::TailscaleMutationFinished(result));
        if refresh_peers {
            state.dispatch(Action::TailscalePeersFinished(
                fetch_tailscale_peers().await,
            ));
        }
    });
}

pub(super) fn dispatch_disruptive_settings_mutation<T: serde::Serialize + 'static>(
    state: UseReducerHandle<AppState>,
    endpoint: &'static str,
    csrf: String,
    body: T,
    label: &'static str,
    success: &'static str,
) {
    state.dispatch(Action::SettingsStarted);
    spawn_local(async move {
        // The inline confirmation panel and expanded form must be painted away before the request
        // can reconfigure the radio and disconnect this browser.
        TimeoutFuture::new(NETWORK_APPLY_PAINT_DELAY_MS).await;
        let result = post_json(endpoint, &csrf, &body, label).await;
        if result.is_ok() {
            let settings = fetch_settings_data().await;
            let peers = fetch_tailscale_peers().await;
            state.dispatch(Action::SettingsFinished(settings, peers));
        }
        state.dispatch(Action::SettingsMutationFinished(
            result.map(|_| success.to_owned()),
        ));
    });
}

pub(super) fn dispatch_scan(state: UseReducerHandle<AppState>, csrf: String) {
    state.dispatch(Action::SettingsStarted);
    spawn_local(async move {
        let result = post_json_response::<_, NetworkScanResponseDto>(
            STA_SCAN_ENDPOINT,
            &csrf,
            &EmptyRequest {},
            "STA 扫描",
        )
        .await
        .map(|response| response.entries);
        state.dispatch(Action::ScanFinished(result));
    });
}

pub(super) fn dispatch_subscription_source(
    state: UseReducerHandle<AppState>,
    csrf: String,
    url: String,
) {
    state.dispatch(Action::SettingsStarted);
    spawn_local(async move {
        let result = async {
            post_json(
                SUBSCRIPTION_SOURCE_ENDPOINT,
                &csrf,
                &SubscriptionSourceRequest { url },
                "订阅来源保存和更新",
            )
            .await?;
            Ok::<_, String>(())
        }
        .await;
        if result.is_ok() {
            let settings = fetch_settings_data().await;
            let peers = fetch_tailscale_peers().await;
            state.dispatch(Action::SettingsFinished(settings, peers));
        }
        state.dispatch(Action::SettingsMutationFinished(
            result.map(|_| "订阅来源已保存并更新".to_owned()),
        ));
    });
}

pub(super) fn reveal_and_focus_after_render(reveal_node: NodeRef, focus_node: NodeRef) {
    spawn_local(async move {
        TimeoutFuture::new(0).await;
        if let Some(element) = reveal_node.cast::<HtmlElement>() {
            // Mobile Safari can reject focus after the async render boundary. Scrolling is
            // explicit so an inline confirmation inserted above the viewport remains visible.
            element.scroll_into_view();
        }
        if let Some(element) = focus_node.cast::<HtmlElement>() {
            let _ = element.focus();
        }
    });
}
