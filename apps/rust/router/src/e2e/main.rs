#![cfg(feature = "e2e")]

use async_trait::async_trait;
use axum::{
    extract::{DefaultBodyLimit, State},
    routing::{get, put},
    Json, Router,
};
use hyz_router::{
    adapters::inbound::{
        control::{ControlHandler, ControlOperation, ControlProxyMode, ControlResult},
        http::app_with_loopback_runtime_frontend,
    },
    application::{
        admin::AdminApplication,
        camera::{CameraApplication, CameraControlPort, CameraError, CameraSession},
        device_policy::DevicePolicySnapshot,
        ports::{AdminCredentialStorePort, AdminRandomPort, ClockPort, PlatformError},
        status::{
            ReadStatus, StatusRouterPlatformPort, StatusSystemProbePort,
            StatusTailscalePlatformPort,
        },
        wifi::{WifiScanEntry, AP_CONFIRM_TIMEOUT_SECS},
    },
    domain::{
        admin::AdminCredential,
        camera::{
            CameraAccessScope, CameraPipelineState, CameraRotation, CameraStatus,
            CameraStreamPreset, CameraStreamProfile,
        },
        device_policy::{DevicePolicyConfigV1, DeviceRoutePolicy, LanClientObservation},
        network_config::{
            NetworkConfigSummary, PendingNetworkConfigSummary, WifiCountry, WifiSsid,
            NETWORK_CONFIG_VERSION,
        },
        panel::{
            DisplayStatus, PanelSnapshot, ProxyDelayResult, ProxyGroup, ProxyGroupKind, ProxyOption,
        },
        status::{
            Component, InterfaceStats, LanTunEffective, LanTunStatus, LinkState, MihomoCoreStatus,
            ProxyResourceState, ProxyStatus, RouterStatus, SystemStats, TailscaleConnectionStatus,
            TailscaleConnectionType, TailscaleExplicitProxyPath, TailscaleProxyFallback,
            TailscaleRouteApproval, TailscaleStatus,
        },
        subscription::{SubscriptionSummary, SubscriptionSummaryState},
        tailscale::{
            TailscaleBackendState, TailscaleEnvironment, TailscaleLoginUrl, TailscaleMode,
            TailscalePeer, TailscalePeerSnapshot,
        },
    },
};
use serde::{Deserialize, Serialize};
use std::{
    env,
    error::Error,
    fs,
    io::Write,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard,
    },
};

const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;
const CSRF_TOKEN: &str = "router-web-e2e-csrf";
const MAX_FRONTEND_TAR_BYTES: u64 = 64 * 1024 * 1024;
const MAX_HARNESS_JSON_BYTES: usize = 256 * 1024;
const USAGE: &str =
    "Usage: router-web-e2e <frontend.tar> [web-port] [harness-control-port]\nPorts default to 0 (ephemeral) and always bind 127.0.0.1.";

