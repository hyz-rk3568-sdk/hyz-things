#![cfg(feature = "web")]

mod ui;

use std::{cell::Cell, rc::Rc};

use gloo_net::http::Request;
use gloo_timers::future::TimeoutFuture;
use hyz_router::domain::{
    panel::{
        DisplayRequest, PanelBootstrap, ProxyDelayRefreshRequest, ProxyGroup, ProxyModeRequest,
        ProxySelectionRequest,
    },
    status::{
        Component, ComponentState, LinkState, ProxyMode, ProxyState, SnapshotState, StatusSnapshot,
    },
};
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlElement, HtmlInputElement, HtmlSelectElement, RequestCredentials};
use yew::prelude::*;

use ui::*;

const STATUS_ENDPOINT: &str = "/api/v1/status";
const PANEL_ENDPOINT: &str = "/api/v1/panel";
const DISPLAY_ENDPOINT: &str = "/api/v1/control/display";
const PROXY_MODE_ENDPOINT: &str = "/api/v1/control/proxy/mode";
const PROXY_SELECTION_ENDPOINT: &str = "/api/v1/control/proxy/selection";
const PROXY_DELAYS_ENDPOINT: &str = "/api/v1/control/proxy/delays";
const AUTH_LOGIN_ENDPOINT: &str = "/api/v1/auth/login";
const AUTH_LOGOUT_ENDPOINT: &str = "/api/v1/auth/logout";
const AUTH_SESSION_ENDPOINT: &str = "/api/v1/auth/session";
const AUTH_PASSWORD_ENDPOINT: &str = "/api/v1/auth/password";
const NETWORK_CONFIG_ENDPOINT: &str = "/api/v1/network/config";
const NETWORK_PENDING_ENDPOINT: &str = "/api/v1/network/pending";
const STA_SCAN_ENDPOINT: &str = "/api/v1/control/network/sta/scan";
const STA_APPLY_ENDPOINT: &str = "/api/v1/control/network/sta/apply";
const AP_PREPARE_ENDPOINT: &str = "/api/v1/control/network/ap/prepare";
const AP_APPLY_ENDPOINT: &str = "/api/v1/control/network/ap/apply";
const AP_CONFIRM_ENDPOINT: &str = "/api/v1/control/network/ap/confirm";
const AP_CANCEL_ENDPOINT: &str = "/api/v1/control/network/ap/cancel";
const SUBSCRIPTION_ENDPOINT: &str = "/api/v1/proxy/subscription";
const SUBSCRIPTION_SOURCE_ENDPOINT: &str = "/api/v1/control/proxy/subscription/source";
const SUBSCRIPTION_REFRESH_ENDPOINT: &str = "/api/v1/control/proxy/subscription/refresh";
const POLL_DELAY_MS: u32 = 2_000;
const NETWORK_APPLY_PAINT_DELAY_MS: u32 = 150;
const MISSING: &str = "—";

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthSessionDto {
    authenticated: bool,
    must_change: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
enum WifiCountryDto {
    #[serde(rename = "AU")]
    Au,
    #[serde(rename = "BR")]
    Br,
    #[serde(rename = "CA")]
    Ca,
    #[serde(rename = "CN")]
    Cn,
    #[serde(rename = "DE")]
    De,
    #[serde(rename = "FR")]
    Fr,
    #[serde(rename = "GB")]
    Gb,
    #[serde(rename = "IN")]
    In,
    #[serde(rename = "JP")]
    Jp,
    #[serde(rename = "KR")]
    Kr,
    #[serde(rename = "NZ")]
    Nz,
    #[serde(rename = "SG")]
    Sg,
    #[serde(rename = "TW")]
    Tw,
    #[serde(rename = "US")]
    Us,
}

impl WifiCountryDto {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Au => "AU",
            Self::Br => "BR",
            Self::Ca => "CA",
            Self::Cn => "CN",
            Self::De => "DE",
            Self::Fr => "FR",
            Self::Gb => "GB",
            Self::In => "IN",
            Self::Jp => "JP",
            Self::Kr => "KR",
            Self::Nz => "NZ",
            Self::Sg => "SG",
            Self::Tw => "TW",
            Self::Us => "US",
        }
    }
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkConfigDto {
    version: u8,
    ap_ssid: String,
    sta_ssid: String,
    country: WifiCountryDto,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingConfigDto {
    version: u8,
    #[serde(rename = "staged_at_unix_ms")]
    _staged_at_unix_ms: u64,
    config: NetworkConfigDto,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkPendingDto {
    pending: Option<PendingConfigDto>,
    applied: bool,
    remaining_seconds: Option<u64>,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct WifiScanDto {
    ssid: String,
    bssid: String,
    frequency_mhz: u16,
    signal_dbm: i16,
    secured: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum SubscriptionStateDto {
    Idle,
    Fetching,
    Active,
    Failed,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SubscriptionDto {
    configured: bool,
    state: SubscriptionStateDto,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkConfigResponseDto {
    config: NetworkConfigDto,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkScanResponseDto {
    entries: Vec<WifiScanDto>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SubscriptionResponseDto {
    subscription: SubscriptionDto,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct EmptyRequest {}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    password: String,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct PasswordRequest {
    current_password: String,
    new_password: String,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct StaRequest {
    ssid: String,
    passphrase: String,
}

enum NetworkApplyIntent {
    Sta(StaRequest),
    Ap,
}

impl NetworkApplyIntent {
    const fn confirmation(&self) -> (&'static str, &'static str) {
        match self {
            Self::Sta(_) => (
                "应用上游 Wi-Fi？",
                "STA 切换可能让 AP 跟随新信道并短暂断开管理连接。确认后弹窗会先关闭，再开始应用。",
            ),
            Self::Ap => (
                "应用下游 AP？",
                "当前 AP 会立即断开。确认后弹窗会先关闭，请随后连接新的 AP，并在倒计时结束前确认保留。",
            ),
        }
    }
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ApRequest {
    ssid: String,
    passphrase: String,
    country: String,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct SubscriptionSourceRequest {
    url: String,
}

#[derive(Clone, PartialEq, Default)]
struct AppState {
    snapshot: Option<StatusSnapshot>,
    panel: Option<PanelBootstrap>,
    last_update: Option<String>,
    poll_error: Option<String>,
    display_notice: Option<String>,
    display_busy: bool,
    proxy_notice: Option<String>,
    proxy_busy: bool,
    loading: bool,
    session_checked: bool,
    session: Option<AuthSessionDto>,
    settings_notice: Option<String>,
    settings_busy: bool,
    network: Option<NetworkConfigDto>,
    pending_network: Option<NetworkPendingDto>,
    scan_entries: Vec<WifiScanDto>,
    subscription: Option<SubscriptionDto>,
}

#[derive(Clone, Copy)]
enum ControlArea {
    Display,
    Proxy,
}

enum Action {
    Started,
    Success(Box<StatusSnapshot>, Box<PanelBootstrap>, String),
    Failure(String),
    ControlStarted(ControlArea),
    ControlFinished(ControlArea, Result<String, String>),
    ProxyDelaysFinished(Result<Vec<ProxyGroup>, String>),
    SessionFinished(Result<AuthSessionDto, String>),
    AuthFinished(Result<(AuthSessionDto, String), String>),
    SettingsStarted,
    SettingsFinished(Result<(NetworkConfigDto, NetworkPendingDto, SubscriptionDto), String>),
    SettingsMutationFinished(Result<String, String>),
    ScanFinished(Result<Vec<WifiScanDto>, String>),
    SettingsNotice(String),
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
                    ControlArea::Proxy => {
                        next.proxy_busy = true;
                        next.proxy_notice = None;
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
                    ControlArea::Proxy => {
                        next.proxy_busy = false;
                        next.proxy_notice = notice;
                    }
                }
                next.into()
            }
            Action::ProxyDelaysFinished(result) => {
                let mut next = (*self).clone();
                next.proxy_busy = false;
                match result {
                    Ok(groups) => {
                        if let Some(bootstrap) = &mut next.panel {
                            bootstrap.panel.proxy_groups = Component::available(groups);
                        }
                        next.proxy_notice = Some("节点延迟已更新".to_owned());
                    }
                    Err(error) => {
                        next.proxy_notice = Some(format!("测速失败：{error}"));
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
            Action::SettingsFinished(result) => {
                let mut next = (*self).clone();
                next.settings_busy = false;
                match result {
                    Ok((network, pending, subscription)) => {
                        next.network = Some(network);
                        next.pending_network = Some(pending);
                        next.subscription = Some(subscription);
                    }
                    Err(error) => {
                        next.settings_notice = Some(format!("设置数据读取失败：{error}"));
                    }
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
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Good,
    Warn,
    Bad,
    Neutral,
}

impl Tone {
    fn class(self) -> &'static str {
        match self {
            Self::Good => "text-success",
            Self::Warn => "text-warning",
            Self::Bad => "text-error",
            Self::Neutral => "text-base-content/60",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkspaceView {
    Overview,
    Proxy,
    Network,
}

impl WorkspaceView {
    const fn tab_id(self) -> &'static str {
        match self {
            Self::Overview => "overview-tab",
            Self::Proxy => "proxy-tab",
            Self::Network => "network-tab",
        }
    }

    const fn panel_id(self) -> &'static str {
        match self {
            Self::Overview => "overview-panel",
            Self::Proxy => "proxy-panel",
            Self::Network => "network-panel",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Overview => "总览",
            Self::Proxy => "代理",
            Self::Network => "网络设置",
        }
    }
}

#[function_component(App)]
fn app() -> Html {
    let state = use_reducer(AppState::default);
    let brightness = use_state(|| 128u16);
    let delay_refresh_started = use_state(|| false);
    let active_view = use_state(|| WorkspaceView::Overview);

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
                    // Delay after completion so requests never overlap.
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
    let select_overview = {
        let active_view = active_view.clone();
        Callback::from(move |_| active_view.set(WorkspaceView::Overview))
    };
    let select_proxy = {
        let active_view = active_view.clone();
        Callback::from(move |_| active_view.set(WorkspaceView::Proxy))
    };
    let select_network = {
        let active_view = active_view.clone();
        Callback::from(move |_| active_view.set(WorkspaceView::Network))
    };
    let active = *active_view;

    html! {
        <main class={PAGE}>
            <header class={APP_HEADER}>
                <div class={BRAND}>
                    <span class={BRAND_MARK} aria-hidden="true">{"HYZ"}</span>
                    <div>
                        <p class={EYEBROW}>{"LOCAL CONTROL PLANE"}</p>
                        <h1 class={PAGE_TITLE}>{"HYZ Router"}</h1>
                        <p class={SUBTITLE}>{"单设备网络、代理与无线管理"}</p>
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
            <nav class={WORKSPACE_TABS} aria-label="管理视图">
                <button id={WorkspaceView::Overview.tab_id()} class={classes!(WORKSPACE_TAB, (active == WorkspaceView::Overview).then_some(WORKSPACE_TAB_ACTIVE))} type="button" aria-pressed={(active == WorkspaceView::Overview).to_string()} aria-controls={WorkspaceView::Overview.panel_id()} onclick={select_overview}>{WorkspaceView::Overview.label()}</button>
                <button id={WorkspaceView::Proxy.tab_id()} class={classes!(WORKSPACE_TAB, (active == WorkspaceView::Proxy).then_some(WORKSPACE_TAB_ACTIVE))} type="button" aria-pressed={(active == WorkspaceView::Proxy).to_string()} aria-controls={WorkspaceView::Proxy.panel_id()} onclick={select_proxy}>{WorkspaceView::Proxy.label()}</button>
                <button id={WorkspaceView::Network.tab_id()} class={classes!(WORKSPACE_TAB, (active == WorkspaceView::Network).then_some(WORKSPACE_TAB_ACTIVE))} type="button" aria-pressed={(active == WorkspaceView::Network).to_string()} aria-controls={WorkspaceView::Network.panel_id()} onclick={select_network}>{WorkspaceView::Network.label()}</button>
            </nav>
            <section id={WorkspaceView::Overview.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={WorkspaceView::Overview.tab_id()} hidden={active != WorkspaceView::Overview}>
                if let Some(snapshot) = &state.snapshot {
                    {render_topology(snapshot)}
                    {render_kpis(snapshot)}
                    {render_issues(snapshot)}
                    <div class={VIEW_HEADING}>
                        <div><p class={EYEBROW}>{"DETAILS"}</p><h2 class={SECTION_TITLE}>{"运行详情"}</h2></div>
                        <span class={SECTION_META}>{"保留最近一次成功快照"}</span>
                    </div>
                    {render_dashboard(snapshot)}
                } else if state.loading {
                    <section class={LOADING_GRID} aria-labelledby="loading-title" aria-busy="true">
                        <h2 id="loading-title" class="sr-only">{"正在加载路由器状态"}</h2>
                        {for (0..3).map(|_| html! { <div class={SKELETON} aria-hidden="true"></div> })}
                    </section>
                } else {
                    <section class={EMPTY_STATE} role="alert" aria-labelledby="empty-title">
                        <span class={EMPTY_ICON} aria-hidden="true">{"!"}</span>
                        <h2 id="empty-title" class={EMPTY_TITLE}>{"暂时无法读取状态"}</h2>
                        <p class={EMPTY_COPY}>{"面板会自动重试，无需刷新页面。"}</p>
                    </section>
                }
                {render_display_control(&state, brightness.clone())}
            </section>
            <section id={WorkspaceView::Proxy.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={WorkspaceView::Proxy.tab_id()} hidden={active != WorkspaceView::Proxy}>
                {render_proxy_control(&state)}
            </section>
            <section id={WorkspaceView::Network.panel_id()} class={WORKSPACE_PANEL} aria-labelledby={WorkspaceView::Network.tab_id()} hidden={active != WorkspaceView::Network}>
                <Settings state={state.clone()} />
            </section>
            <footer class={FOOTER}>{"数据约每 2 秒自动刷新 · 写操作仅接受同源令牌保护的类型化请求"}</footer>
        </main>
    }
}

async fn fetch_dashboard() -> Result<(StatusSnapshot, PanelBootstrap), String> {
    let status = fetch_json::<StatusSnapshot>(STATUS_ENDPOINT, "状态").await?;
    let panel = fetch_json::<PanelBootstrap>(PANEL_ENDPOINT, "控制面").await?;
    Ok((status, panel))
}

async fn fetch_json<T: serde::de::DeserializeOwned>(
    endpoint: &str,
    label: &str,
) -> Result<T, String> {
    let response = Request::get(endpoint)
        .credentials(RequestCredentials::SameOrigin)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|error| format!("无法连接{label}接口：{error}"))?;
    if !response.ok() {
        return Err(format!("{label}接口返回 HTTP {}", response.status()));
    }
    response
        .json::<T>()
        .await
        .map_err(|error| format!("{label}数据格式无效：{error}"))
}

fn dispatch_control<T>(
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

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum DelayRefreshControlResponse {
    ProxyDelays { groups: Vec<ProxyGroup> },
}

fn dispatch_delay_refresh(state: UseReducerHandle<AppState>, csrf_token: String) {
    state.dispatch(Action::ControlStarted(ControlArea::Proxy));
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

async fn post_json<T: serde::Serialize>(
    endpoint: &str,
    csrf: &str,
    body: &T,
    label: &str,
) -> Result<gloo_net::http::Response, String> {
    let request = Request::post(endpoint)
        .credentials(RequestCredentials::SameOrigin)
        .header("Accept", "application/json")
        .header("X-HYZ-CSRF", csrf)
        .json(body)
        .map_err(|error| format!("无法编码{label}请求：{error}"))?;
    let response = request
        .send()
        .await
        .map_err(|error| format!("无法连接{label}接口：{error}"))?;
    if response.ok() {
        Ok(response)
    } else {
        Err(format!("{label}接口返回 HTTP {}", response.status()))
    }
}

async fn post_json_response<T: serde::Serialize, R: serde::de::DeserializeOwned>(
    endpoint: &str,
    csrf: &str,
    body: &T,
    label: &str,
) -> Result<R, String> {
    post_json(endpoint, csrf, body, label)
        .await?
        .json::<R>()
        .await
        .map_err(|error| format!("{label}响应格式无效：{error}"))
}

async fn fetch_settings_data(
) -> Result<(NetworkConfigDto, NetworkPendingDto, SubscriptionDto), String> {
    let network = fetch_json::<NetworkConfigResponseDto>(NETWORK_CONFIG_ENDPOINT, "网络配置")
        .await?
        .config;
    let pending =
        fetch_json::<NetworkPendingDto>(NETWORK_PENDING_ENDPOINT, "待确认网络配置").await?;
    let subscription = fetch_json::<SubscriptionResponseDto>(SUBSCRIPTION_ENDPOINT, "订阅状态")
        .await?
        .subscription;
    Ok((network, pending, subscription))
}

fn dispatch_settings_refresh(state: UseReducerHandle<AppState>) {
    state.dispatch(Action::SettingsStarted);
    spawn_local(async move {
        state.dispatch(Action::SettingsFinished(fetch_settings_data().await));
    });
}

fn dispatch_auth<T: serde::Serialize + 'static>(
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

fn dispatch_settings_mutation<T: serde::Serialize + 'static>(
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
            state.dispatch(Action::SettingsFinished(fetch_settings_data().await));
        }
        state.dispatch(Action::SettingsMutationFinished(
            result.map(|_| success.to_owned()),
        ));
    });
}

fn dispatch_disruptive_settings_mutation<T: serde::Serialize + 'static>(
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
            state.dispatch(Action::SettingsFinished(fetch_settings_data().await));
        }
        state.dispatch(Action::SettingsMutationFinished(
            result.map(|_| success.to_owned()),
        ));
    });
}

fn dispatch_scan(state: UseReducerHandle<AppState>, csrf: String) {
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

fn dispatch_subscription_source(state: UseReducerHandle<AppState>, csrf: String, url: String) {
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
            state.dispatch(Action::SettingsFinished(fetch_settings_data().await));
        }
        state.dispatch(Action::SettingsMutationFinished(
            result.map(|_| "订阅来源已保存并更新".to_owned()),
        ));
    });
}

fn reveal_and_focus_after_render(reveal_node: NodeRef, focus_node: NodeRef) {
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

#[derive(Properties, PartialEq)]
struct SettingsProps {
    state: UseReducerHandle<AppState>,
}

struct ApSettingsRefs<'a> {
    ssid: &'a NodeRef,
    password: &'a NodeRef,
    country: &'a NodeRef,
    apply_button: &'a NodeRef,
}

struct ApSettingsActions {
    prepare: Callback<SubmitEvent>,
    apply: Callback<MouseEvent>,
    confirm: Callback<MouseEvent>,
    cancel: Callback<MouseEvent>,
}

#[function_component(Settings)]
fn settings(props: &SettingsProps) -> Html {
    let state = &props.state;
    let login_password = use_node_ref();
    let current_password = use_node_ref();
    let new_password = use_node_ref();
    let confirm_password = use_node_ref();
    let sta_ssid = use_node_ref();
    let sta_password = use_node_ref();
    let sta_toggle = use_node_ref();
    let sta_apply_button = use_node_ref();
    let ap_ssid = use_node_ref();
    let ap_password = use_node_ref();
    let ap_country = use_node_ref();
    let ap_toggle = use_node_ref();
    let ap_apply_button = use_node_ref();
    let network_confirmation_panel = use_node_ref();
    let confirmation_cancel_button = use_node_ref();
    let subscription_url = use_node_ref();
    let login_expanded = use_state(|| false);
    let sta_expanded = use_state(|| false);
    let ap_expanded = use_state(|| false);
    let network_confirmation_open = use_state(|| false);
    let network_apply_intent = use_mut_ref(|| None::<NetworkApplyIntent>);

    {
        let network_confirmation_panel = network_confirmation_panel.clone();
        let confirmation_cancel_button = confirmation_cancel_button.clone();
        use_effect_with(*network_confirmation_open, move |open| {
            if *open {
                reveal_and_focus_after_render(
                    network_confirmation_panel,
                    confirmation_cancel_button,
                );
            }
            || ()
        });
    }

    let csrf = state
        .panel
        .as_ref()
        .map(|panel| panel.csrf_token.clone())
        .unwrap_or_default();
    let session = state.session.as_ref();
    let authenticated = session.is_some_and(|session| session.authenticated);
    let must_change = session.is_some_and(|session| session.must_change);
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
    let logout = {
        let state = state.clone();
        let csrf = csrf.clone();
        let login_expanded = login_expanded.clone();
        Callback::from(move |_| {
            login_expanded.set(false);
            dispatch_auth(
                state.clone(),
                AUTH_LOGOUT_ENDPOINT,
                csrf.clone(),
                EmptyRequest {},
                "已退出登录",
            )
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
    let scan = {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |_| dispatch_scan(state.clone(), csrf.clone()))
    };
    let apply_sta = {
        let intent = network_apply_intent.clone();
        let confirmation_open = network_confirmation_open.clone();
        let ssid = sta_ssid.clone();
        let password = sta_password.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let (Some(ssid), Some(password)) = (
                ssid.cast::<HtmlInputElement>(),
                password.cast::<HtmlInputElement>(),
            ) else {
                return;
            };
            let request = StaRequest {
                ssid: ssid.value(),
                passphrase: password.value(),
            };
            password.set_value("");
            *intent.borrow_mut() = Some(NetworkApplyIntent::Sta(request));
            confirmation_open.set(true);
        })
    };
    let prepare_ap = {
        let state = state.clone();
        let csrf = csrf.clone();
        let ssid = ap_ssid.clone();
        let password = ap_password.clone();
        let country = ap_country.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let (Some(ssid), Some(password), Some(country)) = (
                ssid.cast::<HtmlInputElement>(),
                password.cast::<HtmlInputElement>(),
                country.cast::<HtmlSelectElement>(),
            ) else {
                return;
            };
            let request = ApRequest {
                ssid: ssid.value(),
                passphrase: password.value(),
                country: country.value(),
            };
            password.set_value("");
            dispatch_settings_mutation(
                state.clone(),
                AP_PREPARE_ENDPOINT,
                csrf.clone(),
                request,
                "AP 准备",
                "AP 配置已准备，请确认断线风险后应用",
            );
        })
    };
    let ap_action = |endpoint: &'static str, label: &'static str, success: &'static str| {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |_| {
            dispatch_settings_mutation(
                state.clone(),
                endpoint,
                csrf.clone(),
                EmptyRequest {},
                label,
                success,
            );
        })
    };
    let confirm_ap = ap_action(AP_CONFIRM_ENDPOINT, "AP 确认", "AP 配置已确认");
    let cancel_ap = ap_action(AP_CANCEL_ENDPOINT, "AP 取消", "AP 配置已取消并回滚");
    let request_ap_apply = {
        let intent = network_apply_intent.clone();
        let confirmation_open = network_confirmation_open.clone();
        Callback::from(move |_| {
            *intent.borrow_mut() = Some(NetworkApplyIntent::Ap);
            confirmation_open.set(true);
        })
    };
    let confirm_network_apply = {
        let state = state.clone();
        let csrf = csrf.clone();
        let intent = network_apply_intent.clone();
        let confirmation_open = network_confirmation_open.clone();
        let sta_expanded = sta_expanded.clone();
        let ap_expanded = ap_expanded.clone();
        let sta_toggle = sta_toggle.clone();
        let ap_toggle = ap_toggle.clone();
        Callback::from(move |_| {
            confirmation_open.set(false);
            let Some(intent) = intent.borrow_mut().take() else {
                return;
            };
            match intent {
                NetworkApplyIntent::Sta(request) => {
                    sta_expanded.set(false);
                    reveal_and_focus_after_render(sta_toggle.clone(), sta_toggle.clone());
                    dispatch_disruptive_settings_mutation(
                        state.clone(),
                        STA_APPLY_ENDPOINT,
                        csrf.clone(),
                        request,
                        "STA 应用",
                        "STA 配置已应用",
                    );
                }
                NetworkApplyIntent::Ap => {
                    ap_expanded.set(false);
                    reveal_and_focus_after_render(ap_toggle.clone(), ap_toggle.clone());
                    dispatch_disruptive_settings_mutation(
                        state.clone(),
                        AP_APPLY_ENDPOINT,
                        csrf.clone(),
                        EmptyRequest {},
                        "AP 应用",
                        "AP 正在切换，请连接新 AP 后确认",
                    );
                }
            }
        })
    };
    let cancel_network_apply = {
        let intent = network_apply_intent.clone();
        let confirmation_open = network_confirmation_open.clone();
        let sta_apply_button = sta_apply_button.clone();
        let ap_apply_button = ap_apply_button.clone();
        Callback::from(move |_| {
            let focus_target = match intent.borrow_mut().take() {
                Some(NetworkApplyIntent::Sta(_)) => sta_apply_button.clone(),
                Some(NetworkApplyIntent::Ap) => ap_apply_button.clone(),
                None => return,
            };
            confirmation_open.set(false);
            reveal_and_focus_after_render(focus_target.clone(), focus_target);
        })
    };
    let toggle_login = {
        let expanded = login_expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };
    let toggle_sta = {
        let expanded = sta_expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };
    let toggle_ap = {
        let expanded = ap_expanded.clone();
        Callback::from(move |_| expanded.set(!*expanded))
    };
    let network_confirmation = if *network_confirmation_open {
        network_apply_intent
            .borrow()
            .as_ref()
            .map(NetworkApplyIntent::confirmation)
    } else {
        None
    };
    let sta_summary = state.network.as_ref().map_or_else(
        || "读取中".to_owned(),
        |network| format!("当前 · {}", network.sta_ssid),
    );
    let ap_summary = state
        .pending_network
        .as_ref()
        .and_then(|pending| {
            pending
                .pending
                .as_ref()
                .map(|candidate| (pending.applied, candidate))
        })
        .map_or_else(
            || {
                state.network.as_ref().map_or_else(
                    || "读取中".to_owned(),
                    |network| format!("当前 · {}", network.ap_ssid),
                )
            },
            |(applied, candidate)| {
                format!(
                    "{} · {}",
                    if applied {
                        "等待确认"
                    } else {
                        "候选待应用"
                    },
                    candidate.config.ap_ssid
                )
            },
        );
    let save_subscription = {
        let state = state.clone();
        let csrf = csrf.clone();
        let input = subscription_url.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let Some(input) = input.cast::<HtmlInputElement>() else {
                return;
            };
            let url = input.value();
            input.set_value("");
            dispatch_subscription_source(state.clone(), csrf.clone(), url);
        })
    };
    let refresh_subscription = {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |_| {
            dispatch_settings_mutation(
                state.clone(),
                SUBSCRIPTION_REFRESH_ENDPOINT,
                csrf.clone(),
                EmptyRequest {},
                "订阅更新",
                "订阅已更新",
            )
        })
    };

    html! {
        <section class={SECTION} aria-labelledby="settings-title" aria-busy={busy.to_string()}>
            <div class={SECTION_HEAD_CENTERED}>
                <div>
                    <p class={EYEBROW}>{"ADMIN"}</p>
                    <h2 id="settings-title" class={SECTION_TITLE}>{"管理设置"}</h2>
                </div>
                if authenticated {
                    <div class={SESSION_ACTIONS}>
                        <span>{"管理员 · admin"}</span>
                        <button class={BUTTON_GHOST} type="button" onclick={logout} disabled={busy || csrf.is_empty()}>{"退出登录"}</button>
                    </div>
                } else {
                    <span class={SECTION_META}>{"状态面板无需登录，设置需要管理员身份"}</span>
                }
            </div>
            if let Some(notice) = &state.settings_notice {
                <div class={FEEDBACK} role="status" aria-live="polite" aria-atomic="true">{notice}</div>
            }
            if let Some((title, message)) = network_confirmation {
                <section ref={network_confirmation_panel} id="network-confirmation-panel" class={CONFIRMATION_PANEL} role="region" aria-live="assertive" aria-atomic="true" aria-labelledby="network-confirmation-title" aria-describedby="network-confirmation-message">
                    <div>
                        <p class={EYEBROW}>{"NETWORK CHANGE"}</p>
                        <h3 id="network-confirmation-title" class={CONFIRMATION_TITLE}>{title}</h3>
                    </div>
                    <p id="network-confirmation-message" class={CONFIRMATION_COPY}>{message}</p>
                    <div class={CONFIRMATION_ACTIONS}>
                        <button ref={confirmation_cancel_button} class={BUTTON} type="button" onclick={cancel_network_apply}>{"返回检查"}</button>
                        <button class={BUTTON_ERROR} type="button" onclick={confirm_network_apply}>{"确认并开始应用"}</button>
                    </div>
                </section>
            }
            if !state.session_checked {
                <div class={SETTINGS_EMPTY} role="status">{"正在检查登录状态…"}</div>
            } else if !authenticated {
                <article class={LOGIN_DISCLOSURE}>
                    <button id="admin-login-toggle" class={DISCLOSURE_TOGGLE} type="button" onclick={toggle_login} aria-expanded={login_expanded.to_string()} aria-controls="admin-login-detail">
                        <span class={DISCLOSURE_COPY}>
                            <strong class={DISCLOSURE_TITLE}>{"管理员登录"}</strong>
                            <small class={DISCLOSURE_SUMMARY}>{"设置保持锁定，状态面板仍可直接查看"}</small>
                        </span>
                        <span class={DISCLOSURE_ACTION} aria-hidden="true">{if *login_expanded { "收起" } else { "展开" }}</span>
                    </button>
                    if *login_expanded {
                        <form id="admin-login-detail" class={AUTH_FORM} role="region" aria-labelledby="admin-login-toggle" onsubmit={login} autocomplete="on">
                            <label class={FIELD}>
                                <span class={FIELD_LABEL}>{"用户名"}</span>
                                <input class={READONLY_INPUT} value="admin" readonly=true autocomplete="username" />
                            </label>
                            <label class={FIELD}>
                                <span class={FIELD_LABEL}>{"密码"}</span>
                                <input class={INPUT} ref={login_password} type="password" required=true autocomplete="current-password" />
                            </label>
                            <button class={BUTTON_BLOCK_MOBILE} type="submit" disabled={busy || csrf.is_empty()}>{if busy { "登录中…" } else { "登录" }}</button>
                            <small class={HELP_TEXT}>{"新设备首次登录密码为 admin；登录后必须立即修改。"}</small>
                        </form>
                    }
                </article>
            } else if must_change {
                <div class={FORCED_PASSWORD}>
                    <div class={classes!(RISK_ALERT, "alert-error", "border-error/20")} role="alert">
                        <div>
                            <strong>{"必须先修改默认密码"}</strong>
                            <p class={RISK_COPY}>{"新密码至少 12 字节，不能继续使用默认密码。完成前其他设置保持锁定。"}</p>
                        </div>
                    </div>
                    <form class={FORM_GRID} onsubmit={change_password} autocomplete="on" aria-describedby="password-policy">
                        <label class={FIELD}><span class={FIELD_LABEL}>{"当前密码"}</span><input class={INPUT} ref={current_password} type="password" required=true autocomplete="current-password" /></label>
                        <label class={FIELD}><span class={FIELD_LABEL}>{"新密码"}</span><input class={INPUT} ref={new_password} type="password" required=true maxlength="1024" autocomplete="new-password" /></label>
                        <label class={FIELD}><span class={FIELD_LABEL}>{"确认新密码"}</span><input class={INPUT} ref={confirm_password} type="password" required=true maxlength="1024" autocomplete="new-password" /></label>
                        <p id="password-policy" class="sr-only">{"新密码长度必须为 12 至 1024 字节，且两次输入必须一致。"}</p>
                        <div class={FORM_ACTIONS}><button class={BUTTON_PRIMARY} type="submit" disabled={busy || csrf.is_empty()}>{"修改密码"}</button></div>
                    </form>
                </div>
            } else {
                <div class={SETTINGS_GRID}>
                    <article class={DISCLOSURE}>
                        <button id="sta-settings-toggle" ref={sta_toggle} class={DISCLOSURE_TOGGLE} type="button" onclick={toggle_sta} aria-expanded={sta_expanded.to_string()} aria-controls="sta-settings-detail">
                            <span class={DISCLOSURE_COPY}><strong class={DISCLOSURE_TITLE}>{"上游 Wi-Fi (STA)"}</strong><small class={DISCLOSURE_SUMMARY}>{sta_summary}</small></span>
                            <span class={DISCLOSURE_ACTION} aria-hidden="true">{if *sta_expanded { "收起" } else { "展开" }}</span>
                        </button>
                        if *sta_expanded {
                            <div id="sta-settings-detail" class={DISCLOSURE_DETAIL} role="region" aria-labelledby="sta-settings-toggle">
                                <div class={DETAIL_TOOLBAR}><small class={HELP_TEXT}>{"扫描附近网络，或手工填写新的上游 Wi-Fi。"}</small><button class={BUTTON} type="button" onclick={scan} disabled={busy}>{if busy { "处理中…" } else { "扫描" }}</button></div>
                                if !state.scan_entries.is_empty() {
                                    <div class={SCAN_LIST} role="group" aria-label="扫描到的 Wi-Fi">
                                        {for state.scan_entries.iter().map(|entry| {
                                            let input = sta_ssid.clone();
                                            let ssid = entry.ssid.clone();
                                            let choose = Callback::from(move |_| { if let Some(input) = input.cast::<HtmlInputElement>() { input.set_value(&ssid); } });
                                            html! { <button type="button" class={SCAN_ENTRY} onclick={choose} disabled={busy} aria-label={format!("选择网络 {}", entry.ssid)}><strong class={SCAN_NAME}>{&entry.ssid}</strong><span class={SCAN_META}>{format!("{} MHz · {} dBm · {}", entry.frequency_mhz, entry.signal_dbm, if entry.secured { "加密" } else { "开放" })}</span></button> }
                                        })}
                                    </div>
                                }
                                <form class={FORM_GRID_COMPACT} onsubmit={apply_sta} autocomplete="off">
                                    <label class={FIELD}><span class={FIELD_LABEL}>{"SSID"}</span><input class={INPUT} ref={sta_ssid} required=true maxlength="32" autocomplete="off" /></label>
                                    <label class={FIELD}><span class={FIELD_LABEL}>{"密码"}</span><input class={INPUT} ref={sta_password} type="password" required=true minlength="8" maxlength="63" autocomplete="new-password" /></label>
                                    <div class={RISK_NOTE} role="note">{"若 STA 与当前 AP 信道不同，设备可能重启 AP 跟随信道，管理连接会短暂断开。"}</div>
                                    <div class={FORM_ACTIONS}><button ref={sta_apply_button} class={BUTTON_PRIMARY} type="submit" disabled={busy}>{"检查并应用 STA"}</button></div>
                                </form>
                            </div>
                        }
                    </article>
                    <article class={DISCLOSURE}>
                        <button id="ap-settings-toggle" ref={ap_toggle} class={DISCLOSURE_TOGGLE} type="button" onclick={toggle_ap} aria-expanded={ap_expanded.to_string()} aria-controls="ap-settings-detail">
                            <span class={DISCLOSURE_COPY}><strong class={DISCLOSURE_TITLE}>{"下游 Wi-Fi (AP)"}</strong><small class={DISCLOSURE_SUMMARY}>{ap_summary}</small></span>
                            <span class={DISCLOSURE_ACTION} aria-hidden="true">{if *ap_expanded { "收起" } else { "展开" }}</span>
                        </button>
                        if *ap_expanded {
                            <div id="ap-settings-detail" class={DISCLOSURE_DETAIL} role="region" aria-labelledby="ap-settings-toggle">
                                {render_ap_settings(
                                    state,
                                    ApSettingsRefs { ssid: &ap_ssid, password: &ap_password, country: &ap_country, apply_button: &ap_apply_button },
                                    ApSettingsActions { prepare: prepare_ap, apply: request_ap_apply, confirm: confirm_ap, cancel: cancel_ap },
                                    busy,
                                )}
                            </div>
                        }
                    </article>
                    <article class={INNER_CARD} aria-labelledby="subscription-title">
                        <div class={CONTROL_TITLE}><h3 id="subscription-title" class={CONTROL_HEADING}>{"代理订阅"}</h3><span class={CONTROL_META}>{"来源只写"}</span></div>
                        if let Some(subscription) = &state.subscription {
                            <div class={SUMMARY}><span>{if subscription.configured { "已配置" } else { "未配置" }}</span><strong class={subscription_tone(subscription.state)}>{subscription_state_label(subscription.state)}</strong></div>
                        }
                        <form class={FORM_GRID_COMPACT} onsubmit={save_subscription} autocomplete="off">
                            <label class={FIELD}><span class={FIELD_LABEL}>{"订阅 URL"}</span><input class={INPUT} ref={subscription_url} type="url" required=true placeholder="https://…" autocomplete="off" autocapitalize="none" spellcheck="false" aria-describedby="subscription-secret-note" /></label>
                            <small id="subscription-secret-note" class={HELP_TEXT}>{"已保存的 URL 永不回显；输入只用于本次提交。"}</small>
                            <div class={FORM_ACTIONS}><button class={BUTTON_PRIMARY} type="submit" disabled={busy}>{"保存并立即更新"}</button><button class={BUTTON} type="button" onclick={refresh_subscription} disabled={busy || !state.subscription.as_ref().is_some_and(|value| value.configured)}>{"手动刷新"}</button></div>
                        </form>
                    </article>
                </div>
            }
        </section>
    }
}

fn render_ap_settings(
    state: &AppState,
    refs: ApSettingsRefs<'_>,
    actions: ApSettingsActions,
    busy: bool,
) -> Html {
    let pending = state
        .pending_network
        .as_ref()
        .and_then(|value| value.pending.as_ref());
    let applied = state
        .pending_network
        .as_ref()
        .is_some_and(|value| value.applied);
    if let Some(pending) = pending {
        let seconds = state
            .pending_network
            .as_ref()
            .and_then(|value| value.remaining_seconds)
            .unwrap_or(0);
        html! {
            <div class={PENDING_AP}>
                <div class={CONFIG_SUMMARY}>
                    <span class={CONFIG_LABEL}>{"候选 AP"}</span>
                    <strong class={CONFIG_VALUE}>{&pending.config.ap_ssid}</strong>
                    <small class={CONFIG_META}>{format!("国家 / 地区 · {}", pending.config.country.as_str())}</small>
                </div>
                if applied {
                    <div class={classes!(RISK_ALERT, "alert-warning", "border-warning/20")} role="alert">
                        <div><strong>{format!("等待确认 · 后端剩余约 {seconds} 秒")}</strong><p class={RISK_COPY}>{"请连接新的 AP 后确认。剩余时间以路由器为准，重新打开页面会刷新；到期会自动回滚。"}</p></div>
                    </div>
                    <div class={FORM_ACTIONS}><button class={BUTTON_PRIMARY} type="button" onclick={actions.confirm.clone()} disabled={busy}>{"确认保留"}</button><button class={BUTTON} type="button" onclick={actions.cancel.clone()} disabled={busy}>{"取消并回滚"}</button></div>
                } else {
                    <div class={classes!(RISK_ALERT, "alert-error", "border-error/20")} role="alert">
                        <div><strong>{"应用会立即断开当前 AP 连接"}</strong><p class={RISK_COPY}>{"请先记住新 SSID 和密码。应用后连接新 AP，再回到本页确认；未确认会自动回滚。"}</p></div>
                    </div>
                    <div class={FORM_ACTIONS}><button ref={refs.apply_button.clone()} class={BUTTON_ERROR} type="button" onclick={actions.apply.clone()} disabled={busy}>{"检查风险并应用"}</button><button class={BUTTON} type="button" onclick={actions.cancel.clone()} disabled={busy}>{"取消"}</button></div>
                }
            </div>
        }
    } else {
        html! {
            <>
                if let Some(network) = &state.network {
                    <div class={CONFIG_SUMMARY}><span class={CONFIG_LABEL}>{"当前配置"}</span><strong class={CONFIG_VALUE}>{&network.ap_ssid}</strong><small class={CONFIG_META}>{format!("国家 / 地区 · {}", network.country.as_str())}</small></div>
                }
                <form class={FORM_GRID_COMPACT} onsubmit={actions.prepare} autocomplete="off">
                    <label class={FIELD}><span class={FIELD_LABEL}>{"SSID"}</span><input class={INPUT} ref={refs.ssid.clone()} required=true maxlength="32" autocomplete="off" /></label>
                    <label class={FIELD}><span class={FIELD_LABEL}>{"密码"}</span><input class={INPUT} ref={refs.password.clone()} type="password" required=true minlength="8" maxlength="63" autocomplete="new-password" /></label>
                    <label class={FIELD}><span class={FIELD_LABEL}>{"国家 / 地区"}</span><select class={SELECT} ref={refs.country.clone()}><option value="CN">{"中国 (CN)"}</option><option value="US">{"美国 (US)"}</option><option value="JP">{"日本 (JP)"}</option><option value="SG">{"新加坡 (SG)"}</option><option value="TW">{"中国台湾 (TW)"}</option><option value="AU">{"澳大利亚 (AU)"}</option><option value="BR">{"巴西 (BR)"}</option><option value="CA">{"加拿大 (CA)"}</option><option value="DE">{"德国 (DE)"}</option><option value="FR">{"法国 (FR)"}</option><option value="GB">{"英国 (GB)"}</option><option value="IN">{"印度 (IN)"}</option><option value="KR">{"韩国 (KR)"}</option><option value="NZ">{"新西兰 (NZ)"}</option></select></label>
                    <div class={FORM_ACTIONS}><button class={BUTTON_PRIMARY} type="submit" disabled={busy}>{"准备 AP 变更"}</button></div>
                </form>
            </>
        }
    }
}

fn subscription_state_label(state: SubscriptionStateDto) -> &'static str {
    match state {
        SubscriptionStateDto::Idle => "空闲",
        SubscriptionStateDto::Fetching => "更新中",
        SubscriptionStateDto::Active => "已生效",
        SubscriptionStateDto::Failed => "更新失败",
    }
}

