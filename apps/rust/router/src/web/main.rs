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
use web_sys::{HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;

const STATUS_ENDPOINT: &str = "/api/v1/status";
const PANEL_ENDPOINT: &str = "/api/v1/panel";
const DISPLAY_ENDPOINT: &str = "/api/v1/control/display";
const PROXY_MODE_ENDPOINT: &str = "/api/v1/control/proxy/mode";
const PROXY_SELECTION_ENDPOINT: &str = "/api/v1/control/proxy/selection";
const PROXY_DELAYS_ENDPOINT: &str = "/api/v1/control/proxy/delays";
const POLL_DELAY_MS: u32 = 2_000;
const MISSING: &str = "—";

#[derive(Clone, PartialEq, Default)]
struct AppState {
    snapshot: Option<StatusSnapshot>,
    panel: Option<PanelBootstrap>,
    last_update: Option<String>,
    poll_error: Option<String>,
    control_notice: Option<String>,
    control_busy: bool,
    loading: bool,
}

enum Action {
    Started,
    Success(Box<StatusSnapshot>, Box<PanelBootstrap>, String),
    Failure(String),
    ControlStarted,
    ControlFinished(Result<String, String>),
    ProxyDelaysFinished(Result<Vec<ProxyGroup>, String>),
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
                {render_control_panel(&state, brightness.clone())}
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
