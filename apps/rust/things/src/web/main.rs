#![cfg(feature = "web")]

mod api;
mod app;
mod ui;

#[path = "hooks/countdown.rs"]
mod countdown;

#[path = "pages/camera.rs"]
mod camera_page;

#[path = "pages/proxy.rs"]
mod proxy_page;

#[path = "pages/tailscale.rs"]
mod tailscale_page;

#[path = "pages/apps.rs"]
mod apps_page;

#[path = "pages/network.rs"]
mod network_page;

use api::*;
use app::*;
use apps_page::*;
use camera_page::*;
use countdown::*;
use network_page::*;
use proxy_page::*;
use tailscale_page::*;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gloo_net::http::Request;
use gloo_timers::future::TimeoutFuture;
use hyz_things::domain::{
    camera::{
        CameraAccessKind, CameraErrorCategory, CameraPipelineState, CameraRotation, CameraStatus,
        CameraStreamPreset,
    },
    panel::{
        DisplayRequest, PanelBootstrap, ProxyDelayRefreshRequest, ProxyGroup, ProxySelectionRequest,
    },
    status::{
        Component, ComponentState, LanTunEffective, LocalSystemProxyEffective, ProxyResourceState,
        ProxyStatus, SnapshotState, StatusSnapshot, TailscaleConnectionType,
        TailscaleErrorCategory, TailscaleStatus, UplinkId, UplinkStatus,
    },
    tailscale::{
        TailscaleBackendState, TailscaleMode, TailscalePeer, TailscalePeerConnection,
        TailscalePeerSnapshot,
    },
};
use js_sys::{Date, Function, Object, Promise, Reflect};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    AudioContext, CanvasRenderingContext2d, Document, Element, Event, HtmlCanvasElement,
    HtmlElement, HtmlInputElement, HtmlMediaElement, HtmlSelectElement, HtmlVideoElement,
    KeyboardEvent, MediaStream, MediaStreamAudioDestinationNode, MediaStreamConstraints,
    MediaStreamTrack, MediaTrackConstraints, PointerEvent, RequestCredentials,
    RtcIceGatheringState, RtcPeerConnection, RtcPeerConnectionState, RtcRtpSender,
    RtcRtpTransceiverDirection, RtcRtpTransceiverInit, RtcSdpType, RtcSessionDescriptionInit,
    RtcTrackEvent, Storage, VideoFrame, VideoFrameInit, Window,
};
use yew::prelude::*;

use ui::*;