fn subscription_tone(state: SubscriptionStateDto) -> &'static str {
    match state {
        SubscriptionStateDto::Active => "text-success",
        SubscriptionStateDto::Fetching => "text-warning",
        SubscriptionStateDto::Failed => "text-error",
        SubscriptionStateDto::Idle => "text-base-content/60",
    }
}

fn current_time() -> String {
    let value = js_sys::Date::new_0()
        .to_time_string()
        .as_string()
        .unwrap_or_default();
    value.get(0..8).unwrap_or(value.as_str()).to_owned()
}

fn overall_status(state: &AppState) -> (&'static str, Tone) {
    match (&state.snapshot, &state.poll_error) {
        (None, Some(_)) => ("连接失败", Tone::Bad),
        (None, None) => ("正在获取", Tone::Neutral),
        (Some(_), Some(_)) => ("数据已降级", Tone::Warn),
        (Some(snapshot), None) if snapshot.state == SnapshotState::Degraded => {
            ("部分异常", Tone::Warn)
        }
        (Some(_), None) => ("运行正常", Tone::Good),
    }
}

fn render_notice(state: &AppState) -> Html {
    match (&state.poll_error, &state.snapshot) {
        (Some(error), Some(_)) => html! {
            <div class={classes!(NOTICE, "alert-warning", "border-warning/20")} role="alert">
                <strong>{"刷新失败，正在展示最近一次数据"}</strong><span class={NOTICE_COPY}>{error}</span>
            </div>
        },
        (Some(error), None) => html! {
            <div class={classes!(NOTICE, "alert-error", "border-error/20")} role="alert">
                <strong>{"状态服务不可用"}</strong><span class={NOTICE_COPY}>{error}</span>
            </div>
        },
        _ => Html::default(),
    }
}

