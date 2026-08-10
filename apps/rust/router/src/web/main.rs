#![cfg(feature = "web")]

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
use web_sys::{HtmlInputElement, HtmlSelectElement, RequestCredentials};
use yew::prelude::*;

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
const AP_CONFIRM_TIMEOUT_MS: u64 = 120_000;
const POLL_DELAY_MS: u32 = 2_000;
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
    staged_at_unix_ms: u64,
    config: NetworkConfigDto,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkPendingDto {
    pending: Option<PendingConfigDto>,
    applied: bool,
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
    control_notice: Option<String>,
    control_busy: bool,
    loading: bool,
    session_checked: bool,
    session: Option<AuthSessionDto>,
    settings_notice: Option<String>,
    settings_busy: bool,
    network: Option<NetworkConfigDto>,
    pending_network: Option<NetworkPendingDto>,
    scan_entries: Vec<WifiScanDto>,
    subscription: Option<SubscriptionDto>,
    ap_applied_local: bool,
}

enum Action {
    Started,
    Success(Box<StatusSnapshot>, Box<PanelBootstrap>, String),
    Failure(String),
    ControlStarted,
    ControlFinished(Result<String, String>),
    ProxyDelaysFinished(Result<Vec<ProxyGroup>, String>),
    SessionFinished(Result<AuthSessionDto, String>),
    AuthFinished(Result<(AuthSessionDto, String), String>),
    SettingsStarted,
    SettingsFinished(Result<(NetworkConfigDto, NetworkPendingDto, SubscriptionDto), String>),
    SettingsMutationFinished(Result<String, String>),
    ScanFinished(Result<Vec<WifiScanDto>, String>),
    SettingsNotice(String),
    ApApplyDispatched,
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
            Action::ControlStarted => Self {
                control_busy: true,
                control_notice: None,
                ..(*self).clone()
            }
            .into(),
            Action::ControlFinished(result) => Self {
                control_busy: false,
                control_notice: Some(match result {
                    Ok(message) => message,
                    Err(error) => format!("操作失败：{error}"),
                }),
                ..(*self).clone()
            }
            .into(),
            Action::ProxyDelaysFinished(result) => {
                let mut next = (*self).clone();
                next.control_busy = false;
                match result {
                    Ok(groups) => {
                        if let Some(bootstrap) = &mut next.panel {
                            bootstrap.panel.proxy_groups = Component::available(groups);
                        }
                        next.control_notice = Some("节点延迟已更新".to_owned());
                    }
                    Err(error) => {
                        next.control_notice = Some(format!("测速失败：{error}"));
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
                        if pending.pending.is_none() {
                            next.ap_applied_local = false;
                        }
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
            Action::ApApplyDispatched => Self {
                ap_applied_local: true,
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
            Self::Good => "good",
            Self::Warn => "warn",
            Self::Bad => "bad",
            Self::Neutral => "neutral",
        }
    }
}

#[function_component(App)]
fn app() -> Html {
    let state = use_reducer(AppState::default);
    let brightness = use_state(|| 128u16);
    let delay_refresh_started = use_state(|| false);

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
        let refresh = state
            .session
            .as_ref()
            .filter(|session| session.authenticated && !session.must_change)
            .and_then(|_| state.panel.as_ref())
            .and_then(|bootstrap| {
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

    html! {
        <main class="shell">
            <header class="hero">
                <div>
                    <p class="eyebrow">{"HYZ ROUTER · 本地控制面"}</p>
                    <h1>{"网络状态"}</h1>
                    <p class="subtitle">{"集中查看设备、链路与透明代理运行情况"}</p>
                </div>
                <div class={classes!("overall", overall_tone.class())} role="status" aria-live="polite">
                    <span class="status-dot" aria-hidden="true"></span>
                    <div><strong>{overall_text}</strong><small>{format!("最后更新：{updated}")}</small></div>
                </div>
            </header>
            {render_notice(&state)}
            if let Some(snapshot) = &state.snapshot {
                {render_dashboard(snapshot)}
                {render_issues(snapshot)}
            } else if state.loading {
                <section class="loading-grid" aria-label="正在加载">
                    {for (0..3).map(|_| html! { <div class="skeleton"></div> })}
                </section>
            } else {
                <section class="empty-state">
                    <span class="empty-icon">{"!"}</span>
                    <h2>{"暂时无法读取状态"}</h2>
                    <p>{"面板会自动重试，无需刷新页面。"}</p>
                </section>
            }
            <Settings state={state.clone()} />
            if state.session.as_ref().is_some_and(|session| session.authenticated && !session.must_change) {
                {render_control_panel(&state, brightness.clone())}
            }
            <footer>{"数据约每 2 秒自动刷新 · 写操作仅接受同源令牌保护的类型化请求"}</footer>
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
    endpoint: &'static str,
    csrf_token: String,
    body: T,
    success: String,
) where
    T: serde::Serialize + 'static,
{
    state.dispatch(Action::ControlStarted);
    spawn_local(async move {
        let request = match Request::post(endpoint)
            .credentials(RequestCredentials::SameOrigin)
            .header("Accept", "application/json")
            .header("X-HYZ-CSRF", &csrf_token)
            .json(&body)
        {
            Ok(request) => request,
            Err(error) => {
                state.dispatch(Action::ControlFinished(Err(format!(
                    "无法编码请求：{error}"
                ))));
                return;
            }
        };
        let result = match request.send().await {
            Ok(response) if response.ok() => Ok(success),
            Ok(response) => Err(format!("控制接口返回 HTTP {}", response.status())),
            Err(error) => Err(format!("无法连接控制接口：{error}")),
        };
        state.dispatch(Action::ControlFinished(result));
    });
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum DelayRefreshControlResponse {
    ProxyDelays { groups: Vec<ProxyGroup> },
}

fn dispatch_delay_refresh(state: UseReducerHandle<AppState>, csrf_token: String) {
    state.dispatch(Action::ControlStarted);
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
    let network = fetch_json::<NetworkConfigDto>(NETWORK_CONFIG_ENDPOINT, "网络配置").await?;
    let pending =
        fetch_json::<NetworkPendingDto>(NETWORK_PENDING_ENDPOINT, "待确认网络配置").await?;
    let subscription = fetch_json::<SubscriptionDto>(SUBSCRIPTION_ENDPOINT, "订阅状态").await?;
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

fn dispatch_scan(state: UseReducerHandle<AppState>, csrf: String) {
    state.dispatch(Action::SettingsStarted);
    spawn_local(async move {
        let result = post_json_response::<_, Vec<WifiScanDto>>(
            STA_SCAN_ENDPOINT,
            &csrf,
            &EmptyRequest {},
            "STA 扫描",
        )
        .await;
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
                "订阅来源保存",
            )
            .await?;
            post_json(
                SUBSCRIPTION_REFRESH_ENDPOINT,
                &csrf,
                &EmptyRequest {},
                "订阅更新",
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

#[derive(Properties, PartialEq)]
struct SettingsProps {
    state: UseReducerHandle<AppState>,
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
    let ap_ssid = use_node_ref();
    let ap_password = use_node_ref();
    let ap_country = use_node_ref();
    let subscription_url = use_node_ref();
    let now_ms = use_state(|| js_sys::Date::now() as u64);

    {
        let now_ms = now_ms.clone();
        use_effect_with((), move |_| {
            let cancelled = Rc::new(Cell::new(false));
            let task_cancelled = cancelled.clone();
            spawn_local(async move {
                while !task_cancelled.get() {
                    TimeoutFuture::new(1_000).await;
                    now_ms.set(js_sys::Date::now() as u64);
                }
            });
            move || cancelled.set(true)
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
        Callback::from(move |_| {
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
        let state = state.clone();
        let csrf = csrf.clone();
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
            dispatch_settings_mutation(
                state.clone(),
                STA_APPLY_ENDPOINT,
                csrf.clone(),
                request,
                "STA 应用",
                "STA 配置已应用",
            );
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
            if endpoint == AP_APPLY_ENDPOINT {
                state.dispatch(Action::ApApplyDispatched);
            }
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
        <section class="settings" aria-labelledby="settings-title">
            <div class="section-head settings-head">
                <div><p class="eyebrow">{"ADMIN"}</p><h2 id="settings-title">{"管理设置"}</h2></div>
                if authenticated {
                    <div class="session-actions"><span>{"admin"}</span><button type="button" onclick={logout} disabled={busy || csrf.is_empty()}>{"退出登录"}</button></div>
                } else {
                    <span>{"状态面板无需登录，设置需要管理员身份"}</span>
                }
            </div>
            if let Some(notice) = &state.settings_notice {
                <div class="control-feedback" role="status" aria-live="polite">{notice}</div>
            }
            if !state.session_checked {
                <div class="control-empty">{"正在检查登录状态…"}</div>
            } else if !authenticated {
                <form class="auth-form" onsubmit={login} autocomplete="on">
                    <label><span>{"用户名"}</span><input value="admin" readonly=true autocomplete="username" /></label>
                    <label><span>{"密码"}</span><input ref={login_password} type="password" required=true autocomplete="current-password" /></label>
                    <button class="primary" type="submit" disabled={busy || csrf.is_empty()}>{if busy { "登录中…" } else { "登录" }}</button>
                </form>
            } else if must_change {
                <div class="forced-password">
                    <div class="risk-banner bad" role="alert"><strong>{"必须先修改默认密码"}</strong><span>{"新密码至少 12 字节，不能继续使用默认密码。完成前其他设置保持锁定。"}</span></div>
                    <form class="form-grid" onsubmit={change_password} autocomplete="on">
                        <label><span>{"当前密码"}</span><input ref={current_password} type="password" required=true autocomplete="current-password" /></label>
                        <label><span>{"新密码"}</span><input ref={new_password} type="password" required=true maxlength="1024" autocomplete="new-password" /></label>
                        <label><span>{"确认新密码"}</span><input ref={confirm_password} type="password" required=true maxlength="1024" autocomplete="new-password" /></label>
                        <div class="form-actions"><button class="primary" type="submit" disabled={busy || csrf.is_empty()}>{"修改密码"}</button></div>
                    </form>
                </div>
            } else {
                <div class="settings-grid">
                    <article class="settings-card network-current">
                        <div class="control-title"><h3>{"当前网络"}</h3><span>{state.network.as_ref().map_or("读取中", |_| "已提交配置")}</span></div>
                        if let Some(network) = &state.network {
                            <dl><div class="metric"><dt>{"AP"}</dt><dd>{&network.ap_ssid}</dd></div><div class="metric"><dt>{"STA"}</dt><dd>{&network.sta_ssid}</dd></div><div class="metric"><dt>{"国家 / 地区"}</dt><dd>{network.country.as_str()}</dd></div></dl>
                        }
                    </article>
                    <article class="settings-card">
                        <div class="control-title"><h3>{"上游 Wi-Fi (STA)"}</h3><button type="button" onclick={scan} disabled={busy}>{if busy { "处理中…" } else { "扫描" }}</button></div>
                        if !state.scan_entries.is_empty() {
                            <div class="scan-list" aria-label="扫描到的 Wi-Fi">
                                {for state.scan_entries.iter().map(|entry| {
                                    let input = sta_ssid.clone();
                                    let ssid = entry.ssid.clone();
                                    let choose = Callback::from(move |_| { if let Some(input) = input.cast::<HtmlInputElement>() { input.set_value(&ssid); } });
                                    html! { <button type="button" class="scan-entry" onclick={choose} disabled={busy}><strong>{&entry.ssid}</strong><span>{format!("{} MHz · {} dBm · {}", entry.frequency_mhz, entry.signal_dbm, if entry.secured { "加密" } else { "开放" })}</span></button> }
                                })}
                            </div>
                        }
                        <form class="form-grid compact" onsubmit={apply_sta} autocomplete="off">
                            <label><span>{"SSID"}</span><input ref={sta_ssid} required=true maxlength="32" autocomplete="off" /></label>
                            <label><span>{"密码"}</span><input ref={sta_password} type="password" required=true minlength="8" maxlength="63" autocomplete="new-password" /></label>
                            <div class="risk-note warn">{"若 STA 与当前 AP 信道不同，设备可能重启 AP 跟随信道，管理连接会短暂断开。"}</div>
                            <div class="form-actions"><button class="primary" type="submit" disabled={busy}>{"应用 STA"}</button></div>
                        </form>
                    </article>
                    <article class="settings-card ap-card">
                        <div class="control-title"><h3>{"下游 Wi-Fi (AP)"}</h3><span>{"两阶段变更"}</span></div>
                        {render_ap_settings(state, &ap_ssid, &ap_password, &ap_country, prepare_ap, ap_action, *now_ms, busy)}
                    </article>
                    <article class="settings-card">
                        <div class="control-title"><h3>{"代理订阅"}</h3><span>{"来源只写"}</span></div>
                        if let Some(subscription) = &state.subscription {
                            <div class="subscription-summary"><span>{if subscription.configured { "已配置" } else { "未配置" }}</span><strong class={subscription_tone(subscription.state)}>{subscription_state_label(subscription.state)}</strong></div>
                        }
                        <form class="form-grid compact" onsubmit={save_subscription} autocomplete="off">
                            <label><span>{"订阅 URL"}</span><input ref={subscription_url} type="url" required=true placeholder="https://…" autocomplete="off" autocapitalize="none" spellcheck="false" /></label>
                            <small>{"已保存的 URL 永不回显；输入只用于本次提交。"}</small>
                            <div class="form-actions"><button class="primary" type="submit" disabled={busy}>{"保存并立即更新"}</button><button type="button" onclick={refresh_subscription} disabled={busy || !state.subscription.as_ref().is_some_and(|value| value.configured)}>{"手动刷新"}</button></div>
                        </form>
                    </article>
                </div>
            }
        </section>
    }
}

fn render_ap_settings(
    state: &AppState,
    ap_ssid: &NodeRef,
    ap_password: &NodeRef,
    ap_country: &NodeRef,
    prepare: Callback<SubmitEvent>,
    ap_action: impl Fn(&'static str, &'static str, &'static str) -> Callback<MouseEvent>,
    now_ms: u64,
    busy: bool,
) -> Html {
    let pending = state
        .pending_network
        .as_ref()
        .and_then(|value| value.pending.as_ref());
    let applied = state.ap_applied_local
        || state
            .pending_network
            .as_ref()
            .is_some_and(|value| value.applied);
    if let Some(pending) = pending {
        let deadline = pending
            .staged_at_unix_ms
            .saturating_add(AP_CONFIRM_TIMEOUT_MS);
        let seconds = deadline.saturating_sub(now_ms).div_ceil(1_000);
        html! {
            <div class="pending-ap">
                <dl><div class="metric"><dt>{"候选 AP"}</dt><dd>{&pending.config.ap_ssid}</dd></div><div class="metric"><dt>{"国家 / 地区"}</dt><dd>{pending.config.country.as_str()}</dd></div></dl>
                if applied {
                    <div class="risk-banner warn" role="alert"><strong>{format!("等待确认 · {seconds} 秒")}</strong><span>{"请连接新的 AP 后确认。倒计时结束会自动回滚；如无法使用新配置，请取消。"}</span></div>
                    <div class="form-actions"><button class="primary" type="button" onclick={ap_action(AP_CONFIRM_ENDPOINT, "AP 确认", "AP 配置已确认")} disabled={busy}>{"确认保留"}</button><button type="button" onclick={ap_action(AP_CANCEL_ENDPOINT, "AP 取消", "AP 配置已取消并回滚")} disabled={busy}>{"取消并回滚"}</button></div>
                } else {
                    <div class="risk-banner bad" role="alert"><strong>{"应用会立即断开当前 AP 连接"}</strong><span>{"请先记住新 SSID 和密码。应用后连接新 AP，再回到本页确认；未确认会自动回滚。"}</span></div>
                    <div class="form-actions"><button class="danger" type="button" onclick={ap_action(AP_APPLY_ENDPOINT, "AP 应用", "AP 正在切换，请连接新 AP 后确认")} disabled={busy}>{"我已了解，立即应用"}</button><button type="button" onclick={ap_action(AP_CANCEL_ENDPOINT, "AP 取消", "AP 候选配置已取消")} disabled={busy}>{"取消"}</button></div>
                }
            </div>
        }
    } else {
        html! {
            <form class="form-grid compact" onsubmit={prepare} autocomplete="off">
                <label><span>{"SSID"}</span><input ref={ap_ssid.clone()} required=true maxlength="32" autocomplete="off" /></label>
                <label><span>{"密码"}</span><input ref={ap_password.clone()} type="password" required=true minlength="8" maxlength="63" autocomplete="new-password" /></label>
                <label><span>{"国家 / 地区"}</span><select ref={ap_country.clone()}><option value="CN">{"中国 (CN)"}</option><option value="US">{"美国 (US)"}</option><option value="JP">{"日本 (JP)"}</option><option value="SG">{"新加坡 (SG)"}</option><option value="TW">{"中国台湾 (TW)"}</option><option value="AU">{"澳大利亚 (AU)"}</option><option value="BR">{"巴西 (BR)"}</option><option value="CA">{"加拿大 (CA)"}</option><option value="DE">{"德国 (DE)"}</option><option value="FR">{"法国 (FR)"}</option><option value="GB">{"英国 (GB)"}</option><option value="IN">{"印度 (IN)"}</option><option value="KR">{"韩国 (KR)"}</option><option value="NZ">{"新西兰 (NZ)"}</option></select></label>
                <div class="form-actions"><button class="primary" type="submit" disabled={busy}>{"准备 AP 变更"}</button></div>
            </form>
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
        SubscriptionStateDto::Active => "good",
        SubscriptionStateDto::Fetching => "warn",
        SubscriptionStateDto::Failed => "bad",
        SubscriptionStateDto::Idle => "neutral",
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
            <div class="notice warn" role="alert"><strong>{"刷新失败，正在展示最近一次数据"}</strong><span>{error}</span></div>
        },
        (Some(error), None) => html! {
            <div class="notice bad" role="alert"><strong>{"状态服务不可用"}</strong><span>{error}</span></div>
        },
        _ => Html::default(),
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
        <section class="dashboard" aria-label="路由器状态卡片">
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

fn render_control_panel(
    state: &UseReducerHandle<AppState>,
    brightness: UseStateHandle<u16>,
) -> Html {
    let Some(bootstrap) = state.panel.as_ref() else {
        return Html::default();
    };
    let csrf = bootstrap.csrf_token.clone();
    let busy = state.control_busy;
    let display = bootstrap.panel.display.data.as_ref();
    let proxy_available = state
        .snapshot
        .as_ref()
        .is_some_and(|snapshot| snapshot.proxy.data.is_some());
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
    let proxy_mode_button = |mode: ProxyMode, message: &'static str| {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |_| {
            dispatch_control(
                state.clone(),
                PROXY_MODE_ENDPOINT,
                csrf.clone(),
                ProxyModeRequest { mode },
                message.to_owned(),
            )
        })
    };

    html! {
        <section class="controls" aria-label="本地控制">
            <div class="section-head"><div><p class="eyebrow">{"CONTROL"}</p><h2>{"设备与代理控制"}</h2></div><span>{"仅限管理 LAN · 同源令牌保护"}</span></div>
            <div class={classes!("control-feedback", state.control_notice.is_none().then_some("empty"))} role="status" aria-live="polite">
                {state.control_notice.as_deref().unwrap_or("等待操作")}
            </div>
            <div class="control-grid">
                if display.is_some() {
                    <article class="control-card">
                        <div class="control-title"><h3>{"LCD 背光"}</h3><span>{display_label}</span></div>
                        <label class="range-row" for="brightness"><span>{"点亮亮度"}</span><strong>{*brightness}</strong></label>
                        <input id="brightness" type="range" min="1" max={max_brightness.to_string()} value={(*brightness).min(max_brightness).to_string()} oninput={on_brightness} disabled={busy} />
                        <div class="button-row"><button class="primary" onclick={display_on} disabled={busy}>{"点亮"}</button><button onclick={display_off} disabled={busy}>{"黑屏"}</button></div>
                        <small>{"黑屏会将 PWM 亮度设为 0；面板 5V 是共享电源，无法单独物理断开。"}</small>
                    </article>
                }
                if proxy_available {
                    <article class="control-card">
                        <div class="control-title"><h3>{"Mihomo 模式"}</h3><span>{"切换时按 fail-open 顺序收敛"}</span></div>
                        <div class="mode-buttons">
                            <button onclick={proxy_mode_button(ProxyMode::Tun, "已切换到 TUN 模式")} disabled={busy}>{"TUN"}</button>
                            <button onclick={proxy_mode_button(ProxyMode::Explicit, "已切换到显式代理")} disabled={busy}>{"显式代理"}</button>
                            <button onclick={proxy_mode_button(ProxyMode::Disabled, "Mihomo 已停用")} disabled={busy}>{"停用"}</button>
                        </div>
                        <small>{"停用代理不会删除订阅配置；普通 NAT 在路由启用时保持可用。"}</small>
                    </article>
                }
            </div>
            <div class="proxy-groups">
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
            <div class="proxy-toolbar">
            <span>{"进入页面时自动测速一次"}</span>
            <button onclick={refresh_delays} disabled={busy}>{if busy { "测速中…" } else { "重新测速全部节点" }}</button>
        </div>
        {for groups.iter().map(|group| {
            let group_name = group.name.clone();
            let selection_state = state.clone();
            let selection_csrf = csrf.to_owned();
            let on_selection = Callback::from(move |event: Event| {
                let select: HtmlSelectElement = event.target_unchecked_into();
                dispatch_control(
                    selection_state.clone(),
                    PROXY_SELECTION_ENDPOINT,
                    selection_csrf.clone(),
                    ProxySelectionRequest { group: group_name.clone(), proxy: select.value() },
                    "代理节点已切换".to_owned(),
                );
            });
            let selected = group.selected.clone();
            html! {
                <article class="proxy-group" key={group.name.clone()}>
                    <div><h3>{&group.name}</h3><span>{group_kind_label(group)}</span></div>
                    <select value={selected.clone().unwrap_or_default()} onchange={on_selection} disabled={busy || !group.selectable} aria-label={format!("{} 节点", group.name)}>
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
        html! { <section class="warnings"><strong>{"状态提示"}</strong><ul>{for issues.into_iter().map(|issue| html! { <li>{issue}</li> })}</ul></section> }
    }
}

fn status_card(
    title: &'static str,
    icon: &'static str,
    status: (&'static str, Tone),
    rows: Vec<(&'static str, String)>,
    accent: &'static str,
) -> Html {
    html! {
        <article class={classes!("card", format!("accent-{accent}"))}>
            <div class="card-head"><span class="card-icon">{icon}</span><h2>{title}</h2><span class={classes!("pill", status.1.class())}><span class="status-dot"></span>{status.0}</span></div>
            <dl>{for rows.into_iter().map(|(label, value)| html! { <div class="metric"><dt>{label}</dt><dd title={value.clone()}>{value}</dd></div> })}</dl>
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