type HarnessResult<T> = Result<T, String>;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessProxyFailures {
    lan_tun: bool,
    tailscale: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessCameraState {
    status: CameraStatus,
    create_count: u64,
    close_count: u64,
    last_offer_sdp: Option<String>,
    sessions: Vec<String>,
}

impl Default for HarnessCameraState {
    fn default() -> Self {
        Self {
            status: CameraStatus {
                available: true,
                pipeline: CameraPipelineState::Stopped,
                active_sessions: 0,
                profile: CameraStreamProfile {
                    codec: "h264".to_owned(),
                    width: 3840,
                    height: 2160,
                    fps: 30,
                    bitrate_bps: 20_000_000,
                    rotation: CameraRotation::Deg0,
                },
                access: hyz_router::domain::camera::CameraAccessKind::Lan,
                error_category: None,
            },
            create_count: 0,
            close_count: 0,
            last_offer_sdp: None,
            sessions: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HarnessState {
    observed_at_unix_ms: u64,
    router: Component<RouterStatus>,
    proxy: Component<ProxyStatus>,
    tailscale: Component<TailscaleStatus>,
    tailscale_peers: TailscalePeerSnapshot,
    tailscale_peers_failure: bool,
    system: Component<SystemStats>,
    camera: HarnessCameraState,
    panel: PanelSnapshot,
    network: NetworkConfigSummary,
    pending_network: Option<PendingNetworkConfigSummary>,
    pending_network_applied: bool,
    scan_entries: Vec<WifiScanEntry>,
    subscription: SubscriptionSummary,
    device_policies: DevicePolicySnapshot,
    proxy_failures: HarnessProxyFailures,
}

impl Default for HarnessState {
    fn default() -> Self {
        let proxy_groups = vec![ProxyGroup {
            name: "自动选择".to_owned(),
            kind: ProxyGroupKind::Selector,
            selectable: true,
            selected: Some("东京".to_owned()),
            options: vec![
                ProxyOption {
                    name: "东京".to_owned(),
                    region: Some("日本".to_owned()),
                    delay_ms: Some(42),
                    alive: Some(true),
                },
                ProxyOption {
                    name: "新加坡".to_owned(),
                    region: Some("新加坡".to_owned()),
                    delay_ms: Some(68),
                    alive: Some(true),
                },
            ],
        }];
        Self {
            observed_at_unix_ms: 1_786_406_400_000,
            router: Component::available(RouterStatus {
                sta_state: Some(LinkState::Up),
                sta_ssid: Some("E2E-Upstream".to_owned()),
                sta_address: Some("192.0.2.10/24".to_owned()),
                sta_signal_dbm: Some(-48),
                default_route_present: Some(true),
                default_route_metric: Some(600),
                ap_state: Some(LinkState::Up),
                ap_client_count: Some(1),
                lan_present: Some(true),
                lan_address: Some("192.168.8.1/24".to_owned()),
                ap_attached_to_lan: Some(true),
                ipv4_forwarding: Some(true),
                masquerade_enabled: Some(true),
            }),
            proxy: Component::available(ProxyStatus {
                configured: true,
                mihomo: MihomoCoreStatus {
                    configured_required: Some(true),
                    process: ProxyResourceState::Ready,
                    runtime_config: ProxyResourceState::Ready,
                    mixed_port: ProxyResourceState::Ready,
                },
                lan_tun: LanTunStatus {
                    desired: Some(true),
                    effective: LanTunEffective::Ready,
                    ordinary_nat_fallback: Some(true),
                },
            }),
            tailscale: Component::available(TailscaleStatus {
                desired_mode: Some(TailscaleMode::Disabled),
                effective_mode: Some(TailscaleMode::Disabled),
                backend_state: TailscaleBackendState::Stopped,
                authenticated: Some(true),
                ipv4: None,
                route_advertised: Some(false),
                local_firewall_ready: Some(false),
                route_approval: TailscaleRouteApproval::UnknownExternalApprovalRequired,
                connection: TailscaleConnectionStatus {
                    kind: TailscaleConnectionType::Unknown,
                    derp_region: None,
                },
                explicit_proxy_desired: Some(false),
                environment: Some(TailscaleEnvironment::Direct),
                explicit_proxy_path: TailscaleExplicitProxyPath::NotRequired,
                proxy_fallback: TailscaleProxyFallback::NotNeeded,
                error_category: None,
            }),
            tailscale_peers: TailscalePeerSnapshot::new(vec![
                TailscalePeer::new(
                    "laptop",
                    Ipv4Addr::new(100, 64, 0, 8),
                    true,
                    Some("linux".to_owned()),
                )
                .unwrap(),
                TailscalePeer::new(
                    "tablet",
                    Ipv4Addr::new(100, 64, 0, 9),
                    false,
                    Some("android".to_owned()),
                )
                .unwrap(),
            ])
            .unwrap(),
            tailscale_peers_failure: false,
            system: Component::available(SystemStats {
                uptime_seconds: Some(3_600),
                cpu_temperature_millidegrees: Some(46_000),
                interfaces: vec![InterfaceStats {
                    name: "wlan0".to_owned(),
                    rx_bytes: 12_345_678,
                    tx_bytes: 2_345_678,
                }],
            }),
            camera: HarnessCameraState::default(),
            panel: PanelSnapshot {
                display: Component::available(DisplayStatus {
                    enabled: true,
                    brightness: 128,
                    actual_brightness: 128,
                    max_brightness: 255,
                }),
                proxy_groups: Component::available(proxy_groups),
            },
            network: NetworkConfigSummary {
                version: NETWORK_CONFIG_VERSION,
                ap_ssid: ssid("HYZ Router E2E"),
                sta_ssid: ssid("E2E-Upstream"),
                country: WifiCountry::Us,
            },
            pending_network: None,
            pending_network_applied: false,
            scan_entries: vec![
                WifiScanEntry {
                    ssid: ssid("E2E-Upstream"),
                    bssid: "02:00:00:00:00:01".to_owned(),
                    frequency_mhz: 2_437,
                    signal_dbm: -48,
                    secured: true,
                },
                WifiScanEntry {
                    ssid: ssid("Guest-Network"),
                    bssid: "02:00:00:00:00:02".to_owned(),
                    frequency_mhz: 2_412,
                    signal_dbm: -67,
                    secured: true,
                },
            ],
            subscription: SubscriptionSummary {
                configured: true,
                state: SubscriptionSummaryState::Active,
            },
            device_policies: DevicePolicySnapshot {
                config: DevicePolicyConfigV1::empty(),
                clients: vec![LanClientObservation {
                    mac: "02:00:00:00:00:10".parse().unwrap(),
                    lease_address: Some("192.168.8.10".parse().unwrap()),
                    hostname: Some("e2e-phone".to_owned()),
                    associated: true,
                    policy: DeviceRoutePolicy::Proxy,
                }],
                effective: true,
            },
            proxy_failures: HarnessProxyFailures::default(),
        }
    }
}

struct HarnessBackend {
    state: Mutex<HarnessState>,
    credential: Mutex<Option<AdminCredential>>,
    random_sequence: AtomicU64,
}

#[derive(Clone)]
struct HarnessControlState {
    backend: Arc<HarnessBackend>,
    admin: Arc<AdminApplication>,
    camera: Arc<CameraApplication>,
}

impl Default for HarnessBackend {
    fn default() -> Self {
        Self {
            state: Mutex::new(HarnessState::default()),
            credential: Mutex::new(None),
            random_sequence: AtomicU64::new(1),
        }
    }
}

impl HarnessBackend {
    fn state(&self) -> HarnessResult<MutexGuard<'_, HarnessState>> {
        self.state
            .lock()
            .map_err(|_| "harness state lock is poisoned".to_owned())
    }

    fn snapshot(&self) -> HarnessResult<HarnessState> {
        Ok(self.state()?.clone())
    }

    fn replace(&self, state: HarnessState) -> HarnessResult<HarnessState> {
        validate_harness_state(&state)?;
        *self.state()? = state;
        self.snapshot()
    }
}

struct HarnessCamera {
    backend: Arc<HarnessBackend>,
}

#[async_trait]
impl CameraControlPort for HarnessCamera {
    async fn status(&self, scope: CameraAccessScope) -> Result<CameraStatus, CameraError> {
        let mut status = self
            .backend
            .state()
            .map_err(|_| CameraError::Unavailable)?
            .camera
            .status
            .clone();
        status.access = scope.kind();
        Ok(status)
    }

    async fn create_session(
        &self,
        _scope: CameraAccessScope,
        offer_sdp: String,
    ) -> Result<CameraSession, CameraError> {
        let mut state = self.backend.state().map_err(|_| CameraError::Unavailable)?;
        if !state.camera.status.available {
            return Err(CameraError::NotReady);
        }
        state.camera.create_count += 1;
        let session_id = format!(
            "e2e-camera.{}{:02x}",
            "a".repeat(46),
            state.camera.create_count
        );
        state.camera.last_offer_sdp = Some(offer_sdp);
        state.camera.sessions.push(session_id.clone());
        state.camera.status.pipeline = CameraPipelineState::Streaming;
        state.camera.status.active_sessions =
            u8::try_from(state.camera.sessions.len()).unwrap_or(u8::MAX);
        Ok(CameraSession {
            session_id,
            answer_sdp: "v=0\r\n".to_owned(),
            negotiation_timeout_seconds: 5,
        })
    }

    async fn close_session(&self, session_id: &str) -> Result<(), CameraError> {
        let mut state = self.backend.state().map_err(|_| CameraError::Unavailable)?;
        let Some(index) = state.camera.sessions.iter().position(|id| id == session_id) else {
            return Err(CameraError::UnknownSession);
        };
        state.camera.sessions.remove(index);
        state.camera.close_count += 1;
        state.camera.status.pipeline = if state.camera.sessions.is_empty() {
            CameraPipelineState::Stopped
        } else {
            CameraPipelineState::Streaming
        };
        state.camera.status.active_sessions =
            u8::try_from(state.camera.sessions.len()).unwrap_or(u8::MAX);
        Ok(())
    }

    async fn set_profile(&self, preset: CameraStreamPreset) -> Result<(), CameraError> {
        let mut state = self.backend.state().map_err(|_| CameraError::Unavailable)?;
        if !state.camera.sessions.is_empty() {
            return Err(CameraError::Busy);
        }
        state.camera.status.profile = CameraStreamProfile {
            codec: "h264".to_owned(),
            width: preset.width(),
            height: preset.height(),
            fps: preset.fps(),
            bitrate_bps: preset.bitrate_bps(),
            rotation: state.camera.status.profile.rotation,
        };
        Ok(())
    }

    async fn set_rotation(&self, rotation: CameraRotation) -> Result<(), CameraError> {
        let mut state = self.backend.state().map_err(|_| CameraError::Unavailable)?;
        if !state.camera.sessions.is_empty() {
            return Err(CameraError::Busy);
        }
        state.camera.status.profile.rotation = rotation;
        Ok(())
    }
}

#[async_trait]
impl ControlHandler for HarnessBackend {
    async fn handle(&self, operation: ControlOperation) -> HarnessResult<ControlResult> {
        let mut state = self.state()?;
        match operation {
            ControlOperation::Status {} => {
                Err("status uses the HTTP status application".to_owned())
            }
            ControlOperation::PanelStatus {} => Ok(ControlResult::PanelStatus {
                snapshot: Box::new(state.panel.clone()),
            }),
            ControlOperation::Display { request } => {
                let display = state
                    .panel
                    .display
                    .data
                    .as_mut()
                    .ok_or_else(|| "display is unavailable".to_owned())?;
                display.enabled = request.enabled;
                display.brightness = if request.enabled {
                    let brightness = request.brightness.unwrap_or(display.brightness.max(1));
                    if brightness > display.max_brightness {
                        return Err("display brightness exceeds the fake panel maximum".to_owned());
                    }
                    brightness
                } else {
                    0
                };
                display.actual_brightness = display.brightness;
                Ok(completed("display applied"))
            }
            ControlOperation::Proxy { mode } => {
                let (lan_tun_enabled, tailscale_enabled) = match mode {
                    ControlProxyMode::Tun => (true, false),
                    ControlProxyMode::Explicit | ControlProxyMode::Disabled => (false, false),
                };
                set_proxy_features(&mut state, lan_tun_enabled, tailscale_enabled)?;
                Ok(completed("legacy proxy mode applied"))
            }
            ControlOperation::ProxyLanTun { enabled } => {
                if state.proxy_failures.lan_tun {
                    return Err("injected LAN TUN failure".to_owned());
                }
                let tailscale_enabled = tailscale_status(&state)?
                    .explicit_proxy_desired
                    .unwrap_or(false);
                set_proxy_features(&mut state, enabled, tailscale_enabled)?;
                Ok(completed("LAN TUN applied"))
            }
            ControlOperation::ProxyTailscale { enabled } => {
                if state.proxy_failures.tailscale {
                    return Err("injected Tailscale proxy failure".to_owned());
                }
                let lan_tun_enabled = state
                    .proxy
                    .data
                    .as_ref()
                    .and_then(|status| status.lan_tun.desired)
                    .unwrap_or(false);
                set_proxy_features(&mut state, lan_tun_enabled, enabled)?;
                Ok(completed("Tailscale proxy applied"))
            }
            ControlOperation::ProxySelection { request } => {
                let groups = proxy_groups_mut(&mut state)?;
                let group = groups
                    .iter_mut()
                    .find(|group| group.name == request.group && group.selectable)
                    .ok_or_else(|| "selectable proxy group was not found".to_owned())?;
                if !group
                    .options
                    .iter()
                    .any(|option| option.name == request.proxy)
                {
                    return Err("proxy is not a member of the selected group".to_owned());
                }
                group.selected = Some(request.proxy);
                Ok(completed("proxy selection applied"))
            }
            ControlOperation::ProxyDelay { request } => {
                let groups = proxy_groups_mut(&mut state)?;
                let option = groups
                    .iter_mut()
                    .flat_map(|group| group.options.iter_mut())
                    .find(|option| option.name == request.proxy)
                    .ok_or_else(|| "proxy was not found".to_owned())?;
                let delay_ms = option.delay_ms.unwrap_or(50);
                option.delay_ms = Some(delay_ms);
                option.alive = Some(true);
                Ok(ControlResult::ProxyDelay {
                    result: ProxyDelayResult {
                        proxy: request.proxy,
                        delay_ms,
                    },
                })
            }
            ControlOperation::ProxyDelayRefresh { .. } => {
                let groups = proxy_groups_mut(&mut state)?;
                for (index, option) in groups
                    .iter_mut()
                    .flat_map(|group| group.options.iter_mut())
                    .enumerate()
                {
                    option.delay_ms = Some(40 + index as u32 * 17);
                    option.alive = Some(true);
                }
                Ok(ControlResult::ProxyDelays {
                    groups: groups.clone(),
                })
            }
            ControlOperation::WifiStatus {} => Ok(ControlResult::WifiConfig {
                config: state.network.clone(),
            }),
            ControlOperation::WifiPending {} => {
                let remaining_seconds = if state.pending_network_applied {
                    state.pending_network.as_ref().map(|pending| {
                        let elapsed = state
                            .observed_at_unix_ms
                            .saturating_sub(pending.staged_at_unix_ms)
                            / 1_000;
                        AP_CONFIRM_TIMEOUT_SECS.saturating_sub(elapsed)
                    })
                } else {
                    None
                };
                Ok(ControlResult::WifiPendingStatus {
                    pending: state.pending_network.clone(),
                    applied: state.pending_network_applied,
                    remaining_seconds,
                })
            }
            ControlOperation::WifiScan {} => Ok(ControlResult::WifiScan {
                entries: state.scan_entries.clone(),
            }),
            ControlOperation::WifiStaApply { request } => {
                if let Some(router) = state.router.data.as_mut() {
                    router.sta_ssid = Some(request.ssid.as_str().to_owned());
                    router.sta_state = Some(LinkState::Up);
                }
                state.network.sta_ssid = request.ssid;
                state.pending_network = None;
                state.pending_network_applied = false;
                Ok(ControlResult::WifiConfig {
                    config: state.network.clone(),
                })
            }
            ControlOperation::WifiApPrepare { request } => {
                let mut config = state.network.clone();
                config.ap_ssid = request.ssid;
                config.country = request.country;
                let pending = PendingNetworkConfigSummary {
                    version: NETWORK_CONFIG_VERSION,
                    staged_at_unix_ms: state.observed_at_unix_ms,
                    config,
                };
                state.pending_network = Some(pending.clone());
                state.pending_network_applied = false;
                Ok(ControlResult::WifiPending { pending })
            }
            ControlOperation::WifiApApply {} => {
                let pending = state
                    .pending_network
                    .clone()
                    .ok_or_else(|| "no AP candidate is pending".to_owned())?;
                state.pending_network_applied = true;
                Ok(ControlResult::WifiPending { pending })
            }
            ControlOperation::WifiApConfirm {} => {
                let pending = state
                    .pending_network
                    .take()
                    .ok_or_else(|| "no AP candidate is pending".to_owned())?;
                state.network = pending.config;
                state.pending_network_applied = false;
                Ok(ControlResult::WifiConfig {
                    config: state.network.clone(),
                })
            }
            ControlOperation::WifiApCancel {} => {
                state.pending_network = None;
                state.pending_network_applied = false;
                Ok(ControlResult::WifiConfig {
                    config: state.network.clone(),
                })
            }
            ControlOperation::DevicePoliciesGet {} => Ok(ControlResult::DevicePolicies {
                snapshot: state.device_policies.clone(),
            }),
            ControlOperation::DevicePoliciesSet { request } => {
                if request.expected_generation != state.device_policies.config.generation {
                    return Err("device policy generation conflict".to_owned());
                }
                let candidate = request.candidate().map_err(str::to_owned)?;
                for client in &mut state.device_policies.clients {
                    client.policy = candidate.policy_for(client.mac);
                }
                for entry in &candidate.entries {
                    if !state
                        .device_policies
                        .clients
                        .iter()
                        .any(|client| client.mac == entry.mac)
                    {
                        state.device_policies.clients.push(LanClientObservation {
                            mac: entry.mac,
                            lease_address: None,
                            hostname: None,
                            associated: false,
                            policy: entry.policy,
                        });
                    }
                }
                state.device_policies.config = candidate;
                Ok(ControlResult::DevicePolicies {
                    snapshot: state.device_policies.clone(),
                })
            }
            ControlOperation::SubscriptionGet {} => Ok(ControlResult::Subscription {
                summary: state.subscription.clone(),
            }),
            ControlOperation::SubscriptionSet { .. } => {
                state.subscription = SubscriptionSummary {
                    configured: true,
                    state: SubscriptionSummaryState::Active,
                };
                Ok(ControlResult::Subscription {
                    summary: state.subscription.clone(),
                })
            }
            ControlOperation::SubscriptionRefresh {} => {
                if !state.subscription.configured {
                    return Err("subscription source is not configured".to_owned());
                }
                state.subscription.state = SubscriptionSummaryState::Active;
                Ok(ControlResult::Subscription {
                    summary: state.subscription.clone(),
                })
            }
            ControlOperation::TailscaleGet {} => Ok(ControlResult::Tailscale {
                status: tailscale_status(&state)?.clone(),
            }),
            ControlOperation::TailscalePeersGet {} => {
                let tailscale = tailscale_status(&state)?;
                if state.tailscale_peers_failure
                    || tailscale.backend_state != TailscaleBackendState::Running
                    || tailscale.authenticated != Some(true)
                {
                    Err("injected or unavailable Tailscale peers read".to_owned())
                } else {
                    Ok(ControlResult::TailscalePeers {
                        snapshot: state.tailscale_peers.clone(),
                    })
                }
            }
            ControlOperation::TailscaleMode { mode } => {
                let status = tailscale_status_mut(&mut state)?;
                status.desired_mode = Some(mode);
                let login_url = if mode == TailscaleMode::Disabled {
                    status.effective_mode = Some(TailscaleMode::Disabled);
                    status.backend_state = TailscaleBackendState::Stopped;
                    status.ipv4 = None;
                    status.route_advertised = Some(false);
                    status.local_firewall_ready = Some(false);
                    status.connection = TailscaleConnectionStatus {
                        kind: TailscaleConnectionType::Unknown,
                        derp_region: None,
                    };
                    None
                } else if status.authenticated == Some(false) {
                    status.effective_mode = None;
                    status.backend_state = TailscaleBackendState::NeedsLogin;
                    status.ipv4 = None;
                    status.route_advertised = Some(false);
                    status.local_firewall_ready = Some(false);
                    Some(
                        TailscaleLoginUrl::new("https://login.tailscale.com/a/router-e2e").unwrap(),
                    )
                } else {
                    status.backend_state = TailscaleBackendState::Running;
                    status.ipv4 = Some("100.64.0.10".parse().unwrap());
                    status.local_firewall_ready = Some(true);
                    status.effective_mode = Some(mode);
                    status.route_advertised = Some(mode == TailscaleMode::LanSubnetAccess);
                    status.connection = TailscaleConnectionStatus {
                        kind: TailscaleConnectionType::Direct,
                        derp_region: None,
                    };
                    None
                };
                Ok(ControlResult::TailscaleMutation {
                    status: status.clone(),
                    login_url,
                })
            }
            ControlOperation::TailscaleLogin {} => {
                let status = tailscale_status_mut(&mut state)?;
                let login_url = (status.authenticated == Some(false)).then(|| {
                    TailscaleLoginUrl::new("https://login.tailscale.com/a/router-e2e").unwrap()
                });
                Ok(ControlResult::TailscaleMutation {
                    status: status.clone(),
                    login_url,
                })
            }
            ControlOperation::TailscaleLogout {} => {
                let status = tailscale_status_mut(&mut state)?;
                status.desired_mode = Some(TailscaleMode::Disabled);
                status.effective_mode = Some(TailscaleMode::Disabled);
                status.backend_state = TailscaleBackendState::Stopped;
                status.authenticated = Some(false);
                status.ipv4 = None;
                status.route_advertised = Some(false);
                status.local_firewall_ready = Some(false);
                status.connection = TailscaleConnectionStatus {
                    kind: TailscaleConnectionType::Unknown,
                    derp_region: None,
                };
                Ok(ControlResult::TailscaleMutation {
                    status: status.clone(),
                    login_url: None,
                })
            }
            ControlOperation::Router { .. }
            | ControlOperation::Ota { .. }
            | ControlOperation::Dhcp { .. } => {
                Err("operation is outside the router web harness boundary".to_owned())
            }
        }
    }
}

#[async_trait]
impl StatusRouterPlatformPort for HarnessBackend {
    async fn read_router_status(&self) -> Component<RouterStatus> {
        self.state()
            .map(|state| state.router.clone())
            .unwrap_or_else(|error| Component::unavailable(harness_issue(error)))
    }

    async fn read_proxy_status(&self) -> Component<ProxyStatus> {
        self.state()
            .map(|state| state.proxy.clone())
            .unwrap_or_else(|error| Component::unavailable(harness_issue(error)))
    }
}

#[async_trait]
impl StatusTailscalePlatformPort for HarnessBackend {
    async fn read_tailscale_status(&self) -> Component<TailscaleStatus> {
        self.state()
            .map(|state| state.tailscale.clone())
            .unwrap_or_else(|error| Component::unavailable(harness_issue(error)))
    }
}

#[async_trait]
impl StatusSystemProbePort for HarnessBackend {
    async fn read_system_stats(&self) -> Component<SystemStats> {
        self.state()
            .map(|state| state.system.clone())
            .unwrap_or_else(|error| Component::unavailable(harness_issue(error)))
    }
}

impl ClockPort for HarnessBackend {
    fn unix_time_millis(&self) -> u64 {
        self.state()
            .map(|state| state.observed_at_unix_ms)
            .unwrap_or_default()
    }
}

impl AdminCredentialStorePort for HarnessBackend {
    fn load_admin_credential(&self) -> Result<Option<AdminCredential>, PlatformError> {
        self.credential
            .lock()
            .map(|credential| credential.clone())
            .map_err(|_| PlatformError::InvalidState("e2e credential lock is poisoned".to_owned()))
    }

    fn save_admin_credential(&self, credential: &AdminCredential) -> Result<(), PlatformError> {
        *self.credential.lock().map_err(|_| {
            PlatformError::InvalidState("e2e credential lock is poisoned".to_owned())
        })? = Some(credential.clone());
        Ok(())
    }
}

impl AdminRandomPort for HarnessBackend {
    fn fill_random(&self, destination: &mut [u8]) -> Result<(), PlatformError> {
        let sequence = self.random_sequence.fetch_add(1, Ordering::Relaxed);
        for (index, byte) in destination.iter_mut().enumerate() {
            *byte = sequence.wrapping_add(index as u64) as u8;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Config {
    frontend_tar: PathBuf,
    web_port: u16,
    control_port: u16,
}

impl Config {
    fn parse() -> Result<Self, String> {
        let mut arguments = env::args_os().skip(1);
        let frontend_tar = arguments
            .next()
            .map(PathBuf::from)
            .ok_or_else(|| USAGE.to_owned())?;
        let web_port = parse_port(arguments.next(), "web port")?;
        let control_port = parse_port(arguments.next(), "harness control port")?;
        if arguments.next().is_some() {
            return Err(USAGE.to_owned());
        }
        Ok(Self {
            frontend_tar,
            web_port,
            control_port,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::parse().map_err(std::io::Error::other)?;
    let metadata = fs::metadata(&config.frontend_tar)?;
    if !metadata.is_file() || metadata.len() > MAX_FRONTEND_TAR_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "frontend tar must be a regular file no larger than 64 MiB",
        )
        .into());
    }
    let frontend_tar = fs::read(&config.frontend_tar)?;

    let web_listener =
        tokio::net::TcpListener::bind(SocketAddr::from((LOOPBACK, config.web_port))).await?;
    let web_address = web_listener.local_addr()?;
    let control_listener =
        tokio::net::TcpListener::bind(SocketAddr::from((LOOPBACK, config.control_port))).await?;
    let control_address = control_listener.local_addr()?;
    let web_origin = format!("http://{web_address}");
    let control_origin = format!("http://{control_address}");

    let backend = Arc::new(HarnessBackend::default());
    let admin = Arc::new(AdminApplication::initialize(
        backend.clone(),
        backend.clone(),
        backend.clone(),
    )?);
    let read_status = ReadStatus::new_uncached_with_tailscale(
        backend.clone(),
        backend.clone(),
        backend.clone(),
        backend.clone(),
    );
    let web_control: Arc<dyn ControlHandler> = backend.clone();
    let camera = Arc::new(CameraApplication::new(Arc::new(HarnessCamera {
        backend: backend.clone(),
    })));
    let web_app = app_with_loopback_runtime_frontend(
        read_status,
        web_control,
        admin.clone(),
        camera.clone(),
        CSRF_TOKEN.to_owned(),
        web_origin.clone(),
        &frontend_tar,
    )?;
    let harness_app = harness_control_app(backend, admin, camera);

    println!(
        "{}",
        serde_json::json!({
            "web_origin": web_origin,
            "harness_control_origin": control_origin,
            "frontend_tar": config.frontend_tar,
        })
    );
    std::io::stdout().flush()?;

    tokio::select! {
        result = axum::serve(web_listener, web_app) => result?,
        result = axum::serve(control_listener, harness_app) => result?,
        result = tokio::signal::ctrl_c() => result?,
    }
    Ok(())
}

fn harness_control_app(
    backend: Arc<HarnessBackend>,
    admin: Arc<AdminApplication>,
    camera: Arc<CameraApplication>,
) -> Router {
    Router::new()
        .route("/health", get(harness_health))
        .route("/state", get(harness_state).put(replace_harness_state))
        .route("/reset", put(reset_harness_state))
        .layer(DefaultBodyLimit::max(MAX_HARNESS_JSON_BYTES))
        .with_state(HarnessControlState {
            backend,
            admin,
            camera,
        })
}

async fn harness_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

async fn harness_state(
    State(control): State<HarnessControlState>,
) -> Result<Json<HarnessState>, (axum::http::StatusCode, String)> {
    control
        .backend
        .snapshot()
        .map(Json)
        .map_err(harness_control_error)
}

async fn replace_harness_state(
    State(control): State<HarnessControlState>,
    Json(state): Json<HarnessState>,
) -> Result<Json<HarnessState>, (axum::http::StatusCode, String)> {
    control
        .backend
        .replace(state)
        .map(Json)
        .map_err(harness_control_error)
}

async fn reset_harness_state(
    State(control): State<HarnessControlState>,
) -> Result<Json<HarnessState>, (axum::http::StatusCode, String)> {
    control.camera.close_all().await;
    control
        .admin
        .reset_e2e_bootstrap()
        .map_err(|error| harness_internal_error(error.to_string()))?;
    control
        .backend
        .replace(HarnessState::default())
        .map(Json)
        .map_err(harness_control_error)
}

fn validate_harness_state(state: &HarnessState) -> HarnessResult<()> {
    validate_component("router", &state.router)?;
    validate_component("proxy", &state.proxy)?;
    validate_component("tailscale", &state.tailscale)?;
    validate_component("system", &state.system)?;
    validate_component("display", &state.panel.display)?;
    validate_component("proxy groups", &state.panel.proxy_groups)?;
    if state.network.version != NETWORK_CONFIG_VERSION {
        return Err("network summary version is unsupported".to_owned());
    }
    if state.device_policies.config.version
        != hyz_router::domain::device_policy::DEVICE_POLICY_VERSION
    {
        return Err("device policy version is unsupported".to_owned());
    }
    if state.pending_network_applied && state.pending_network.is_none() {
        return Err("an applied network candidate must be present".to_owned());
    }
    if state.pending_network.as_ref().is_some_and(|pending| {
        pending.version != NETWORK_CONFIG_VERSION
            || pending.config.version != NETWORK_CONFIG_VERSION
    }) {
        return Err("pending network summary version is unsupported".to_owned());
    }
    let validated_peers = TailscalePeerSnapshot::new(
        state
            .tailscale_peers
            .peers
            .iter()
            .map(|peer| {
                TailscalePeer::new(peer.name.clone(), peer.ipv4, peer.online, peer.os.clone())
                    .ok_or_else(|| "Tailscale peer state violates field bounds".to_owned())
            })
            .collect::<HarnessResult<Vec<_>>>()?,
    )
    .ok_or_else(|| "Tailscale peer snapshot violates count or uniqueness bounds".to_owned())?;
    if validated_peers != state.tailscale_peers {
        return Err("Tailscale peer snapshot counts or ordering are inconsistent".to_owned());
    }
    if let Some(display) = &state.panel.display.data {
        if display.brightness > display.max_brightness
            || display.actual_brightness > display.max_brightness
            || (!display.enabled && (display.brightness != 0 || display.actual_brightness != 0))
        {
            return Err("display state violates brightness invariants".to_owned());
        }
    }
    if let Some(groups) = &state.panel.proxy_groups.data {
        for group in groups {
            if group.selectable
                && group.selected.as_ref().is_some_and(|selected| {
                    !group.options.iter().any(|option| &option.name == selected)
                })
            {
                return Err("selected proxy must belong to its group".to_owned());
            }
        }
    }
    Ok(())
}

fn validate_component<T>(label: &str, component: &Component<T>) -> HarnessResult<()> {
    let valid = match component.state {
        hyz_router::domain::status::ComponentState::Available => {
            component.data.is_some() && component.issue.is_none()
        }
        hyz_router::domain::status::ComponentState::Degraded => {
            component.data.is_some() && component.issue.is_some()
        }
        hyz_router::domain::status::ComponentState::Unavailable => {
            component.data.is_none() && component.issue.is_some()
        }
    };
    if valid {
        Ok(())
    } else {
        Err(format!("{label} component state is inconsistent"))
    }
}

fn set_proxy_features(
    state: &mut HarnessState,
    lan_tun_enabled: bool,
    tailscale_enabled: bool,
) -> HarnessResult<()> {
    let core_required = lan_tun_enabled || tailscale_enabled;
    let core_state = if core_required {
        ProxyResourceState::Ready
    } else {
        ProxyResourceState::Absent
    };
    let proxy = state
        .proxy
        .data
        .as_mut()
        .ok_or_else(|| "proxy status is unavailable".to_owned())?;
    proxy.mihomo.configured_required = Some(core_required);
    proxy.mihomo.process = core_state;
    proxy.mihomo.runtime_config = core_state;
    proxy.mihomo.mixed_port = core_state;
    proxy.lan_tun.desired = Some(lan_tun_enabled);
    proxy.lan_tun.effective = if lan_tun_enabled {
        LanTunEffective::Ready
    } else {
        LanTunEffective::OrdinaryNat
    };
    proxy.lan_tun.ordinary_nat_fallback = Some(true);
    state.proxy.state = hyz_router::domain::status::ComponentState::Available;
    state.proxy.issue = None;
    state.device_policies.effective = lan_tun_enabled;

    let tailscale = tailscale_status_mut(state)?;
    tailscale.explicit_proxy_desired = Some(tailscale_enabled);
    tailscale.environment = Some(if tailscale_enabled {
        TailscaleEnvironment::MihomoExplicit
    } else {
        TailscaleEnvironment::Direct
    });
    tailscale.explicit_proxy_path = if tailscale_enabled {
        TailscaleExplicitProxyPath::Ready
    } else {
        TailscaleExplicitProxyPath::NotRequired
    };
    tailscale.proxy_fallback = TailscaleProxyFallback::NotNeeded;
    tailscale.error_category = None;
    state.tailscale.state = hyz_router::domain::status::ComponentState::Available;
    state.tailscale.issue = None;
    Ok(())
}

fn tailscale_status(state: &HarnessState) -> HarnessResult<&TailscaleStatus> {
    state
        .tailscale
        .data
        .as_ref()
        .ok_or_else(|| "Tailscale status is unavailable".to_owned())
}

fn tailscale_status_mut(state: &mut HarnessState) -> HarnessResult<&mut TailscaleStatus> {
    state
        .tailscale
        .data
        .as_mut()
        .ok_or_else(|| "Tailscale status is unavailable".to_owned())
}

fn proxy_groups_mut(state: &mut HarnessState) -> HarnessResult<&mut Vec<ProxyGroup>> {
    state
        .panel
        .proxy_groups
        .data
        .as_mut()
        .ok_or_else(|| "proxy groups are unavailable".to_owned())
}

fn completed(message: impl Into<String>) -> ControlResult {
    ControlResult::Completed {
        message: message.into(),
    }
}

fn harness_issue(message: String) -> hyz_router::domain::status::Issue {
    hyz_router::domain::status::Issue::new("e2e_harness_unavailable", message)
}

fn harness_control_error(error: String) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::BAD_REQUEST, error)
}

fn harness_internal_error(error: String) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, error)
}

fn ssid(value: &str) -> WifiSsid {
    WifiSsid::new(value).expect("fixed e2e SSID must be valid")
}

fn parse_port(value: Option<std::ffi::OsString>, label: &str) -> Result<u16, String> {
    let Some(value) = value else {
        return Ok(0);
    };
    value
        .into_string()
        .map_err(|_| format!("{label} must be UTF-8"))?
        .parse::<u16>()
        .map_err(|_| format!("{label} must be an integer from 0 through 65535"))
}