fn render_topology(snapshot: &StatusSnapshot) -> Html {
    let router = snapshot.router.data.as_ref();
    let proxy = snapshot.proxy.data.as_ref();
    let internet_tone = match router {
        Some(router)
            if router.sta_state == Some(LinkState::Up)
                && router.default_route_present == Some(true) =>
        {
            Tone::Good
        }
        Some(router) if router.sta_state == Some(LinkState::Connecting) => Tone::Warn,
        Some(router)
            if router.sta_state == Some(LinkState::Down)
                || router.default_route_present == Some(false) =>
        {
            Tone::Bad
        }
        _ => Tone::Neutral,
    };
    let sta_detail = router.map_or_else(
        || "等待链路数据".to_owned(),
        |router| {
            let ssid = router.sta_ssid.as_deref().unwrap_or(MISSING);
            router.sta_signal_dbm.map_or_else(
                || ssid.to_owned(),
                |signal| format!("{ssid} · {signal} dBm"),
            )
        },
    );
    let router_detail = router.map_or_else(
        || "等待转发状态".to_owned(),
        |router| {
            format!(
                "转发 {} · NAT {}",
                router
                    .ipv4_forwarding
                    .map(format_bool)
                    .unwrap_or_else(missing),
                router
                    .masquerade_enabled
                    .map(format_bool)
                    .unwrap_or_else(missing)
            )
        },
    );
    let ap_detail = router.map_or_else(
        || "等待 LAN 状态".to_owned(),
        |router| {
            format!(
                "{} · {} 台客户端",
                router.lan_address.as_deref().unwrap_or(MISSING),
                router.ap_client_count.unwrap_or(0)
            )
        },
    );
    let proxy_detail = proxy.map_or_else(
        || "代理状态不可用".to_owned(),
        |proxy| {
            format!(
                "{} · {}",
                proxy_mode_label(proxy.mode),
                proxy_state_label(proxy.state)
            )
        },
    );

    html! {
        <section class={TOPOLOGY} aria-labelledby="topology-title">
            <div class={SECTION_HEAD_CENTERED}>
                <div><p class={EYEBROW}>{"PATH"}</p><h2 id="topology-title" class={SECTION_TITLE}>{"网络拓扑"}</h2></div>
                <span class={SECTION_META}>{"从上游连接到管理 LAN 的实时路径"}</span>
            </div>
            <div class={TOPOLOGY_FLOW}>
                {topology_node("WAN", "互联网", router.and_then(|value| value.sta_address.clone()).unwrap_or_else(|| "等待 WAN 地址".to_owned()), internet_tone)}
                {topology_link(internet_tone)}
                {topology_node("STA", "上游 Wi-Fi", sta_detail, component_tone(&snapshot.router))}
                {topology_link(component_tone(&snapshot.router))}
                {topology_node("RTR", "Router / NAT", router_detail, component_tone(&snapshot.router))}
                {topology_link(component_tone(&snapshot.router))}
                {topology_node("LAN", "AP / LAN", ap_detail, component_tone(&snapshot.router))}
            </div>
            <div class={TOPOLOGY_PROXY_ROW}>
                <span class={TOPOLOGY_BRANCH} aria-hidden="true">{"↳"}</span>
                {topology_node("TUN", "Mihomo / TUN", proxy_detail, component_tone(&snapshot.proxy))}
            </div>
        </section>
    }
}

