#![cfg(feature = "web")]

mod ui;

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
        Component, ComponentState, LanTunEffective, LinkState, ProxyResourceState, SnapshotState,
        StatusSnapshot, TailscaleConnectionType, TailscaleErrorCategory,
        TailscaleExplicitProxyPath, TailscaleProxyFallback, TailscaleStatus,
    },
    tailscale::{
        TailscaleBackendState, TailscaleEnvironment, TailscaleMode, TailscalePeerSnapshot,
    },
};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    Event, HtmlElement, HtmlInputElement, HtmlSelectElement, HtmlVideoElement, MediaStream,
    RequestCredentials, RtcIceGatheringState, RtcPeerConnection, RtcPeerConnectionState,
    RtcRtpTransceiverDirection, RtcRtpTransceiverInit, RtcSdpType, RtcSessionDescriptionInit,
    RtcTrackEvent,
};
use yew::prelude::*;

use ui::*;

const STATUS_ENDPOINT: &str = "/api/v1/status";
const PANEL_ENDPOINT: &str = "/api/v1/panel";
const DISPLAY_ENDPOINT: &str = "/api/v1/control/display";
const PROXY_LAN_TUN_ENDPOINT: &str = "/api/v1/control/proxy/lan-tun";
const PROXY_TAILSCALE_ENDPOINT: &str = "/api/v1/control/proxy/tailscale";
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
const DEVICE_POLICIES_ENDPOINT: &str = "/api/v1/proxy/device-policies";
const DEVICE_POLICIES_UPDATE_ENDPOINT: &str = "/api/v1/control/proxy/device-policies";
const TAILSCALE_ENDPOINT: &str = "/api/v1/tailscale";
const TAILSCALE_PEERS_ENDPOINT: &str = "/api/v1/tailscale/peers";
const TAILSCALE_MODE_ENDPOINT: &str = "/api/v1/control/tailscale/mode";
const TAILSCALE_LOGIN_ENDPOINT: &str = "/api/v1/control/tailscale/login";
const TAILSCALE_LOGOUT_ENDPOINT: &str = "/api/v1/control/tailscale/logout";
const CAMERA_STATUS_ENDPOINT: &str = "/api/v1/camera/status";
const CAMERA_SESSION_CREATE_ENDPOINT: &str = "/api/v1/control/camera/session/create";
const CAMERA_SESSION_CLOSE_ENDPOINT: &str = "/api/v1/control/camera/session/close";
const CAMERA_PROFILE_UPDATE_ENDPOINT: &str = "/api/v1/control/camera/profile";
const CAMERA_ROTATION_UPDATE_ENDPOINT: &str = "/api/v1/control/camera/rotation";
const CAMERA_ICE_GATHER_TIMEOUT_MS: u32 = 10_000;
const CAMERA_ICE_POLL_MS: u32 = 50;
const POLL_DELAY_MS: u32 = 2_000;
const NETWORK_APPLY_PAINT_DELAY_MS: u32 = 150;
const MISSING: &str = "—";

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthSessionDto {
    authenticated: bool,
    must_change: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraStatusResponseDto {
    camera: CameraStatus,
    available_presets: Vec<CameraStreamPreset>,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CameraProfileUpdateRequestDto {
    preset: CameraStreamPreset,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraProfileUpdateResponseDto {
    applied: bool,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CameraRotationUpdateRequestDto {
    rotation: CameraRotation,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraRotationUpdateResponseDto {
    applied: bool,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CameraSessionCreateRequestDto {
    offer_sdp: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CameraSessionCreateResponseDto {
    session_id: String,
    answer_sdp: String,
    negotiation_timeout_seconds: u16,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CameraSessionCloseRequestDto {
    session_id: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CameraViewPhase {
    Idle,
    Starting,
    Connecting,
    Playing,
}

impl CameraViewPhase {
    const fn label(self) -> &'static str {
        match self {
            Self::Idle => "未播放",
            Self::Starting => "正在协商",
            Self::Connecting => "正在连接",
            Self::Playing => "直播中",
        }
    }
}

struct CameraSessionRuntime {
    peer: RtcPeerConnection,
    _on_track: Closure<dyn FnMut(RtcTrackEvent)>,
    _on_connection_state_change: Closure<dyn FnMut(Event)>,
    session_id: Option<String>,
    csrf: String,
}

type CameraRuntime = Rc<RefCell<Option<CameraSessionRuntime>>>;

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

#[derive(Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum DevicePolicyDto {
    Direct,
    Proxy,
}

#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DevicePolicyEntryDto {
    mac: String,
    label: String,
    policy: DevicePolicyDto,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DevicePolicyConfigDto {
    version: u8,
    generation: u64,
    entries: Vec<DevicePolicyEntryDto>,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LanClientDto {
    mac: String,
    lease_address: Option<String>,
    hostname: Option<String>,
    associated: bool,
    policy: DevicePolicyDto,
}

#[derive(Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DevicePolicySnapshotDto {
    config: DevicePolicyConfigDto,
    clients: Vec<LanClientDto>,
    effective: bool,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct DevicePolicyUpdateDto {
    expected_generation: u64,
    entries: Vec<DevicePolicyEntryDto>,
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

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ProxyFeatureRequestDto {
    enabled: bool,
}

#[derive(serde::Serialize)]
#[serde(deny_unknown_fields)]
struct TailscaleModeRequestDto {
    mode: TailscaleMode,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TailscaleResponseDto {
    tailscale: TailscaleStatus,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TailscalePeersResponseDto {
    peers: TailscalePeerSnapshot,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TailscaleMutationResponseDto {
    tailscale: TailscaleStatus,
    login_url: Option<String>,
}

#[derive(Clone, PartialEq, Default)]
struct AppState {
    snapshot: Option<StatusSnapshot>,
    panel: Option<PanelBootstrap>,
    last_update: Option<String>,
    poll_error: Option<String>,
    display_notice: Option<String>,
    display_busy: bool,
    lan_tun_notice: Option<String>,
    lan_tun_busy: bool,
    tailscale_proxy_notice: Option<String>,
    tailscale_proxy_busy: bool,
    node_notice: Option<String>,
    node_busy: bool,
    loading: bool,
    session_checked: bool,
    session: Option<AuthSessionDto>,
    settings_notice: Option<String>,
    settings_busy: bool,
    network: Option<NetworkConfigDto>,
    pending_network: Option<NetworkPendingDto>,
    scan_entries: Vec<WifiScanDto>,
    subscription: Option<SubscriptionDto>,
    device_policies: Option<DevicePolicySnapshotDto>,
    tailscale: Option<TailscaleStatus>,
    tailscale_peers: Option<TailscalePeerSnapshot>,
    tailscale_peers_error: Option<String>,
    tailscale_login_url: Option<String>,
}

#[derive(Clone, Copy)]
enum ControlArea {
    Display,
    LanTun,
    TailscaleProxy,
    Nodes,
}

type SettingsData = (
    NetworkConfigDto,
    NetworkPendingDto,
    SubscriptionDto,
    DevicePolicySnapshotDto,
    TailscaleStatus,
);

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
    SettingsFinished(
        Result<SettingsData, String>,
        Result<TailscalePeerSnapshot, String>,
    ),
    TailscaleMutationFinished(Result<(TailscaleStatus, Option<String>, String), String>),
    TailscalePeersFinished(Result<TailscalePeerSnapshot, String>),
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
                    ControlArea::LanTun => {
                        next.lan_tun_busy = true;
                        next.lan_tun_notice = None;
                    }
                    ControlArea::TailscaleProxy => {
                        next.tailscale_proxy_busy = true;
                        next.tailscale_proxy_notice = None;
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
                    ControlArea::TailscaleProxy => {
                        next.tailscale_proxy_busy = false;
                        next.tailscale_proxy_notice = notice;
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
    Network,
}

impl WorkspaceView {
    const fn tab_id(self) -> &'static str {
        match self {
            Self::Overview => "overview-tab",
            Self::Network => "network-tab",
        }
    }

    const fn panel_id(self) -> &'static str {
        match self {
            Self::Overview => "overview-panel",
            Self::Network => "network-panel",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Overview => "总览",
            Self::Network => "网络设置",
        }
    }
}

#[derive(Properties, PartialEq)]
struct CameraLiveViewProps {
    csrf: String,
    stop_generation: u32,
}

fn camera_pipeline_label(state: CameraPipelineState) -> &'static str {
    match state {
        CameraPipelineState::Stopped => "已停止",
        CameraPipelineState::Starting => "启动中",
        CameraPipelineState::Streaming => "推流中",
        CameraPipelineState::Stopping => "停止中",
        CameraPipelineState::Failed => "故障",
        CameraPipelineState::Unknown => "未知",
    }
}

fn camera_access_label(access: CameraAccessKind) -> &'static str {
    match access {
        CameraAccessKind::Lan => "LAN",
        CameraAccessKind::Tailscale => "Tailscale",
    }
}

fn camera_error_label(error: CameraErrorCategory) -> &'static str {
    match error {
        CameraErrorCategory::CameraNotFound => "未找到摄像头",
        CameraErrorCategory::CameraBusy => "摄像头占用中",
        CameraErrorCategory::MediaPipelineFailed => "媒体管线故障",
        CameraErrorCategory::EncoderUnavailable => "编码器不可用",
        CameraErrorCategory::ControlUnavailable => "控制服务不可用",
        CameraErrorCategory::WebrtcNegotiationFailed => "WebRTC 协商失败",
        CameraErrorCategory::WebrtcTransportFailed => "WebRTC 传输失败",
        CameraErrorCategory::ResourceExhausted => "资源不足",
        CameraErrorCategory::Unknown => "未知故障",
    }
}

fn camera_js_error(context: &str, error: JsValue) -> String {
    let detail = error.as_string().unwrap_or_else(|| format!("{error:?}"));
    format!("{context}：{detail}")
}

fn next_camera_generation(generation: &Rc<RefCell<u64>>) -> u64 {
    let mut current = generation.borrow_mut();
    *current = current.wrapping_add(1);
    *current
}

fn camera_generation_is_current(generation: &Rc<RefCell<u64>>, expected: u64) -> bool {
    *generation.borrow() == expected
}

async fn close_camera_session(session_id: String, csrf: String) {
    let _ = post_json(
        CAMERA_SESSION_CLOSE_ENDPOINT,
        &csrf,
        &CameraSessionCloseRequestDto { session_id },
        "摄像头会话关闭",
    )
    .await;
}

fn take_camera_runtime(runtime: &CameraRuntime, video: &NodeRef) -> Option<(String, String)> {
    let session = runtime.borrow_mut().take().and_then(|active| {
        active.peer.set_ontrack(None);
        active.peer.set_onconnectionstatechange(None);
        active.peer.close();
        active
            .session_id
            .map(|session_id| (session_id, active.csrf))
    });
    if let Some(video) = video.cast::<HtmlVideoElement>() {
        let _ = video.pause();
        video.set_src_object(None);
        video.load();
    }
    session
}

fn close_camera_runtime(runtime: &CameraRuntime, video: &NodeRef) {
    if let Some((session_id, csrf)) = take_camera_runtime(runtime, video) {
        spawn_local(close_camera_session(session_id, csrf));
    }
}

async fn wait_for_camera_ice(peer: &RtcPeerConnection) -> Result<(), String> {
    let mut waited = 0;
    while peer.ice_gathering_state() != RtcIceGatheringState::Complete {
        if waited >= CAMERA_ICE_GATHER_TIMEOUT_MS {
            return Err("ICE 候选收集超时".to_owned());
        }
        TimeoutFuture::new(CAMERA_ICE_POLL_MS).await;
        waited += CAMERA_ICE_POLL_MS;
    }
    Ok(())
}

async fn refresh_camera_status(
    status: UseStateHandle<Option<CameraStatus>>,
    presets: UseStateHandle<Vec<CameraStreamPreset>>,
    error: UseStateHandle<Option<String>>,
) {
    match fetch_json::<CameraStatusResponseDto>(CAMERA_STATUS_ENDPOINT, "摄像头状态").await {
        Ok(response) => {
            status.set(Some(response.camera));
            presets.set(response.available_presets);
            error.set(None);
        }
        Err(message) => {
            status.set(None);
            presets.set(Vec::new());
            error.set(Some(message));
        }
    }
}

#[function_component(CameraLiveView)]
fn camera_live_view(props: &CameraLiveViewProps) -> Html {
    let video = use_node_ref();
    let stage = use_node_ref();
    let status = use_state(|| None::<CameraStatus>);
    let presets = use_state(|| Vec::<CameraStreamPreset>::new());
    let status_error = use_state(|| None::<String>);
    let notice = use_state(|| None::<String>);
    let phase = use_state(|| CameraViewPhase::Idle);
    let runtime = use_mut_ref(|| None::<CameraSessionRuntime>);
    let generation = use_mut_ref(|| 0u64);
    let previous_stop_generation = use_mut_ref(|| props.stop_generation);
    // 乐观旋转状态：点击后立即推进 0 → 270 → 180 → 90 → 0，不依赖 2s 轮询的
    // 延迟刷新，保证「旋转画面」每次点击都严格逆时针 90°。
    let rotation = use_mut_ref(|| None::<CameraRotation>);

    {
        let status = status.clone();
        let rotation = rotation.clone();
        use_effect_with(status.clone(), move |_| {
            if rotation.borrow().is_none() {
                *rotation.borrow_mut() = status.as_ref().map(|camera| camera.profile.rotation);
            }
            || ()
        });
    }

    {
        let status = status.clone();
        let presets = presets.clone();
        let status_error = status_error.clone();
        use_effect_with((), move |_| {
            let cancelled = Rc::new(Cell::new(false));
            let task_cancelled = cancelled.clone();
            spawn_local(async move {
                while !task_cancelled.get() {
                    refresh_camera_status(status.clone(), presets.clone(), status_error.clone())
                        .await;
                    TimeoutFuture::new(POLL_DELAY_MS).await;
                }
            });
            move || cancelled.set(true)
        });
    }

    {
        let runtime = runtime.clone();
        let video = video.clone();
        let phase = phase.clone();
        let notice = notice.clone();
        let generation = generation.clone();
        let previous = previous_stop_generation.clone();
        use_effect_with(props.stop_generation, move |requested| {
            let changed = *previous.borrow() != *requested;
            *previous.borrow_mut() = *requested;
            if changed {
                next_camera_generation(&generation);
                close_camera_runtime(&runtime, &video);
                phase.set(CameraViewPhase::Idle);
                notice.set(Some("摄像头直播已在退出登录前停止".to_owned()));
            }
            || ()
        });
    }

    {
        let runtime = runtime.clone();
        let video = video.clone();
        let phase = phase.clone();
        let notice = notice.clone();
        let generation = generation.clone();
        use_effect_with((), move |_| {
            let window = web_sys::window();
            let document = window.as_ref().and_then(|window| window.document());

            let pagehide_runtime = runtime.clone();
            let pagehide_video = video.clone();
            let pagehide_generation = generation.clone();
            let pagehide = Closure::<dyn FnMut(Event)>::new(move |_| {
                next_camera_generation(&pagehide_generation);
                close_camera_runtime(&pagehide_runtime, &pagehide_video);
            });

            let hidden_runtime = runtime.clone();
            let hidden_video = video.clone();
            let hidden_phase = phase.clone();
            let hidden_notice = notice.clone();
            let hidden_generation = generation.clone();
            let watched_document = document.clone();
            let visibility = Closure::<dyn FnMut(Event)>::new(move |_| {
                if watched_document
                    .as_ref()
                    .is_some_and(|document| document.hidden())
                {
                    next_camera_generation(&hidden_generation);
                    close_camera_runtime(&hidden_runtime, &hidden_video);
                    hidden_phase.set(CameraViewPhase::Idle);
                    hidden_notice.set(Some("页面离开前已停止摄像头直播".to_owned()));
                }
            });

            if let Some(window) = &window {
                let _ = window.add_event_listener_with_callback(
                    "pagehide",
                    pagehide.as_ref().unchecked_ref(),
                );
            }
            if let Some(document) = &document {
                let _ = document.add_event_listener_with_callback(
                    "visibilitychange",
                    visibility.as_ref().unchecked_ref(),
                );
            }

            move || {
                if let Some(window) = &window {
                    let _ = window.remove_event_listener_with_callback(
                        "pagehide",
                        pagehide.as_ref().unchecked_ref(),
                    );
                }
                if let Some(document) = &document {
                    let _ = document.remove_event_listener_with_callback(
                        "visibilitychange",
                        visibility.as_ref().unchecked_ref(),
                    );
                }
                next_camera_generation(&generation);
                close_camera_runtime(&runtime, &video);
            }
        });
    }

    let start = {
        let csrf = props.csrf.clone();
        let video = video.clone();
        let phase = phase.clone();
        let notice = notice.clone();
        let runtime = runtime.clone();
        let generation = generation.clone();
        Callback::from(move |_| {
            if csrf.is_empty() || runtime.borrow().is_some() {
                return;
            }
            close_camera_runtime(&runtime, &video);
            let attempt = next_camera_generation(&generation);
            phase.set(CameraViewPhase::Starting);
            notice.set(None);

            let csrf = csrf.clone();
            let video = video.clone();
            let phase = phase.clone();
            let notice = notice.clone();
            let runtime = runtime.clone();
            let generation = generation.clone();
            spawn_local(async move {
                let result = async {
                    let peer = RtcPeerConnection::new()
                        .map_err(|error| camera_js_error("无法创建 WebRTC 连接", error))?;
                    let transceiver = RtcRtpTransceiverInit::new();
                    transceiver.set_direction(RtcRtpTransceiverDirection::Recvonly);
                    peer.add_transceiver_with_str_and_init("video", &transceiver);

                    let track_video = video.clone();
                    let track_phase = phase.clone();
                    let track_notice = notice.clone();
                    let on_track =
                        Closure::<dyn FnMut(RtcTrackEvent)>::new(move |event: RtcTrackEvent| {
                            let stream = event
                                .streams()
                                .get(0)
                                .dyn_into::<MediaStream>()
                                .ok()
                                .or_else(|| {
                                    let stream = MediaStream::new().ok()?;
                                    stream.add_track(&event.track());
                                    Some(stream)
                                });
                            if let (Some(video), Some(stream)) =
                                (track_video.cast::<HtmlVideoElement>(), stream)
                            {
                                video.set_src_object(Some(&stream));
                                let _ = video.play();
                                track_phase.set(CameraViewPhase::Playing);
                                track_notice.set(None);
                            }
                        });
                    peer.set_ontrack(Some(on_track.as_ref().unchecked_ref()));

                    let connection_peer = peer.clone();
                    let connection_runtime = runtime.clone();
                    let connection_video = video.clone();
                    let connection_phase = phase.clone();
                    let connection_notice = notice.clone();
                    let connection_generation = generation.clone();
                    let on_connection_state_change = Closure::<dyn FnMut(Event)>::new(move |_| {
                        if matches!(
                            connection_peer.connection_state(),
                            RtcPeerConnectionState::Disconnected | RtcPeerConnectionState::Failed
                        ) {
                            next_camera_generation(&connection_generation);
                            close_camera_runtime(&connection_runtime, &connection_video);
                            connection_phase.set(CameraViewPhase::Idle);
                            connection_notice.set(Some("摄像头 WebRTC 连接已中断".to_owned()));
                        }
                    });
                    peer.set_onconnectionstatechange(Some(
                        on_connection_state_change.as_ref().unchecked_ref(),
                    ));
                    *runtime.borrow_mut() = Some(CameraSessionRuntime {
                        peer: peer.clone(),
                        _on_track: on_track,
                        _on_connection_state_change: on_connection_state_change,
                        session_id: None,
                        csrf: csrf.clone(),
                    });

                    let offer_value = JsFuture::from(peer.create_offer())
                        .await
                        .map_err(|error| camera_js_error("无法创建 WebRTC offer", error))?;
                    let offer: RtcSessionDescriptionInit = offer_value.unchecked_into();
                    JsFuture::from(peer.set_local_description(&offer))
                        .await
                        .map_err(|error| camera_js_error("无法设置本地 WebRTC 描述", error))?;
                    if !camera_generation_is_current(&generation, attempt) {
                        return Ok::<_, String>(());
                    }
                    wait_for_camera_ice(&peer).await?;
                    if !camera_generation_is_current(&generation, attempt) {
                        return Ok::<_, String>(());
                    }
                    let offer_sdp = peer
                        .local_description()
                        .map(|description| description.sdp())
                        .filter(|sdp| !sdp.is_empty())
                        .ok_or_else(|| "浏览器没有生成可提交的 WebRTC offer".to_owned())?;
                    let response = post_json_response::<_, CameraSessionCreateResponseDto>(
                        CAMERA_SESSION_CREATE_ENDPOINT,
                        &csrf,
                        &CameraSessionCreateRequestDto { offer_sdp },
                        "摄像头会话创建",
                    )
                    .await?;

                    if !camera_generation_is_current(&generation, attempt) {
                        spawn_local(close_camera_session(response.session_id, csrf.clone()));
                        return Ok::<_, String>(());
                    }
                    if let Some(active) = runtime.borrow_mut().as_mut() {
                        active.session_id = Some(response.session_id);
                    }

                    if !camera_generation_is_current(&generation, attempt) {
                        return Ok::<_, String>(());
                    }
                    phase.set(CameraViewPhase::Connecting);
                    let answer = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
                    answer.set_sdp(&response.answer_sdp);
                    JsFuture::from(peer.set_remote_description(&answer))
                        .await
                        .map_err(|error| camera_js_error("无法设置摄像头 WebRTC answer", error))?;
                    if !camera_generation_is_current(&generation, attempt) {
                        return Ok::<_, String>(());
                    }

                    let timeout_ms =
                        u32::from(response.negotiation_timeout_seconds).saturating_mul(1_000);
                    let timeout_phase = phase.clone();
                    let timeout_notice = notice.clone();
                    let timeout_runtime = runtime.clone();
                    let timeout_video = video.clone();
                    let timeout_generation = generation.clone();
                    spawn_local(async move {
                        TimeoutFuture::new(timeout_ms).await;
                        if camera_generation_is_current(&timeout_generation, attempt)
                            && *timeout_phase == CameraViewPhase::Connecting
                        {
                            next_camera_generation(&timeout_generation);
                            close_camera_runtime(&timeout_runtime, &timeout_video);
                            timeout_phase.set(CameraViewPhase::Idle);
                            timeout_notice.set(Some("摄像头 WebRTC 协商超时".to_owned()));
                        }
                    });
                    Ok(())
                }
                .await;

                if let Err(error) = result {
                    if camera_generation_is_current(&generation, attempt) {
                        next_camera_generation(&generation);
                        close_camera_runtime(&runtime, &video);
                        phase.set(CameraViewPhase::Idle);
                        notice.set(Some(format!("播放失败：{error}")));
                    }
                }
            });
        })
    };

    let stop = {
        let runtime = runtime.clone();
        let video = video.clone();
        let phase = phase.clone();
        let notice = notice.clone();
        let generation = generation.clone();
        Callback::from(move |_| {
            next_camera_generation(&generation);
            close_camera_runtime(&runtime, &video);
            phase.set(CameraViewPhase::Idle);
            notice.set(Some("摄像头直播已停止".to_owned()));
        })
    };

    let rotate = {
        let csrf = props.csrf.clone();
        let status = status.clone();
        let notice = notice.clone();
        let phase = phase.clone();
        let runtime = runtime.clone();
        let video = video.clone();
        let generation = generation.clone();
        let start = start.clone();
        let rotation = rotation.clone();
        Callback::from(move |_| {
            let current = rotation
                .borrow()
                .or_else(|| status.as_ref().map(|camera| camera.profile.rotation));
            let Some(current) = current else {
                return;
            };
            let next = current.next_rotation();
            *rotation.borrow_mut() = Some(next);
            let was_playing = *phase == CameraViewPhase::Playing;
            let pending_close = take_camera_runtime(&runtime, &video);
            if was_playing {
                phase.set(CameraViewPhase::Idle);
            }
            next_camera_generation(&generation);
            let csrf = csrf.clone();
            let notice = notice.clone();
            let phase = phase.clone();
            let start = start.clone();
            let rotation = rotation.clone();
            spawn_local(async move {
                if let Some((session_id, token)) = pending_close {
                    close_camera_session(session_id, token).await;
                }
                match post_json_response::<_, CameraRotationUpdateResponseDto>(
                    CAMERA_ROTATION_UPDATE_ENDPOINT,
                    &csrf,
                    &CameraRotationUpdateRequestDto { rotation: next },
                    "摄像头画面设置",
                )
                .await
                {
                    Ok(response) if response.applied => {
                        notice.set(Some(format!("画面已旋转：{}°", next.degrees())));
                        if was_playing {
                            phase.set(CameraViewPhase::Idle);
                            start.emit(());
                        }
                    }
                    Ok(_) => {
                        *rotation.borrow_mut() = None;
                        notice.set(Some("画面旋转未生效".to_owned()));
                    }
                    Err(message) => {
                        *rotation.borrow_mut() = None;
                        notice.set(Some(message));
                    }
                }
            });
        })
    };

    let set_profile = {
        let csrf = props.csrf.clone();
        let notice = notice.clone();
        let phase = phase.clone();
        let runtime = runtime.clone();
        let video = video.clone();
        let generation = generation.clone();
        let start = start.clone();
        Callback::from(move |event: Event| {
            let select: HtmlSelectElement = event.target_unchecked_into();
            let Some(preset) = CameraStreamPreset::ALL
                .into_iter()
                .find(|candidate| candidate.id() == select.value())
            else {
                return;
            };
            let was_playing = *phase == CameraViewPhase::Playing;
            let pending_close = take_camera_runtime(&runtime, &video);
            if was_playing {
                phase.set(CameraViewPhase::Idle);
            }
            next_camera_generation(&generation);
            let csrf = csrf.clone();
            let notice = notice.clone();
            let phase = phase.clone();
            let start = start.clone();
            spawn_local(async move {
                if let Some((session_id, token)) = pending_close {
                    close_camera_session(session_id, token).await;
                }
                match post_json_response::<_, CameraProfileUpdateResponseDto>(
                    CAMERA_PROFILE_UPDATE_ENDPOINT,
                    &csrf,
                    &CameraProfileUpdateRequestDto { preset },
                    "摄像头画面设置",
                )
                .await
                {
                    Ok(response) if response.applied => {
                        notice.set(Some(format!("画面已切换：{}", preset.label())));
                        if was_playing {
                            phase.set(CameraViewPhase::Idle);
                            start.emit(());
                        }
                    }
                    Ok(_) => notice.set(Some("画面设置未生效".to_owned())),
                    Err(message) => notice.set(Some(message)),
                }
            });
        })
    };

    let start_button = {
        let start = start.clone();
        Callback::from(move |_| start.emit(()))
    };

    let camera_available = status.as_ref().is_some_and(|camera| camera.available);
    html! {
        <article class={CAMERA_CARD} aria-labelledby="camera-live-title">
            <div class={CAMERA_LAYOUT}>
                <div class={CAMERA_COPY}>
                    <div class={CONTROL_TITLE}>
                        <div>
                            <p class={EYEBROW}>{"CAMERA"}</p>
                            <h3 id="camera-live-title" class={CONTROL_HEADING}>{"摄像头直播"}</h3>
                        </div>
                        <span class={classes!(STATUS_BADGE, if *phase == CameraViewPhase::Playing { "text-success" } else { "text-base-content/60" })}>
                            <span class={STATUS_DOT_SMALL} aria-hidden="true"></span>
                            {phase.label()}
                        </span>
                    </div>
                    if let Some(camera) = status.as_ref() {
                        <dl class={CAMERA_METRICS}>
                            <div class={CAMERA_METRIC}><dt>{"设备"}</dt><dd>{if camera.available { "可用" } else { "不可用" }}</dd></div>
                            <div class={CAMERA_METRIC}><dt>{"管线"}</dt><dd>{camera_pipeline_label(camera.pipeline)}</dd></div>
                            <div class={CAMERA_METRIC}><dt>{"画面"}</dt><dd>{format!("{} × {} · {} fps · {:.1} Mbps · {} · 旋转 {}°", camera.profile.width, camera.profile.height, camera.profile.fps, camera.profile.bitrate_bps as f64 / 1_000_000.0, camera.profile.codec, camera.profile.rotation.degrees())}</dd></div>
                            <div class={CAMERA_METRIC}><dt>{"访问"}</dt><dd>{format!("{} · {} 个会话", camera_access_label(camera.access), camera.active_sessions)}</dd></div>
                        </dl>
                        if !presets.is_empty() {
                            <label class={FIELD}>
                                <span class={FIELD_LABEL}>{"画面分辨率 / 码率（切换会自动停止并重新打开直播）"}</span>
                                <select class={SELECT} onchange={set_profile} disabled={!camera_available || props.csrf.is_empty()} aria-label="画面分辨率与码率">
                                    {for presets.iter().map(|preset| {
                                        let selected = camera.profile.width == preset.width()
                                            && camera.profile.height == preset.height()
                                            && camera.profile.bitrate_bps == preset.bitrate_bps();
                                        html! {
                                            <option value={preset.id()} selected={selected}>{preset.label()}</option>
                                        }
                                    })}
                                </select>
                            </label>
                        }
                        if let Some(error) = camera.error_category {
                            <p class="text-xs text-error" role="status">{camera_error_label(error)}</p>
                        }
                    } else if let Some(error) = status_error.as_ref() {
                        <p class="text-xs text-error" role="status">{format!("状态读取失败：{error}")}</p>
                    } else {
                        <p class={HELP_TEXT} role="status">{"正在读取摄像头状态…"}</p>
                    }
                    <p class={HELP_TEXT}>{"视频不会自动启动。点击播放后，浏览器仅接收设备视频轨道；离开页面或退出登录会立即停止会话。"}</p>
                    <div class={BUTTON_ROW}>
                        if *phase == CameraViewPhase::Idle {
                            <button class={BUTTON_PRIMARY} type="button" onclick={start_button} disabled={!camera_available || props.csrf.is_empty()}>{"播放直播"}</button>
                        } else {
                            <button class={BUTTON_ERROR} type="button" onclick={stop}>{"停止直播"}</button>
                        }
                    </div>
                    if let Some(message) = notice.as_ref() {
                        <p class={CAMERA_NOTICE} role="status" aria-live="polite">{message}</p>
                    }
                </div>
                <div ref={stage} class={CAMERA_STAGE}>
                    <video ref={video} class={CAMERA_VIDEO} autoplay=true playsinline=true muted=true aria-label="摄像头实时画面"></video>
                    if *phase == CameraViewPhase::Playing {
                        <button class={CAMERA_ROTATE_BUTTON} type="button" onclick={rotate} aria-label="旋转画面">
                            {"旋转画面"}
                        </button>
                    }
                    if *phase == CameraViewPhase::Idle {
                        <div class={CAMERA_PLACEHOLDER} aria-hidden="true">
                            <span class={CAMERA_PLACEHOLDER_ICON}>{"LIVE"}</span>
                            <span>{"等待手动播放"}</span>
                        </div>
                    }
                </div>
            </div>
        </article>
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
                    if let Some(panel) = &state.panel {
                        {render_proxy_groups_read_only(&panel.panel.proxy_groups)}
                    }
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

async fn fetch_settings_data() -> Result<SettingsData, String> {
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

async fn fetch_tailscale_peers() -> Result<TailscalePeerSnapshot, String> {
    fetch_json::<TailscalePeersResponseDto>(TAILSCALE_PEERS_ENDPOINT, "Tailscale 设备列表")
        .await
        .map(|response| response.peers)
}

fn dispatch_settings_refresh(state: UseReducerHandle<AppState>) {
    state.dispatch(Action::SettingsStarted);
    spawn_local(async move {
        let settings = fetch_settings_data().await;
        let peers = fetch_tailscale_peers().await;
        state.dispatch(Action::SettingsFinished(settings, peers));
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
            let settings = fetch_settings_data().await;
            let peers = fetch_tailscale_peers().await;
            state.dispatch(Action::SettingsFinished(settings, peers));
        }
        state.dispatch(Action::SettingsMutationFinished(
            result.map(|_| success.to_owned()),
        ));
    });
}

fn dispatch_tailscale_mutation<T: serde::Serialize + 'static>(
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
            let settings = fetch_settings_data().await;
            let peers = fetch_tailscale_peers().await;
            state.dispatch(Action::SettingsFinished(settings, peers));
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
            let settings = fetch_settings_data().await;
            let peers = fetch_tailscale_peers().await;
            state.dispatch(Action::SettingsFinished(settings, peers));
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
    let camera_stop_generation = use_state(|| 0u32);
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
        let camera_stop_generation = camera_stop_generation.clone();
        Callback::from(move |_| {
            login_expanded.set(false);
            camera_stop_generation.set((*camera_stop_generation).wrapping_add(1));
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
                        <button class="btn btn-ghost btn-sm text-base-content" type="button" onclick={logout} disabled={busy || csrf.is_empty()}>{"退出登录"}</button>
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
                <>
                    <CameraLiveView csrf={csrf.clone()} stop_generation={*camera_stop_generation} />
                    {render_proxy_control(state)}
                    {render_tailscale_control(state, &csrf)}
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
                    <DevicePolicies state={state.clone()} csrf={csrf.clone()} />
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
                </>
            }
        </section>
    }
}

fn dispatch_device_policy_update(
    state: UseReducerHandle<AppState>,
    csrf: String,
    generation: u64,
    entries: Vec<DevicePolicyEntryDto>,
) {
    state.dispatch(Action::SettingsStarted);
    spawn_local(async move {
        let result = post_json(
            DEVICE_POLICIES_UPDATE_ENDPOINT,
            &csrf,
            &DevicePolicyUpdateDto {
                expected_generation: generation,
                entries,
            },
            "设备代理策略",
        )
        .await;
        if result.is_ok() {
            let settings = fetch_settings_data().await;
            let peers = fetch_tailscale_peers().await;
            state.dispatch(Action::SettingsFinished(settings, peers));
        }
        let message = result
            .map(|_| "设备代理策略已保存".to_owned())
            .map_err(|error| {
                if error.contains("HTTP 409") {
                    "设备策略更新未完成（HTTP 409）；请重新加载并核对当前配置后重试".to_owned()
                } else {
                    error
                }
            });
        state.dispatch(Action::SettingsMutationFinished(message));
    });
}

#[derive(Properties, PartialEq)]
struct DevicePoliciesProps {
    state: UseReducerHandle<AppState>,
    csrf: String,
}

#[function_component(DevicePolicies)]
fn device_policies(props: &DevicePoliciesProps) -> Html {
    let manual_mac = use_node_ref();
    let manual_label = use_node_ref();
    let Some(snapshot) = props.state.device_policies.as_ref() else {
        return html! { <article class={INNER_CARD} aria-label="设备代理"><span class={HELP_TEXT}>{"正在读取设备代理策略…"}</span></article> };
    };
    let generation = snapshot.config.generation;
    let configured = snapshot.config.entries.clone();
    let add = {
        let state = props.state.clone();
        let csrf = props.csrf.clone();
        let mac = manual_mac.clone();
        let label = manual_label.clone();
        let entries = configured.clone();
        Callback::from(move |event: SubmitEvent| {
            event.prevent_default();
            let (Some(mac), Some(label)) = (
                mac.cast::<HtmlInputElement>(),
                label.cast::<HtmlInputElement>(),
            ) else {
                return;
            };
            let mut next = entries.clone();
            next.retain(|entry| !entry.mac.eq_ignore_ascii_case(&mac.value()));
            next.push(DevicePolicyEntryDto {
                mac: mac.value(),
                label: label.value(),
                policy: DevicePolicyDto::Proxy,
            });
            dispatch_device_policy_update(state.clone(), csrf.clone(), generation, next);
        })
    };
    html! {
        <article class={INNER_CARD} aria-labelledby="device-policies-title">
            <div class={CONTROL_TITLE}>
                <h3 id="device-policies-title" class={CONTROL_HEADING}>{"设备代理"}</h3>
                <span class={CONTROL_META}>{format!("配置代次 {}", generation)}</span>
            </div>
            if !snapshot.effective {
                <div class={RISK_NOTE} role="note">{"策略已保存但当前透明代理未启用；仅在全局 TUN 模式生效。"}</div>
            }
            <p class={HELP_TEXT}>{"未配置设备默认使用代理。可为每个 MAC 保存稳定显示名；手机私有/随机 MAC 改变后仍会被识别为新设备。MAC 是家庭 LAN 标识，不是强认证。"}</p>
            <div class="grid gap-3">
                {for snapshot.clients.iter().cloned().map(|client| html! {
                    <DevicePolicyRow
                        state={props.state.clone()}
                        csrf={props.csrf.clone()}
                        generation={generation}
                        configured={configured.clone()}
                        client={client}
                    />
                })}
            </div>
            <form class={FORM_GRID_COMPACT} onsubmit={add} autocomplete="off">
                <label class={FIELD}><span class={FIELD_LABEL}>{"手工添加 MAC"}</span><input class={INPUT} ref={manual_mac} required=true placeholder="02:00:00:00:00:01" maxlength="17" autocapitalize="none" spellcheck="false" /></label>
                <label class={FIELD}><span class={FIELD_LABEL}>{"显示名"}</span><input class={INPUT} ref={manual_label} maxlength="32" /></label>
                <div class={FORM_ACTIONS}><button class={BUTTON_PRIMARY} type="submit" disabled={props.state.settings_busy}>{"添加或更新为代理"}</button></div>
            </form>
        </article>
    }
}

#[derive(Properties, PartialEq)]
struct DevicePolicyRowProps {
    state: UseReducerHandle<AppState>,
    csrf: String,
    generation: u64,
    configured: Vec<DevicePolicyEntryDto>,
    client: LanClientDto,
}

#[function_component(DevicePolicyRow)]
fn device_policy_row(props: &DevicePolicyRowProps) -> Html {
    let persisted_label = props
        .configured
        .iter()
        .find(|entry| entry.mac == props.client.mac)
        .map(|entry| entry.label.clone())
        .unwrap_or_default();
    let initial_label = if persisted_label.is_empty() {
        props.client.hostname.clone().unwrap_or_default()
    } else {
        persisted_label.clone()
    };
    let label = use_state(|| initial_label.clone());
    {
        let label = label.clone();
        use_effect_with(initial_label, move |current| {
            label.set(current.clone());
            || ()
        });
    }

    let on_label_input = {
        let label = label.clone();
        Callback::from(move |event: InputEvent| {
            let input: HtmlInputElement = event.target_unchecked_into();
            label.set(input.value());
        })
    };
    let save_label = {
        let state = props.state.clone();
        let csrf = props.csrf.clone();
        let mac = props.client.mac.clone();
        let entries = props.configured.clone();
        let label = label.clone();
        let policy = props.client.policy;
        let generation = props.generation;
        Callback::from(move |_| {
            let saved_label = label.trim().to_owned();
            let mut next = entries.clone();
            next.retain(|entry| entry.mac != mac);
            if !saved_label.is_empty() || policy == DevicePolicyDto::Direct {
                next.push(DevicePolicyEntryDto {
                    mac: mac.clone(),
                    label: saved_label,
                    policy,
                });
            }
            dispatch_device_policy_update(state.clone(), csrf.clone(), generation, next);
        })
    };
    let change_policy = {
        let state = props.state.clone();
        let csrf = props.csrf.clone();
        let mac = props.client.mac.clone();
        let entries = props.configured.clone();
        let label = label.clone();
        let generation = props.generation;
        Callback::from(move |event: Event| {
            let select: HtmlSelectElement = event.target_unchecked_into();
            let policy = if select.value() == "direct" {
                DevicePolicyDto::Direct
            } else {
                DevicePolicyDto::Proxy
            };
            let saved_label = label.trim().to_owned();
            let mut next = entries.clone();
            next.retain(|entry| entry.mac != mac);
            if policy == DevicePolicyDto::Direct || !saved_label.is_empty() {
                next.push(DevicePolicyEntryDto {
                    mac: mac.clone(),
                    label: saved_label,
                    policy,
                });
            }
            dispatch_device_policy_update(state.clone(), csrf.clone(), generation, next);
        })
    };
    let clear = {
        let state = props.state.clone();
        let csrf = props.csrf.clone();
        let mac = props.client.mac.clone();
        let entries = props.configured.clone();
        let generation = props.generation;
        Callback::from(move |_| {
            let mut next = entries.clone();
            next.retain(|entry| entry.mac != mac);
            dispatch_device_policy_update(state.clone(), csrf.clone(), generation, next);
        })
    };
    let is_configured = props
        .configured
        .iter()
        .any(|entry| entry.mac == props.client.mac);
    let display_name = if label.is_empty() {
        props.client.mac.clone()
    } else {
        (*label).clone()
    };

    html! {
        <article class="grid min-w-0 gap-3 rounded-box border border-base-content/10 p-3">
            <div class="min-w-0">
                <strong class="block truncate">{display_name}</strong>
                <small class="block truncate font-mono text-base-content/65">{format!("{} · {} · {}", props.client.mac, props.client.lease_address.as_deref().unwrap_or("无租约 IP"), if props.client.associated { "在线" } else { "离线" })}</small>
            </div>
            <div class="grid min-w-0 gap-2 sm:grid-cols-[minmax(0,1fr)_auto_auto] sm:items-end">
                <label class={FIELD}>
                    <span class={FIELD_LABEL}>{"显示名"}</span>
                    <input class={INPUT} value={(*label).clone()} oninput={on_label_input} maxlength="32" aria-label={format!("{} 显示名", props.client.mac)} />
                </label>
                <button class={BUTTON} type="button" onclick={save_label} disabled={props.state.settings_busy}>{"保存名称"}</button>
                <select class={SELECT} aria-label={format!("{} 代理策略", props.client.mac)} onchange={change_policy} disabled={props.state.settings_busy}>
                    <option value="proxy" selected={props.client.policy == DevicePolicyDto::Proxy}>{"代理"}</option>
                    <option value="direct" selected={props.client.policy == DevicePolicyDto::Direct}>{"直连"}</option>
                </select>
            </div>
            if is_configured {
                <div class={FORM_ACTIONS}>
                    <button class={BUTTON_GHOST} type="button" onclick={clear} disabled={props.state.settings_busy} aria-label={format!("清除 {} 的名称和设备策略", props.client.mac)}>{"清除名称和自定义策略"}</button>
                </div>
            }
        </article>
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
                "Core {} · LAN {}",
                mihomo_core_status_label(proxy),
                lan_tun_status_label(proxy)
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
                {topology_node("PX", "Mihomo / 代理数据面", proxy_detail, component_tone(&snapshot.proxy))}
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
    let proxy_path = proxy.map(lan_tun_status_label).unwrap_or_else(missing);
    let clients = router
        .and_then(|value| value.ap_client_count)
        .map(|value| format!("{value} 台"))
        .unwrap_or_else(missing);

    html! {
        <section class={KPI_GRID} aria-label="关键网络指标">
            {kpi("WAN IPv4", wan, "上游地址")}
            {kpi("上游 Wi-Fi", sta, "当前连接")}
            {kpi("LAN 代理", proxy_path, "透明代理 / 普通 NAT")}
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
    let tailscale = &snapshot.tailscale;
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
            "Tailscale 中继代理",
            tailscale
                .data
                .as_ref()
                .map(tailscale_proxy_status_label)
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

fn render_tailscale_peers(state: &UseReducerHandle<AppState>) -> Html {
    let refresh = {
        let state = state.clone();
        Callback::from(move |_| dispatch_settings_refresh(state.clone()))
    };
    match (&state.tailscale_peers, &state.tailscale_peers_error) {
        (Some(snapshot), error) => {
            let summary = format!(
                "Tailnet 设备 · {} / {} 在线",
                snapshot.online, snapshot.total
            );
            html! {
                <div class="grid gap-3 rounded-box border border-base-content/10 bg-base-200/40 p-4" role="region" aria-label="Tailnet 设备">
                    <div class={CONTROL_TITLE}>
                        <h3 class={CONTROL_HEADING}>{"Tailnet 设备"}</h3>
                        <span class={CONTROL_META}>{summary}</span>
                    </div>
                    <div class={BUTTON_ROW}>
                        <button class={BUTTON} type="button" onclick={refresh.clone()} disabled={state.settings_busy}>{"重新读取设备"}</button>
                    </div>
                    if let Some(error) = error {
                        <div class={RISK_NOTE} role="status">{format!("设备列表读取失败，当前显示上次成功数据，数据可能已过期：{error}")}</div>
                    }
                    if snapshot.peers.is_empty() {
                        <div class={SETTINGS_EMPTY} role="status">{"暂无其他 Tailnet 设备"}</div>
                    } else {
                        <details class="group rounded-box border border-base-content/10 bg-base-100/70 p-3">
                            <summary class="cursor-pointer font-medium">{"查看设备列表"}</summary>
                            <ul class="mt-3 grid gap-2">
                                {for snapshot.peers.iter().map(|peer| {
                                    let status = if peer.online { "在线" } else { "离线" };
                                    let tone = if peer.online { Tone::Good } else { Tone::Neutral };
                                    html! {
                                        <li class="grid min-w-0 gap-2 rounded-box border border-base-content/10 p-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center" key={peer.ipv4.to_string()}>
                                            <div class="min-w-0">
                                                <strong class="block truncate" title={peer.name.clone()}>{&peer.name}</strong>
                                                <span class={HELP_TEXT}>{format!("{}{}", peer.ipv4, peer.os.as_ref().map_or_else(String::new, |os| format!(" · {os}")))}</span>
                                            </div>
                                            <span class={classes!(STATUS_BADGE, tone.class())}><span class={STATUS_DOT_SMALL} aria-hidden="true"></span>{status}</span>
                                        </li>
                                    }
                                })}
                            </ul>
                        </details>
                    }
                    <small class={HELP_TEXT}>{"“在线”仅表示该设备当前连接到 Tailnet，不表示它正在访问本路由器的 LAN。"}</small>
                </div>
            }
        }
        (None, Some(error)) => html! {
            <div class="grid gap-3">
                <div class={RISK_NOTE} role="status">{format!("Tailnet 设备列表暂不可用：{error}")}</div>
                <div class={BUTTON_ROW}><button class={BUTTON} type="button" onclick={refresh.clone()} disabled={state.settings_busy}>{"重新读取设备"}</button></div>
            </div>
        },
        (None, None) => html! {
            <div class={SETTINGS_EMPTY} role="status">{"正在读取 Tailnet 设备列表…"}</div>
        },
    }
}

fn render_tailscale_control(state: &UseReducerHandle<AppState>, csrf: &str) -> Html {
    let Some(tailscale) = state.tailscale.as_ref() else {
        return html! {
            <section class={SECTION} aria-labelledby="tailscale-title">
                <div class={SECTION_HEAD}><div><p class={EYEBROW}>{"REMOTE LAN"}</p><h2 id="tailscale-title" class={SECTION_TITLE}>{"Tailscale 远程 LAN"}</h2></div></div>
                <div class={SETTINGS_EMPTY} role="status">{"正在读取 Tailscale 状态…"}</div>
            </section>
        };
    };
    let busy = state.settings_busy;
    let enable = {
        let state = state.clone();
        let csrf = csrf.to_owned();
        Callback::from(move |_| {
            dispatch_tailscale_mutation(
                state.clone(),
                TAILSCALE_MODE_ENDPOINT,
                csrf.clone(),
                TailscaleModeRequestDto {
                    mode: TailscaleMode::LanSubnetAccess,
                },
                "已请求启用远程 LAN 访问",
            )
        })
    };
    let disable = {
        let state = state.clone();
        let csrf = csrf.to_owned();
        Callback::from(move |_| {
            dispatch_tailscale_mutation(
                state.clone(),
                TAILSCALE_MODE_ENDPOINT,
                csrf.clone(),
                TailscaleModeRequestDto {
                    mode: TailscaleMode::Disabled,
                },
                "Tailscale 已停用，设备认证已保留",
            )
        })
    };
    let request_login = {
        let state = state.clone();
        let csrf = csrf.to_owned();
        Callback::from(move |_| {
            dispatch_tailscale_mutation(
                state.clone(),
                TAILSCALE_LOGIN_ENDPOINT,
                csrf.clone(),
                EmptyRequest {},
                "已取得一次性登录链接",
            )
        })
    };
    let logout = {
        let state = state.clone();
        let csrf = csrf.to_owned();
        Callback::from(move |_| {
            dispatch_tailscale_mutation(
                state.clone(),
                TAILSCALE_LOGOUT_ENDPOINT,
                csrf.clone(),
                EmptyRequest {},
                "Tailscale 已注销并停用",
            )
        })
    };
    let desired = tailscale
        .desired_mode
        .map(tailscale_mode_label)
        .unwrap_or("未知");
    let effective = tailscale
        .effective_mode
        .map(tailscale_mode_label)
        .unwrap_or("尚未就绪");
    let connection = tailscale_connection_label(tailscale);
    let needs_login = tailscale.backend_state == TailscaleBackendState::NeedsLogin
        || tailscale.authenticated == Some(false)
            && tailscale.desired_mode != Some(TailscaleMode::Disabled);
    let lan_access_ready = tailscale.effective_mode == Some(TailscaleMode::LanSubnetAccess)
        && tailscale.backend_state == TailscaleBackendState::Running
        && tailscale.authenticated == Some(true)
        && tailscale.route_advertised == Some(true)
        && tailscale.local_firewall_ready == Some(true);
    let disabled_ready = tailscale.desired_mode == Some(TailscaleMode::Disabled)
        && tailscale.effective_mode == Some(TailscaleMode::Disabled)
        && tailscale.backend_state == TailscaleBackendState::Stopped;
    let enable_label = if lan_access_ready {
        "远程 LAN 访问已启用"
    } else {
        "启用远程 LAN 访问"
    };
    let disable_label = if disabled_ready {
        "Tailscale 已停用"
    } else {
        "停用（保留认证）"
    };

    html! {
        <section class={SECTION} aria-labelledby="tailscale-title" aria-busy={busy.to_string()}>
            <div class={SECTION_HEAD}>
                <div><p class={EYEBROW}>{"REMOTE LAN"}</p><h2 id="tailscale-title" class={SECTION_TITLE}>{"Tailscale 远程 LAN"}</h2></div>
                <span class={SECTION_META}>{"固定 192.168.8.0/24 · 不提供 Exit Node"}</span>
            </div>
            <article class={INNER_CARD} role="region" aria-label="Tailscale 远程 LAN 状态">
                <div class={CONTROL_TITLE}><h3 class={CONTROL_HEADING}>{"LAN Access"}</h3><span class={CONTROL_META}>{format!("期望 {desired} · 当前 {effective}")}</span></div>
                <dl class={METRIC_LIST}>
                    <div class={METRIC}><dt class={METRIC_LABEL}>{"认证"}</dt><dd class={METRIC_VALUE}>{tailscale.authenticated.map(|value| if value { "已认证" } else { "需要登录" }).unwrap_or("未知")}</dd></div>
                    <div class={METRIC}><dt class={METRIC_LABEL}>{"Tailscale IPv4"}</dt><dd class={METRIC_VALUE}>{tailscale.ipv4.map(|value| value.to_string()).unwrap_or_else(missing)}</dd></div>
                    <div class={METRIC}><dt class={METRIC_LABEL}>{"连接"}</dt><dd class={METRIC_VALUE}>{connection}</dd></div>
                    <div class={METRIC}><dt class={METRIC_LABEL}>{"本地路由 / 防火墙"}</dt><dd class={METRIC_VALUE}>{format!("路由 {} · 防火墙 {}", tailscale.route_advertised.map(format_bool).unwrap_or_else(missing), tailscale.local_firewall_ready.map(format_bool).unwrap_or_else(missing))}</dd></div>
                </dl>
                {render_tailscale_peers(state)}
                if needs_login {
                    <div class={classes!(RISK_ALERT, "alert-warning", "border-warning/20")} role="alert">
                        <div>
                            <strong>{"需要完成 Tailscale 登录"}</strong>
                            <p class={RISK_COPY}>{"登录链接只在本次管理员写操作响应中返回，不会保存或出现在状态 GET 中。"}</p>
                        </div>
                    </div>
                    if let Some(login_url) = &state.tailscale_login_url {
                        <div class={BUTTON_ROW}>
                            <a class={BUTTON_PRIMARY} href={login_url.clone()} target="_blank" rel="noopener noreferrer">{"打开一次性 Tailscale 登录链接"}</a>
                            <button class={BUTTON} type="button" onclick={enable.clone()} disabled={busy}>{"已完成登录，继续启用"}</button>
                        </div>
                    } else {
                        <button class={BUTTON_PRIMARY} type="button" onclick={request_login.clone()} disabled={busy}>{"取得一次性登录链接"}</button>
                    }
                }
                if lan_access_ready {
                    <div class={classes!(RISK_ALERT, "alert-success", "border-success/20")} role="status">
                        <div>
                            <strong>{"本机远程 LAN 访问已启用"}</strong>
                            <p class={RISK_COPY}>{"Tailscale 已认证，固定子网路由和本地防火墙均已就绪。"}</p>
                        </div>
                    </div>
                }
                if let Some(category) = tailscale.error_category {
                    <div class={RISK_NOTE} role="note">{format!("错误类别：{}", tailscale_error_label(category))}</div>
                }
                <div class={BUTTON_ROW} role="group" aria-label="Tailscale 操作">
                    <button
                        class={if lan_access_ready { BUTTON } else { BUTTON_PRIMARY }}
                        type="button"
                        onclick={enable}
                        disabled={busy || lan_access_ready}
                        aria-pressed={lan_access_ready.to_string()}
                    >{enable_label}</button>
                    <button
                        class={BUTTON}
                        type="button"
                        onclick={disable}
                        disabled={busy || disabled_ready}
                        aria-pressed={disabled_ready.to_string()}
                    >{disable_label}</button>
                    <button class={BUTTON_ERROR} type="button" onclick={logout} disabled={busy}>{"注销并移除认证"}</button>
                </div>
                <small class={HELP_TEXT}>{"仅支持固定 RouterOnly / LAN Access 安全模式；浏览器不能输入 URL、auth key、子网、端口或控制参数。"}</small>
            </article>
        </section>
    }
}

fn render_proxy_control(state: &UseReducerHandle<AppState>) -> Html {
    let Some(bootstrap) = state.panel.as_ref() else {
        return Html::default();
    };
    let csrf = bootstrap.csrf_token.clone();
    let proxy = state
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.proxy.data.as_ref());
    let tailscale = state
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.tailscale.data.as_ref());
    let lan_desired = proxy.and_then(|status| status.lan_tun.desired);
    let tailscale_desired = tailscale.and_then(|status| status.explicit_proxy_desired);
    let lan_status = proxy.map_or_else(|| "未知".to_owned(), lan_tun_status_label);
    let tailscale_status =
        tailscale.map_or_else(|| "未知 · 未确认".to_owned(), tailscale_proxy_status_label);
    let mihomo_status = proxy.map_or_else(|| "未知".to_owned(), mihomo_core_status_label);
    let selected_node = bootstrap
        .panel
        .proxy_groups
        .data
        .as_ref()
        .and_then(|groups| groups.iter().find_map(|group| group.selected.as_deref()))
        .unwrap_or(MISSING);
    let toggle_lan = {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let enabled = input.checked();
            dispatch_control(
                state.clone(),
                ControlArea::LanTun,
                PROXY_LAN_TUN_ENDPOINT,
                csrf.clone(),
                ProxyFeatureRequestDto { enabled },
                if enabled {
                    "LAN 透明代理已启用"
                } else {
                    "LAN 透明代理已关闭，普通 NAT 保持可用"
                }
                .to_owned(),
            );
        })
    };
    let toggle_tailscale = {
        let state = state.clone();
        let csrf = csrf.clone();
        Callback::from(move |event: Event| {
            let input: HtmlInputElement = event.target_unchecked_into();
            let enabled = input.checked();
            dispatch_control(
                state.clone(),
                ControlArea::TailscaleProxy,
                PROXY_TAILSCALE_ENDPOINT,
                csrf.clone(),
                ProxyFeatureRequestDto { enabled },
                if enabled {
                    "Tailscale 中继代理已启用"
                } else {
                    "Tailscale 中继代理已关闭，已使用 Direct"
                }
                .to_owned(),
            );
        })
    };

    html! {
        <section class={classes!(SECTION, "gap-6")} aria-labelledby="proxy-controls-title">
            <div class={SECTION_HEAD}>
                <div><p class={EYEBROW}>{"PROXY"}</p><h2 id="proxy-controls-title" class={SECTION_TITLE}>{"代理设置"}</h2></div>
                <span class={SECTION_META}>{"两个数据面独立切换，共享 Mihomo core"}</span>
            </div>
            <article class={INNER_CARD} aria-labelledby="proxy-features-title">
                <div class={CONTROL_TITLE}>
                    <h3 id="proxy-features-title" class={CONTROL_HEADING}>{"代理能力"}</h3>
                    <span class={CONTROL_META}>{format!("Mihomo core：{mihomo_status}")}</span>
                </div>
                <div class="grid gap-3">
                    <label class="flex min-w-0 items-center justify-between gap-4 rounded-box border border-base-content/10 bg-base-200/40 p-4">
                        <span class="grid min-w-0 gap-1">
                            <strong class={CONTROL_HEADING}>{"LAN 透明代理"}</strong>
                            <small class={HELP_TEXT}>{"通过 Mihomo TUN 接管来自 192.168.8.0/24 的下游流量；关闭后使用普通 NAT。"}</small>
                            <span class={CONTROL_META}>{lan_status}</span>
                        </span>
                        <input class="toggle toggle-primary shrink-0" type="checkbox" role="switch" aria-label="LAN 透明代理" checked={lan_desired == Some(true)} onchange={toggle_lan} disabled={state.lan_tun_busy || lan_desired.is_none()} />
                    </label>
                    if let Some(notice) = &state.lan_tun_notice {
                        <div class={FEEDBACK} role="status" aria-live="polite" aria-atomic="true">{notice}</div>
                    }
                    <label class="flex min-w-0 items-center justify-between gap-4 rounded-box border border-base-content/10 bg-base-200/40 p-4">
                        <span class="grid min-w-0 gap-1">
                            <strong class={CONTROL_HEADING}>{"Tailscale 中继代理"}</strong>
                            <small class={HELP_TEXT}>{"只让 tailscaled 的 HTTP/HTTPS 和 DERP 连接使用当前 Mihomo 节点；UDP direct 仍直接探测。"}</small>
                            <span class={CONTROL_META}>{tailscale_status}</span>
                        </span>
                        <input class="toggle toggle-secondary shrink-0" type="checkbox" role="switch" aria-label="Tailscale 中继代理" checked={tailscale_desired == Some(true)} onchange={toggle_tailscale} disabled={state.tailscale_proxy_busy || tailscale_desired.is_none()} />
                    </label>
                    if let Some(notice) = &state.tailscale_proxy_notice {
                        <div class={FEEDBACK} role="status" aria-live="polite" aria-atomic="true">{notice}</div>
                    }
                </div>
                <div class={SUMMARY}><span>{"当前代理节点"}</span><strong>{selected_node}</strong></div>
                <small class={HELP_TEXT}>{"停用后保留订阅配置；普通 NAT 在路由启用时保持可用；浏览器不能直连 Mihomo Controller。"}</small>
            </article>
            if let Some(notice) = &state.node_notice {
                <div class={FEEDBACK} role="status" aria-live="polite" aria-atomic="true">{notice}</div>
            }
            <div class={PROXY_GROUPS} aria-busy={state.node_busy.to_string()}>
                {render_proxy_groups(&bootstrap.panel.proxy_groups, state, &csrf, state.node_busy)}
            </div>
        </section>
    }
}

fn render_proxy_groups_read_only(component: &Component<Vec<ProxyGroup>>) -> Html {
    let Some(groups) = component.data.as_ref() else {
        return Html::default();
    };
    if groups.is_empty() {
        return Html::default();
    }
    html! {
        <section class={SECTION} aria-labelledby="proxy-readonly-title">
            <div class={SECTION_HEAD}>
                <div><p class={EYEBROW}>{"PROXY STATUS"}</p><h2 id="proxy-readonly-title" class={SECTION_TITLE}>{"当前代理与延迟"}</h2></div>
                <span class={SECTION_META}>{"只读 · 修改需管理员登录"}</span>
            </div>
            <div class={PROXY_GROUPS}>
                {for groups.iter().map(|group| {
                    let selected = group.selected.as_deref().unwrap_or(MISSING);
                    html! {
                        <article class={PROXY_GROUP} key={group.name.clone()}>
                            <div class={PROXY_NAME_WRAP}>
                                <div><h3 class={PROXY_NAME}>{&group.name}</h3><small class={HELP_TEXT}>{format!("当前选择 · {selected}")}</small></div>
                                <span class={PROXY_KIND}>{group_kind_label(group)}</span>
                            </div>
                            <dl class={METRIC_LIST}>
                                {for group.options.iter().map(|option| {
                                    let state = option.delay_ms.map_or_else(
                                        || if option.alive == Some(false) { "超时".to_owned() } else { "未测速".to_owned() },
                                        |delay| format!("{delay} ms"),
                                    );
                                    let name = option.region.as_ref().map_or_else(
                                        || option.name.clone(),
                                        |region| format!("{} · {region}", option.name),
                                    );
                                    html! {
                                        <div class={METRIC} key={option.name.clone()}>
                                            <dt class={METRIC_LABEL}>{name}</dt>
                                            <dd class={METRIC_VALUE}>{if group.selected.as_deref() == Some(option.name.as_str()) { format!("当前 · {state}") } else { state }}</dd>
                                        </div>
                                    }
                                })}
                            </dl>
                        </article>
                    }
                })}
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
                        ControlArea::Nodes,
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
    use hyz_things::domain::panel::ProxyGroupKind;
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

fn link_state_label(state: LinkState) -> &'static str {
    match state {
        LinkState::Up => "已连接",
        LinkState::Down => "已断开",
        LinkState::Connecting => "连接中",
        LinkState::Unknown => "未知",
    }
}

fn tailscale_mode_label(mode: TailscaleMode) -> &'static str {
    match mode {
        TailscaleMode::Disabled => "已停用",
        TailscaleMode::RouterOnly => "RouterOnly（仅路由器）",
        TailscaleMode::LanSubnetAccess => "LAN Access",
    }
}

fn tailscale_connection_label(status: &TailscaleStatus) -> String {
    match status.connection.kind {
        TailscaleConnectionType::Direct => "Direct".to_owned(),
        TailscaleConnectionType::PeerRelay => "Peer Relay".to_owned(),
        TailscaleConnectionType::Derp => status
            .connection
            .derp_region
            .as_ref()
            .map_or_else(|| "DERP".to_owned(), |region| format!("DERP · {region}")),
        TailscaleConnectionType::Unknown => "未知".to_owned(),
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

fn tailscale_proxy_status_label(status: &TailscaleStatus) -> String {
    match (
        status.explicit_proxy_desired,
        status.environment,
        status.explicit_proxy_path,
        status.proxy_fallback,
    ) {
        (
            Some(true),
            Some(TailscaleEnvironment::MihomoExplicit),
            TailscaleExplicitProxyPath::Ready,
            TailscaleProxyFallback::NotNeeded,
        ) => "已启用".to_owned(),
        (
            Some(true),
            Some(TailscaleEnvironment::MihomoExplicit),
            TailscaleExplicitProxyPath::Unavailable,
            _,
        ) => "已降级 · 代理路径不可用".to_owned(),
        (
            Some(true),
            Some(TailscaleEnvironment::MihomoExplicit),
            TailscaleExplicitProxyPath::Unknown,
            _,
        ) => "未知 · 代理路径未确认".to_owned(),
        (
            Some(true),
            Some(TailscaleEnvironment::Direct),
            _,
            TailscaleProxyFallback::DirectRestored,
        ) => "已降级 · 已恢复 Direct".to_owned(),
        (
            Some(false),
            Some(TailscaleEnvironment::Direct),
            TailscaleExplicitProxyPath::NotRequired,
            TailscaleProxyFallback::NotNeeded,
        ) => "已关闭 · Direct".to_owned(),
        (None, _, _, _) | (_, None, _, _) | (_, _, _, TailscaleProxyFallback::NotConfirmed) => {
            "未知 · 未确认".to_owned()
        }
        _ => "已降级 · 未确认".to_owned(),
    }
}

fn wan_traffic(
    component: &Component<hyz_things::domain::status::SystemStats>,
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