const CAMERA_ICE_GATHER_TIMEOUT_MS: u32 = 10_000;
const CAMERA_ICE_POLL_MS: u32 = 50;
// 页面隐藏后不立即关闭直播会话：宽限期内回来就继续，超时才真正关闭
// （多 viewer 场景下避免切走标签页即断流）。
const CAMERA_HIDDEN_CLOSE_GRACE_MS: u32 = 60_000;
const POLL_DELAY_MS: u32 = 2_000;
const NETWORK_APPLY_PAINT_DELAY_MS: u32 = 150;
// 画面设置（预设/旋转）提交遇到 camera busy（残留会话或 close 尚未完成）时
// 短暂重试，避免连续快速操作被一个过渡态永久卡在"未播放"。
const CAMERA_UPDATE_RETRY_ATTEMPTS: u8 = 3;
const CAMERA_UPDATE_RETRY_DELAY_MS: u32 = 1_000;
const MISSING: &str = "—";
fn is_escape_key(event: &KeyboardEvent) -> bool {
    matches!(event.key().as_str(), "Escape" | "Esc") || event.code() == "Escape"
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

fn render_overview(state: &UseReducerHandle<AppState>) -> Html {
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

fn deployment_time_label(unix_ms: Option<u64>) -> String {
    const MAX_DATE_MILLIS: u64 = 8_640_000_000_000_000;
    let Some(unix_ms) = unix_ms.filter(|value| *value <= MAX_DATE_MILLIS) else {
        return "未记录".to_owned();
    };
    js_sys::Date::new(&JsValue::from_f64(unix_ms as f64))
        .to_iso_string()
        .as_string()
        .unwrap_or_else(|| "未记录".to_owned())
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
    let tailscale = snapshot.tailscale.data.as_ref();
    let active_uplink = router.and_then(|value| value.active_uplink);
    let ethernet = router.and_then(|value| value.ethernet.as_ref());
    let wifi = router.and_then(|value| value.wifi.as_ref());
    let active_status = active_uplink.and_then(|uplink| match uplink {
        UplinkId::Ethernet => ethernet,
        UplinkId::Wifi => wifi,
    });
    let internet_tone = active_status
        .map(|status| uplink_tone(Some(status)))
        .unwrap_or_else(|| {
            if [ethernet, wifi]
                .into_iter()
                .flatten()
                .any(|status| status.session_up == Some(true))
            {
                Tone::Warn
            } else if [ethernet, wifi]
                .into_iter()
                .flatten()
                .all(|status| status.link_up == Some(false))
            {
                Tone::Bad
            } else {
                Tone::Neutral
            }
        });
    let internet_detail = active_uplink.map_or_else(
        || "没有确认的活动上行".to_owned(),
        |uplink| {
            let metric = active_status
                .and_then(|status| status.default_route_metric)
                .map_or_else(|| "未知".to_owned(), |metric| metric.to_string());
            format!("活动 {} · 默认路由 metric {metric}", uplink_label(uplink))
        },
    );
    let router_detail = router.map_or_else(
        || "等待转发状态".to_owned(),
        |router| {
            format!(
                "活动 {} · 转发 {} · NAT {}",
                router.active_uplink.map(uplink_label).unwrap_or(MISSING),
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
                "Core {} · LAN {}",
                mihomo_core_status_label(proxy),
                lan_tun_status_label(proxy)
            )
        },
    );
    let tailscale_detail = tailscale.map_or_else(
        || "Tailscale 状态不可用".to_owned(),
        tailscale_status_detail,
    );

    html! {
        <section class={TOPOLOGY} aria-labelledby="topology-title">
            <div class={SECTION_HEAD_CENTERED}>
                <div><p class={EYEBROW}>{"PATH"}</p><h2 id="topology-title" class={SECTION_TITLE}>{"网络拓扑"}</h2></div>
                <span class={SECTION_META}>{"Ethernet 优先，Wi-Fi 保持备用；下游同时经过 Router/NAT 与代理运行时"}</span>
            </div>
            <div class={TOPOLOGY_FLOW}>
                {topology_node("WAN", "互联网", internet_detail, internet_tone)}
                {topology_link(internet_tone)}
                <div class={TOPOLOGY_UPLINKS}>
                    <div class={TOPOLOGY_UPLINKS_HEAD}><span>{"上游双链路"}</span><span>{"Ethernet 优先 · Wi-Fi fallback"}</span></div>
                    {topology_uplink_node("ETH", "Ethernet WAN", uplink_status_detail(ethernet), uplink_tone(ethernet), active_uplink == Some(UplinkId::Ethernet))}
                    {topology_uplink_node("STA", "Wi-Fi WAN", wifi_uplink_status_detail(router), uplink_tone(wifi), active_uplink == Some(UplinkId::Wifi))}
                </div>
                {topology_link(internet_tone)}
                {topology_node("RTR", "Router / NAT", router_detail, component_tone(&snapshot.router))}
                {topology_link(component_tone(&snapshot.router))}
                {topology_node("LAN", "AP / LAN", ap_detail, component_tone(&snapshot.router))}
            </div>
            <div class={TOPOLOGY_BRANCH_STACK}>
                <div class={TOPOLOGY_PROXY_ROW}>
                    <span class={TOPOLOGY_BRANCH} aria-hidden="true">{"↳"}</span>
                    {topology_node("PX", "Mihomo / 代理数据面", proxy_detail, component_tone(&snapshot.proxy))}
                </div>
                <div class={TOPOLOGY_PROXY_ROW}>
                    <span class={TOPOLOGY_BRANCH} aria-hidden="true">{"↳"}</span>
                    {topology_node("TS", "Tailscale", tailscale_detail, component_tone(&snapshot.tailscale))}
                </div>
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

fn topology_uplink_node(
    icon: &'static str,
    title: &'static str,
    detail: String,
    tone: Tone,
    active: bool,
) -> Html {
    html! {
        <article class={TOPOLOGY_NODE}>
            <span class={classes!(TOPOLOGY_ICON, tone.class())} aria-hidden="true">{icon}</span>
            <span class={TOPOLOGY_COPY}>
                <span class="flex min-w-0 items-center gap-2">
                    <strong class={classes!(TOPOLOGY_TITLE, "truncate")}>{title}</strong>
                    if active {
                        <span class={TOPOLOGY_UPLINK_BADGE}>{"主用"}</span>
                    }
                </span>
                <small class={TOPOLOGY_DETAIL} title={detail.clone()}>{detail}</small>
            </span>
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

fn uplink_tone(status: Option<&UplinkStatus>) -> Tone {
    let Some(status) = status else {
        return Tone::Neutral;
    };
    if status.link_up == Some(false) {
        Tone::Bad
    } else if status.session_up == Some(false)
        || status.address_present == Some(false)
        || status.default_route_present == Some(false)
    {
        Tone::Warn
    } else if status.link_up == Some(true)
        && status.session_up == Some(true)
        && status.address_present == Some(true)
        && status.default_route_present == Some(true)
        && status.gateway.is_some()
    {
        Tone::Good
    } else {
        Tone::Warn
    }
}

fn uplink_status_detail(status: Option<&UplinkStatus>) -> String {
    let Some(status) = status else {
        return "状态不可用".to_owned();
    };
    let link = match status.link_up {
        Some(true) => "链路正常",
        Some(false) => "链路断开",
        None => "链路未知",
    };
    let dhcp = match status.session_up {
        Some(true) => "DHCP 已建立",
        Some(false) => "DHCP 未建立",
        None => "DHCP 未知",
    };
    let address = status.address.as_deref().unwrap_or(MISSING);
    let gateway = status
        .gateway
        .map(|value| value.to_string())
        .unwrap_or_else(missing);
    let route = status
        .default_route_metric
        .map(|metric| format!("默认路由 metric {metric}"))
        .unwrap_or_else(|| "默认路由未确认".to_owned());
    format!("{link} · {dhcp} · IPv4 {address} · 网关 {gateway} · {route}")
}

fn wifi_uplink_status_detail(router: Option<&hyz_things::domain::status::RouterStatus>) -> String {
    let detail = uplink_status_detail(router.and_then(|router| router.wifi.as_ref()));
    let ssid = router
        .and_then(|router| router.sta_ssid.as_deref())
        .unwrap_or(MISSING);
    let signal = router
        .and_then(|router| router.sta_signal_dbm)
        .map(|value| format!(" · 信号 {value} dBm"))
        .unwrap_or_default();
    format!("{detail} · SSID {ssid}{signal}")
}

fn uplink_label(uplink: UplinkId) -> &'static str {
    match uplink {
        UplinkId::Ethernet => "Ethernet",
        UplinkId::Wifi => "Wi-Fi",
    }
}

fn active_uplink_detail(router: Option<&hyz_things::domain::status::RouterStatus>) -> String {
    router
        .and_then(|router| router.active_uplink)
        .map(|uplink| {
            let metric = router
                .and_then(|router| match uplink {
                    UplinkId::Ethernet => router.ethernet.as_ref(),
                    UplinkId::Wifi => router.wifi.as_ref(),
                })
                .and_then(|status| status.default_route_metric)
                .map_or_else(|| "未知".to_owned(), |metric| metric.to_string());
            format!("{} · metric {metric}", uplink_label(uplink))
        })
        .unwrap_or_else(|| "未确认".to_owned())
}

fn active_resolver_detail(router: Option<&hyz_things::domain::status::RouterStatus>) -> String {
    router
        .and_then(|router| router.active_resolver.as_ref())
        .map(|resolver| {
            let nameservers = resolver
                .nameservers
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} · {nameservers}", uplink_label(resolver.uplink))
        })
        .unwrap_or_else(|| "未确认".to_owned())
}

fn render_kpis(snapshot: &StatusSnapshot) -> Html {
    let router = snapshot.router.data.as_ref();
    let proxy = snapshot.proxy.data.as_ref();
    let ethernet = router.and_then(|value| value.ethernet.as_ref());
    let wifi = router.and_then(|value| value.wifi.as_ref());
    let active = active_uplink_detail(router);
    let ethernet_address = ethernet
        .and_then(|status| status.address.clone())
        .unwrap_or_else(missing);
    let wifi_address = wifi
        .and_then(|status| status.address.clone())
        .unwrap_or_else(missing);
    let proxy_path = proxy.map(lan_tun_status_label).unwrap_or_else(missing);
    let clients = router
        .and_then(|value| value.ap_client_count)
        .map(|value| format!("{value} 台 · {proxy_path}"))
        .unwrap_or_else(|| proxy_path.clone());

    html! {
        <section class={KPI_GRID} aria-label="关键网络指标">
            {kpi("活动上行", active, "Ethernet 优先 · Wi-Fi fallback")}
            {kpi("Ethernet WAN", ethernet_address, "DHCP / 默认路由 metric 100")}
            {kpi("Wi-Fi WAN", wifi_address, "DHCP / 默认路由 metric 600")}
            {kpi("LAN 数据面", clients, "代理 TUN / 普通 NAT · AP 客户端")}
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
    let router_data = router.data.as_ref();
    let router_rows = vec![
        ("活动上行", active_uplink_detail(router_data)),
        (
            "Ethernet WAN",
            uplink_status_detail(router_data.and_then(|v| v.ethernet.as_ref())),
        ),
        ("Wi-Fi WAN", wifi_uplink_status_detail(router_data)),
        ("活动解析器", active_resolver_detail(router_data)),
        (
            "LAN 地址",
            router_data
                .and_then(|v| v.lan_address.clone())
                .unwrap_or_else(missing),
        ),
        (
            "AP 客户端",
            router_data
                .and_then(|v| v.ap_client_count)
                .map(|v| format!("{v} 台"))
                .unwrap_or_else(missing),
        ),
        (
            "IPv4 转发",
            router_data
                .and_then(|v| v.ipv4_forwarding)
                .map(format_bool)
                .unwrap_or_else(missing),
        ),
        (
            "IPv4 NAT",
            router_data
                .and_then(|v| v.masquerade_enabled)
                .map(format_bool)
                .unwrap_or_else(missing),
        ),
    ];
    let proxy_rows = vec![
        (
            "Mihomo core",
            proxy
                .data
                .as_ref()
                .map(mihomo_core_status_label)
                .unwrap_or_else(missing),
        ),
        (
            "运行配置",
            proxy
                .data
                .as_ref()
                .map(|v| proxy_resource_label(v.mihomo.runtime_config).to_owned())
                .unwrap_or_else(missing),
        ),
        (
            "本机 mixed port",
            proxy
                .data
                .as_ref()
                .map(|v| proxy_resource_label(v.mihomo.mixed_port).to_owned())
                .unwrap_or_else(missing),
        ),
        (
            "LAN TUN",
            proxy
                .data
                .as_ref()
                .map(lan_tun_status_label)
                .unwrap_or_else(missing),
        ),
        (
            "本机系统代理",
            proxy
                .data
                .as_ref()
                .map(local_system_proxy_status_label)
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
            "WAN 总接收",
            wan_traffic(system)
                .map(|v| format_bytes(v.0))
                .unwrap_or_else(missing),
        ),
        (
            "WAN 总发送",
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
                {status_card("代理状态", "PX", component_card_status(proxy), proxy_rows, "proxy")}
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

fn render_issues(snapshot: &StatusSnapshot) -> Html {
    let issues: Vec<&str> = [
        snapshot.router.issue.as_ref(),
        snapshot.proxy.issue.as_ref(),
        snapshot.tailscale.issue.as_ref(),
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

fn tailscale_mode_label(mode: TailscaleMode) -> &'static str {
    match mode {
        TailscaleMode::Disabled => "已停用",
        TailscaleMode::RouterOnly => "RouterOnly（仅路由器）",
        TailscaleMode::LanSubnetAccess => "LAN Access",
    }
}

fn tailscale_error_label(category: TailscaleErrorCategory) -> &'static str {
    match category {
        TailscaleErrorCategory::ProbeFailed => "状态探测失败",
        TailscaleErrorCategory::Conflict => "状态冲突",
        TailscaleErrorCategory::NotReady => "尚未就绪",
        TailscaleErrorCategory::OperationFailed => "操作失败",
    }
}

fn proxy_resource_label(state: ProxyResourceState) -> &'static str {
    match state {
        ProxyResourceState::Ready => "就绪",
        ProxyResourceState::Absent => "已停止",
        ProxyResourceState::NotReady => "未就绪",
        ProxyResourceState::Unknown => "未知",
    }
}

fn mihomo_core_status_label(status: &hyz_things::domain::status::ProxyStatus) -> String {
    match (
        status.mihomo.configured_required,
        status.mihomo.process,
        status.mihomo.runtime_config,
        status.mihomo.mixed_port,
    ) {
        (
            Some(true),
            ProxyResourceState::Ready,
            ProxyResourceState::Ready,
            ProxyResourceState::Ready,
        ) => "运行中".to_owned(),
        (
            Some(false),
            ProxyResourceState::Absent,
            ProxyResourceState::Absent,
            ProxyResourceState::Absent,
        ) => "已停止".to_owned(),
        (None, _, _, _) | (_, ProxyResourceState::Unknown, _, _) => "未知".to_owned(),
        _ => "已降级 · 未就绪".to_owned(),
    }
}

fn lan_tun_status_label(status: &hyz_things::domain::status::ProxyStatus) -> String {
    match (status.lan_tun.desired, status.lan_tun.effective) {
        (Some(true), LanTunEffective::Ready) => "已启用".to_owned(),
        (Some(true), LanTunEffective::OrdinaryNat) => "已降级 · 普通 NAT".to_owned(),
        (Some(true), LanTunEffective::NotConfirmed) => "已降级 · 未确认".to_owned(),
        (Some(false), LanTunEffective::OrdinaryNat) => "已关闭 · 普通 NAT".to_owned(),
        (Some(false), LanTunEffective::Ready) => "未知 · 状态冲突".to_owned(),
        (Some(false), LanTunEffective::NotConfirmed) | (None, _) => "未知 · 未确认".to_owned(),
    }
}

fn local_system_proxy_status_label(status: &ProxyStatus) -> String {
    match (
        status.local_system_proxy.desired,
        status.local_system_proxy.effective,
    ) {
        (Some(true), LocalSystemProxyEffective::Ready) => "已启用".to_owned(),
        (Some(true), LocalSystemProxyEffective::NotConfirmed)
        | (Some(true), LocalSystemProxyEffective::Disabled) => "已降级 · 未确认".to_owned(),
        (Some(false), LocalSystemProxyEffective::Disabled) => "已关闭".to_owned(),
        (Some(false), _) => "未知 · 状态冲突".to_owned(),
        (None, _) => "未知 · 未确认".to_owned(),
    }
}

fn tailscale_status_detail(status: &TailscaleStatus) -> String {
    let mode = status
        .effective_mode
        .map(tailscale_mode_label)
        .unwrap_or("未知");
    let connection = match status.connection.kind {
        TailscaleConnectionType::Direct => "Direct".to_owned(),
        TailscaleConnectionType::PeerRelay => "Peer relay".to_owned(),
        TailscaleConnectionType::Derp => status
            .connection
            .derp_region
            .as_deref()
            .map_or_else(|| "DERP".to_owned(), |region| format!("DERP {region}")),
        TailscaleConnectionType::Unknown => "连接未知".to_owned(),
    };
    format!("{mode} · {connection}")
}

fn wan_traffic(
    component: &Component<hyz_things::domain::status::SystemStats>,
) -> Option<(u64, u64)> {
    component.data.as_ref().and_then(|stats| {
        let ((rx_bytes, tx_bytes), found) = stats.interfaces.iter().fold(
            ((0_u64, 0_u64), false),
            |((rx_total, tx_total), found), interface| {
                if matches!(interface.name.as_str(), "eth0" | "wlan0") {
                    (
                        (
                            rx_total.saturating_add(interface.rx_bytes),
                            tx_total.saturating_add(interface.tx_bytes),
                        ),
                        true,
                    )
                } else {
                    ((rx_total, tx_total), found)
                }
            },
        );
        found.then_some((rx_bytes, tx_bytes))
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

#[cfg(test)]
mod tests {
    use super::{countdown_snapshot, wan_traffic, EXAM_COUNTDOWN_TARGETS};
    use hyz_things::domain::status::{Component, InterfaceStats, SystemStats};

    #[test]
    fn upcoming_exam_targets_are_sorted_chronologically() {
        let dates: Vec<_> = EXAM_COUNTDOWN_TARGETS
            .iter()
            .map(|target| target.target_iso)
            .collect();
        assert_eq!(
            dates,
            vec![
                "2026-11-29T00:00:00+08:00",
                "2026-12-06T00:00:00+08:00",
                "2027-03-14T00:00:00+08:00",
                "2027-03-27T00:00:00+08:00",
            ]
        );
    }

    #[test]
    fn countdown_snapshot_tracks_progress_and_clamps_after_exam_day() {
        let before = countdown_snapshot(0, 0, 1_000);
        assert_eq!(before.remaining_seconds, 1);
        assert_eq!(before.progress_percent, 0);
        assert!(!before.finished);

        let during = countdown_snapshot(250, 0, 1_000);
        assert_eq!(during.remaining_seconds, 1);
        assert_eq!(during.progress_percent, 25);
        assert!(!during.finished);

        let after = countdown_snapshot(1_500, 0, 1_000);
        assert_eq!(after.remaining_seconds, 0);
        assert_eq!(after.progress_percent, 100);
        assert!(after.finished);
    }

    #[test]
    fn wan_traffic_sums_ethernet_and_wifi_only() {
        let component = Component::available(SystemStats {
            interfaces: vec![
                InterfaceStats {
                    name: "eth0".to_owned(),
                    rx_bytes: 10,
                    tx_bytes: 20,
                },
                InterfaceStats {
                    name: "wlan0".to_owned(),
                    rx_bytes: 3,
                    tx_bytes: 5,
                },
                InterfaceStats {
                    name: "br-lan".to_owned(),
                    rx_bytes: 100,
                    tx_bytes: 200,
                },
            ],
            ..SystemStats::default()
        });

        assert_eq!(wan_traffic(&component), Some((13, 25)));
    }

    #[test]
    fn wan_traffic_is_missing_without_an_uplink_interface() {
        let component = Component::available(SystemStats {
            interfaces: vec![InterfaceStats {
                name: "br-lan".to_owned(),
                rx_bytes: 100,
                tx_bytes: 200,
            }],
            ..SystemStats::default()
        });

        assert_eq!(wan_traffic(&component), None);
    }
}

fn main() {
    yew::Renderer::<App>::new().render();
}