fn topology_node(icon: &'static str, title: &'static str, detail: String, tone: Tone) -> Html {
    html! {
        <article class={TOPOLOGY_NODE}>
            <span class={classes!(TOPOLOGY_ICON, tone.class())} aria-hidden="true">{icon}</span>
            <span class={TOPOLOGY_COPY}><strong class={TOPOLOGY_TITLE}>{title}</strong><small class={TOPOLOGY_DETAIL} title={detail.clone()}>{detail}</small></span>
        </article>
    }
}

fn topology_link(tone: Tone) -> Html {
    html! { <span class={classes!(TOPOLOGY_LINK, tone.class())} aria-hidden="true">{"→"}</span> }
}

fn component_tone<T>(component: &Component<T>) -> Tone {
    match component.state {
        ComponentState::Available => Tone::Good,
        ComponentState::Degraded => Tone::Warn,
        ComponentState::Unavailable => Tone::Bad,
    }
}

fn render_kpis(snapshot: &StatusSnapshot) -> Html {
    let router = snapshot.router.data.as_ref();
    let proxy = snapshot.proxy.data.as_ref();
    let wan = router
        .and_then(|value| value.sta_address.clone())
        .unwrap_or_else(missing);
    let sta = router.map_or_else(missing, |router| {
        let ssid = router.sta_ssid.as_deref().unwrap_or(MISSING);
        router.sta_signal_dbm.map_or_else(
            || ssid.to_owned(),
            |signal| format!("{ssid} · {signal} dBm"),
        )
    });
    let proxy_mode = proxy
        .map(|value| proxy_mode_label(value.mode).to_owned())
        .unwrap_or_else(missing);
    let clients = router
        .and_then(|value| value.ap_client_count)
        .map(|value| format!("{value} 台"))
        .unwrap_or_else(missing);

    html! {
        <section class={KPI_GRID} aria-label="关键网络指标">
            {kpi("WAN IPv4", wan, "上游地址")}
            {kpi("上游 Wi-Fi", sta, "当前连接")}
            {kpi("代理模式", proxy_mode, "Mihomo")}
            {kpi("AP 客户端", clients, "下游设备")}
        </section>
    }
}

fn kpi(label: &'static str, value: String, meta: &'static str) -> Html {
    html! {
        <article class={KPI_CARD}>
            <span class={KPI_LABEL}>{label}</span>
            <strong class={KPI_VALUE} title={value.clone()}>{value}</strong>
            <small class={KPI_META}>{meta}</small>
        </article>
    }
}

fn render_dashboard(snapshot: &StatusSnapshot) -> Html {
    let router = &snapshot.router;
    let proxy = &snapshot.proxy;
    let system = &snapshot.system;
    let router_rows = vec![
        (
            "Wi-Fi",
            router
                .data
                .as_ref()
                .and_then(|v| v.sta_ssid.clone())
                .unwrap_or_else(missing),
        ),
        (
            "WAN IPv4",
            router
                .data
                .as_ref()
                .and_then(|v| v.sta_address.clone())
                .unwrap_or_else(missing),
        ),
        (
            "WAN 链路",
            router
                .data
                .as_ref()
                .and_then(|v| v.sta_state)
                .map(link_state_label)
                .unwrap_or(MISSING)
                .to_owned(),
        ),
        (
            "LAN 地址",
            router
                .data
                .as_ref()
                .and_then(|v| v.lan_address.clone())
                .unwrap_or_else(missing),
        ),
        (
            "AP 客户端",
            router
                .data
                .as_ref()
                .and_then(|v| v.ap_client_count)
                .map(|v| format!("{v} 台"))
                .unwrap_or_else(missing),
        ),
        (
            "IPv4 转发",
            router
                .data
                .as_ref()
                .and_then(|v| v.ipv4_forwarding)
                .map(format_bool)
                .unwrap_or_else(missing),
        ),
    ];
    let proxy_rows = vec![
        (
            "Mihomo",
            proxy
                .data
                .as_ref()
                .map(|v| proxy_state_label(v.state).to_owned())
                .unwrap_or_else(missing),
        ),
        (
            "运行模式",
            proxy
                .data
                .as_ref()
                .map(|v| proxy_mode_label(v.mode).to_owned())
                .unwrap_or_else(missing),
        ),
        (
            "配置就绪",
            proxy
                .data
                .as_ref()
                .and_then(|v| v.configured)
                .map(format_bool)
                .unwrap_or_else(missing),
        ),
        (
            "普通 NAT 回退",
            proxy
                .data
                .as_ref()
                .and_then(|v| v.ordinary_nat_fallback)
                .map(format_bool)
                .unwrap_or_else(missing),
        ),
    ];
    let system_rows = vec![
        (
            "运行时间",
            system
                .data
                .as_ref()
                .and_then(|v| v.uptime_seconds)
                .map(format_uptime)
                .unwrap_or_else(missing),
        ),
        (
            "CPU 温度",
            system
                .data
                .as_ref()
                .and_then(|v| v.cpu_temperature_millidegrees)
                .map(|v| format!("{:.1} °C", v as f64 / 1_000.0))
                .unwrap_or_else(missing),
        ),
        (
            "网络接口",
            system
                .data
                .as_ref()
                .map(|v| format!("{} 个", v.interfaces.len()))
                .unwrap_or_else(missing),
        ),
        (
            "WAN 接收",
            wan_traffic(system)
                .map(|v| format_bytes(v.0))
                .unwrap_or_else(missing),
        ),
        (
            "WAN 发送",
            wan_traffic(system)
                .map(|v| format_bytes(v.1))
                .unwrap_or_else(missing),
        ),
    ];

    html! {
        <section class={DASHBOARD} aria-labelledby="dashboard-title">
            <h2 id="dashboard-title" class="sr-only">{"路由器状态"}</h2>
            if router.data.is_some() {
                {status_card("路由 / LAN", "NET", component_card_status(router), router_rows, "wan")}
            }
            if proxy.data.is_some() {
                {status_card("Mihomo / TUN", "TUN", component_card_status(proxy), proxy_rows, "proxy")}
            }
            if system.data.is_some() {
                {status_card("系统 / 流量", "SYS", component_card_status(system), system_rows, "system")}
            }
        </section>
    }
}

fn render_display_control(
    state: &UseReducerHandle<AppState>,
    brightness: UseStateHandle<u16>,
) -> Html {
    let Some(bootstrap) = state.panel.as_ref() else {
        return Html::default();
    };
    let csrf = bootstrap.csrf_token.clone();
    let busy = state.display_busy;
    let display = bootstrap.panel.display.data.as_ref();
    let max_brightness = display.map_or(255, |display| display.max_brightness.max(1));
    let display_label = display.map_or_else(
        || "显示状态不可用".to_owned(),
        |display| {
            if display.enabled {
                format!(
                    "已点亮 · {} / {}",
                    display.actual_brightness, display.max_brightness
                )
            } else {
                "默认黑屏 · 背光已关闭".to_owned()
            }
        },
    );

    let on_brightness = {
        let brightness = brightness.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            if let Ok(value) = input.value().parse::<u16>() {
                brightness.set(value.max(1));
            }
        })
    };
    let display_on = {
        let state = state.clone();
        let csrf = csrf.clone();
        let brightness = *brightness;
        Callback::from(move |_| {
            dispatch_control(
                state.clone(),
                ControlArea::Display,
                DISPLAY_ENDPOINT,
                csrf.clone(),
                DisplayRequest {
                    enabled: true,
                    brightness: Some(brightness),
                },
                "背光已开启".to_owned(),
            )
        })
    };
    let display_off = {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |_| {
            dispatch_control(
                state.clone(),
                ControlArea::Display,
                DISPLAY_ENDPOINT,
                csrf.clone(),
                DisplayRequest {
                    enabled: false,
                    brightness: None,
                },
                "背光已关闭".to_owned(),
            )
        })
    };

    html! {
        if display.is_some() {
            <section class={SECTION} aria-labelledby="display-controls-title" aria-busy={busy.to_string()}>
                <div class={SECTION_HEAD}>
                    <div><p class={EYEBROW}>{"QUICK CONTROL"}</p><h2 id="display-controls-title" class={SECTION_TITLE}>{"设备快捷控制"}</h2></div>
                    <span class={SECTION_META}>{"仅限管理 LAN · 同源令牌保护"}</span>
                </div>
                <div class={classes!(FEEDBACK, state.display_notice.is_none().then_some("invisible"))} role="status" aria-live="polite" aria-atomic="true">
                    {state.display_notice.as_deref().unwrap_or("等待操作")}
                </div>
                <article class={INNER_CARD} aria-labelledby="display-control-title">
                    <div class={CONTROL_TITLE}><h3 id="display-control-title" class={CONTROL_HEADING}>{"LCD 背光"}</h3><span class={CONTROL_META}>{display_label}</span></div>
                    <label class={RANGE_LABEL} for="brightness"><span>{"点亮亮度"}</span><strong>{*brightness}</strong></label>
                    <input class={RANGE} id="brightness" type="range" min="1" max={max_brightness.to_string()} value={(*brightness).min(max_brightness).to_string()} oninput={on_brightness} disabled={busy} />
                    <div class={BUTTON_ROW}><button class={BUTTON_PRIMARY} type="button" onclick={display_on} disabled={busy}>{"点亮"}</button><button class={BUTTON} type="button" onclick={display_off} disabled={busy}>{"黑屏"}</button></div>
                    <small class={HELP_TEXT}>{"黑屏会将 PWM 亮度设为 0；面板 5V 是共享电源，无法单独物理断开。"}</small>
                </article>
            </section>
        }
    }
}

fn render_proxy_control(state: &UseReducerHandle<AppState>) -> Html {
    let Some(bootstrap) = state.panel.as_ref() else {
        return Html::default();
    };
    let csrf = bootstrap.csrf_token.clone();
    let busy = state.proxy_busy;
    let proxy_status = state
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.proxy.data.as_ref());
    let proxy_mode_button = |mode: ProxyMode, message: &'static str| {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |_| {
            dispatch_control(
                state.clone(),
                ControlArea::Proxy,
                PROXY_MODE_ENDPOINT,
                csrf.clone(),
                ProxyModeRequest { mode },
                message.to_owned(),
            )
        })
    };

    html! {
        <section class={classes!(SECTION, "gap-6")} aria-labelledby="proxy-controls-title" aria-busy={busy.to_string()}>
            <div class={SECTION_HEAD}>
                <div><p class={EYEBROW}>{"PROXY"}</p><h2 id="proxy-controls-title" class={SECTION_TITLE}>{"代理路径与节点"}</h2></div>
                <span class={SECTION_META}>{"模式切换按 fail-open 顺序收敛"}</span>
            </div>
            <div class={classes!(FEEDBACK, state.proxy_notice.is_none().then_some("invisible"))} role="status" aria-live="polite" aria-atomic="true">
                {state.proxy_notice.as_deref().unwrap_or("等待操作")}
            </div>
            if let Some(proxy) = proxy_status {
                <div class={PROXY_SUMMARY} aria-label="代理状态概览">
                    <article class={PROXY_STAT}><span class={PROXY_STAT_LABEL}>{"当前状态"}</span><strong class={PROXY_STAT_VALUE}>{proxy_state_label(proxy.state)}</strong></article>
                    <article class={PROXY_STAT}><span class={PROXY_STAT_LABEL}>{"运行模式"}</span><strong class={PROXY_STAT_VALUE}>{proxy_mode_label(proxy.mode)}</strong></article>
                    <article class={PROXY_STAT}><span class={PROXY_STAT_LABEL}>{"配置状态"}</span><strong class={PROXY_STAT_VALUE}>{proxy.configured.map(|configured| if configured { "已就绪" } else { "未配置" }).unwrap_or(MISSING)}</strong></article>
                </div>
                <article class={INNER_CARD} aria-labelledby="proxy-mode-title">
                    <div class={CONTROL_TITLE}><h3 id="proxy-mode-title" class={CONTROL_HEADING}>{"Mihomo 模式"}</h3><span class={CONTROL_META}>{"停用后保留订阅配置"}</span></div>
                    <div class={BUTTON_ROW} role="group" aria-label="Mihomo 运行模式">
                        <button class={BUTTON} type="button" onclick={proxy_mode_button(ProxyMode::Tun, "已切换到 TUN 模式")} disabled={busy}>{"TUN"}</button>
                        <button class={BUTTON} type="button" onclick={proxy_mode_button(ProxyMode::Explicit, "已切换到显式代理")} disabled={busy}>{"显式代理"}</button>
                        <button class={BUTTON} type="button" onclick={proxy_mode_button(ProxyMode::Disabled, "Mihomo 已停用")} disabled={busy}>{"停用"}</button>
                    </div>
                    <small class={HELP_TEXT}>{"普通 NAT 在路由启用时保持可用；浏览器不能直连 Mihomo controller。"}</small>
                </article>
            } else {
                <div class={SETTINGS_EMPTY} role="status">{"代理状态暂不可用"}</div>
            }
            <div class={PROXY_GROUPS}>
                {render_proxy_groups(&bootstrap.panel.proxy_groups, state, &csrf, busy)}
            </div>
        </section>
    }
}

fn render_proxy_groups(
    component: &Component<Vec<ProxyGroup>>,
    state: &UseReducerHandle<AppState>,
    csrf: &str,
    busy: bool,
) -> Html {
    let Some(groups) = component.data.as_ref() else {
        return Html::default();
    };
    if groups.is_empty() {
        return Html::default();
    }
    let refresh_delays = {
        let state = state.clone();
        let csrf = csrf.to_owned();
        Callback::from(move |_| dispatch_delay_refresh(state.clone(), csrf.clone()))
    };
    html! {
        <>
            <div class={PROXY_TOOLBAR}>
                <span>{"进入页面时自动测速一次"}</span>
                <button class={BUTTON} type="button" onclick={refresh_delays} disabled={busy}>{if busy { "测速中…" } else { "重新测速全部节点" }}</button>
            </div>
            {for groups.iter().map(|group| {
                let group_name = group.name.clone();
                let selection_state = state.clone();
                let selection_csrf = csrf.to_owned();
                let on_selection = Callback::from(move |event: Event| {
                    let select: HtmlSelectElement = event.target_unchecked_into();
                    dispatch_control(
                        selection_state.clone(),
                        ControlArea::Proxy,
                        PROXY_SELECTION_ENDPOINT,
                        selection_csrf.clone(),
                        ProxySelectionRequest { group: group_name.clone(), proxy: select.value() },
                        "代理节点已切换".to_owned(),
                    );
                });
                let selected = group.selected.clone();
                html! {
                    <article class={PROXY_GROUP} key={group.name.clone()}>
                        <div class={PROXY_NAME_WRAP}><h3 class={PROXY_NAME}>{&group.name}</h3><span class={PROXY_KIND}>{group_kind_label(group)}</span></div>
                        <select class={SELECT} value={selected.clone().unwrap_or_default()} onchange={on_selection} disabled={busy || !group.selectable} aria-label={format!("{} 节点", group.name)}>
                            {for group.options.iter().map(|option| {
                                let delay = option
                                    .delay_ms
                                    .map(|delay| format!("{delay} ms"))
                                    .or_else(|| (option.alive == Some(false)).then(|| "超时".to_owned()));
                                let details = [option.region.clone(), delay].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                                let label = if details.is_empty() { option.name.clone() } else { format!("{} · {details}", option.name) };
                                html! { <option key={option.name.clone()} value={option.name.clone()} selected={group.selected.as_deref() == Some(option.name.as_str())}>{label}</option> }
                            })}
                        </select>
                    </article>
                }
            })}
        </>
    }
}

fn group_kind_label(group: &ProxyGroup) -> &'static str {
    use hyz_router::domain::panel::ProxyGroupKind;
    match group.kind {
        ProxyGroupKind::Selector => "手动选择",
        ProxyGroupKind::UrlTest => "自动测速",
        ProxyGroupKind::Fallback => "故障转移",
        ProxyGroupKind::LoadBalance => "负载均衡",
        ProxyGroupKind::Relay => "链式代理",
        ProxyGroupKind::Other => "代理组",
    }
}

fn render_issues(snapshot: &StatusSnapshot) -> Html {
    let issues: Vec<&str> = [
        snapshot.router.issue.as_ref(),
        snapshot.proxy.issue.as_ref(),
        snapshot.system.issue.as_ref(),
    ]
    .into_iter()
    .flatten()
    .map(|issue| issue.message.as_str())
    .collect();
    if issues.is_empty() {
        Html::default()
    } else {
        html! {
            <section class={WARNINGS} aria-labelledby="issues-title">
                <div>
                    <strong id="issues-title">{"状态提示"}</strong>
                    <ul class={WARNINGS_LIST}>{for issues.into_iter().map(|issue| html! { <li>{issue}</li> })}</ul>
                </div>
            </section>
        }
    }
}

fn status_card(
    title: &'static str,
    icon: &'static str,
    status: (&'static str, Tone),
    rows: Vec<(&'static str, String)>,
    accent: &'static str,
) -> Html {
    let (card_accent, icon_accent) = match accent {
        "wan" => (ACCENT_WAN, ICON_WAN),
        "proxy" => (ACCENT_PROXY, ICON_PROXY),
        _ => (ACCENT_SYSTEM, ICON_SYSTEM),
    };
    html! {
        <article class={classes!(STATUS_CARD, card_accent)}>
            <div class={STATUS_CARD_BODY}>
                <div class={STATUS_CARD_HEAD}>
                    <span class={classes!(STATUS_ICON, icon_accent)} aria-hidden="true">{icon}</span>
                    <h2 class={STATUS_TITLE}>{title}</h2>
                    <span class={classes!(STATUS_BADGE, status.1.class())}><span class={STATUS_DOT_SMALL} aria-hidden="true"></span>{status.0}</span>
                </div>
                <dl class={METRIC_LIST}>{for rows.into_iter().map(|(label, value)| html! { <div class={METRIC}><dt class={METRIC_LABEL}>{label}</dt><dd class={METRIC_VALUE} title={value.clone()}>{value}</dd></div> })}</dl>
            </div>
        </article>
    }
}

fn component_card_status<T>(component: &Component<T>) -> (&'static str, Tone) {
    match component.state {
        ComponentState::Available => ("正常", Tone::Good),
        ComponentState::Degraded => ("降级", Tone::Warn),
        ComponentState::Unavailable => ("不可用", Tone::Bad),
    }
}

fn link_state_label(state: LinkState) -> &'static str {
    match state {
        LinkState::Up => "已连接",
        LinkState::Down => "已断开",
        LinkState::Connecting => "连接中",
        LinkState::Unknown => "未知",
    }
}

fn proxy_state_label(state: ProxyState) -> &'static str {
    match state {
        ProxyState::Running => "运行中",
        ProxyState::Stopped => "已停止",
        ProxyState::Disabled => "未启用",
        ProxyState::Error => "异常",
        ProxyState::Unknown => "未知",
    }
}

fn proxy_mode_label(mode: ProxyMode) -> &'static str {
    match mode {
        ProxyMode::Explicit => "显式代理",
        ProxyMode::Tun => "TUN",
        ProxyMode::Disabled => "未启用",
        ProxyMode::Unknown => "未知",
    }
}

fn wan_traffic(
    component: &Component<hyz_router::domain::status::SystemStats>,
) -> Option<(u64, u64)> {
    component.data.as_ref().and_then(|stats| {
        stats
            .interfaces
            .iter()
            .find(|interface| interface.name == "wlan0")
            .map(|interface| (interface.rx_bytes, interface.tx_bytes))
    })
}

fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    if days > 0 {
        format!("{days} 天 {hours} 小时")
    } else if hours > 0 {
        format!("{hours} 小时 {minutes} 分")
    } else {
        format!("{minutes} 分钟")
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn format_bool(value: bool) -> String {
    if value { "是" } else { "否" }.to_owned()
}

fn missing() -> String {
    MISSING.to_owned()
}

fn main() {
    yew::Renderer::<App>::new().render();
}
