use super::*;
use std::collections::BTreeSet;

#[derive(Clone, PartialEq, Default)]
pub(crate) struct ResourceMeta {
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    pub(crate) last_success: Option<String>,
    pub(crate) request_id: u64,
    pub(crate) stale: bool,
}

impl ResourceMeta {
    fn started(&mut self, request_id: u64) {
        if request_id >= self.request_id {
            self.request_id = request_id;
            self.loading = true;
        }
    }

    fn finished<T>(&mut self, request_id: u64, result: &Result<T, String>) -> bool {
        if request_id < self.request_id {
            return false;
        }
        self.request_id = request_id;
        self.loading = false;
        match result {
            Ok(_) => {
                self.error = None;
                self.last_success = Some(current_time());
                self.stale = false;
            }
            Err(error) => {
                self.error = Some(error.clone());
                self.stale = true;
            }
        }
        true
    }

    pub(crate) fn status_text(&self) -> String {
        match (&self.last_success, &self.error) {
            (Some(time), Some(_)) => format!("上次成功 {time} · 当前刷新失败"),
            (Some(time), None) => format!("上次成功 {time}"),
            (None, Some(_)) => "尚无成功数据".to_owned(),
            (None, None) if self.loading => "正在读取".to_owned(),
            _ => "尚未读取".to_owned(),
        }
    }
}

#[derive(Clone, PartialEq, Default)]
pub(crate) struct AppState {
    pub(crate) snapshot: Option<StatusSnapshot>,
    pub(crate) panel: Option<PanelBootstrap>,
    pub(crate) last_update: Option<String>,
    pub(crate) poll_error: Option<String>,
    pub(crate) loading: bool,
    pub(crate) status_meta: ResourceMeta,
    pub(crate) panel_meta: ResourceMeta,

    pub(crate) display_notice: Option<String>,
    pub(crate) display_busy: bool,
    pub(crate) lan_tun_notice: Option<String>,
    pub(crate) lan_tun_busy: bool,
    pub(crate) local_system_proxy_notice: Option<String>,
    pub(crate) local_system_proxy_busy: bool,
    pub(crate) node_notice: Option<String>,
    pub(crate) node_busy: bool,

    pub(crate) session_checked: bool,
    pub(crate) session: Option<AuthSessionDto>,
    pub(crate) auth_epoch: u64,

    pub(crate) settings_notice: Option<String>,
    pub(crate) settings_busy: bool,
    pub(crate) scan_entries: Vec<WifiScanDto>,
    pub(crate) network: Option<NetworkConfigDto>,
    pub(crate) network_meta: ResourceMeta,
    pub(crate) pending_network: Option<NetworkPendingDto>,
    pub(crate) pending_meta: ResourceMeta,

    pub(crate) subscription: Option<SubscriptionDto>,
    pub(crate) subscription_meta: ResourceMeta,
    pub(crate) subscription_notice: Option<String>,
    pub(crate) subscription_busy: bool,

    pub(crate) device_policies: Option<DevicePolicySnapshotDto>,
    pub(crate) device_meta: ResourceMeta,
    pub(crate) device_notice: Option<String>,
    pub(crate) device_busy: bool,

    pub(crate) tailscale: Option<TailscaleStatus>,
    pub(crate) tailscale_meta: ResourceMeta,
    pub(crate) tailscale_peers: Option<TailscalePeerSnapshot>,
    pub(crate) tailscale_peers_meta: ResourceMeta,
    pub(crate) tailscale_peers_error: Option<String>,
    pub(crate) tailscale_login_url: Option<String>,
    pub(crate) tailscale_notice: Option<String>,
    pub(crate) tailscale_busy: bool,

    pub(crate) apps: Option<Vec<InstalledAppDto>>,
    pub(crate) apps_meta: ResourceMeta,
    pub(crate) apps_error: Option<String>,
}

#[derive(Clone, Copy)]
pub(crate) enum ControlArea {
    Display,
    LanTun,
    LocalSystemProxy,
    Nodes,
}

pub(crate) enum Action {
    StatusStarted(u64),
    StatusFinished(u64, Result<StatusSnapshot, String>),
    PanelStarted(u64),
    PanelFinished(u64, Result<PanelBootstrap, String>),
    NetworkStarted(u64, u64),
    NetworkFinished(u64, u64, Result<NetworkConfigDto, String>),
    PendingStarted(u64, u64),
    PendingFinished(u64, u64, Result<NetworkPendingDto, String>),
    SubscriptionStarted(u64, u64),
    SubscriptionFinished(u64, u64, Result<SubscriptionDto, String>),
    DevicesStarted(u64, u64),
    DevicesFinished(u64, u64, Result<DevicePolicySnapshotDto, String>),
    TailscaleStarted(u64, u64),
    TailscaleFinished(u64, u64, Result<TailscaleStatus, String>),
    TailscalePeersStarted(u64, u64),
    TailscalePeersFinished(u64, u64, Result<TailscalePeerSnapshot, String>),
    AppsStarted(u64),
    AppsFinished(u64, Result<Vec<InstalledAppDto>, String>),

    SessionFinished(Result<AuthSessionDto, String>),
    AuthStarted,
    AuthFinished(Result<(AuthSessionDto, String), String>),
    AuthenticationExpired(u64, String),

    ControlStarted(ControlArea),
    ControlFinished(ControlArea, Result<String, String>),
    ProxyDelaysFinished(Result<Vec<ProxyGroup>, String>),

    SettingsStarted,
    SettingsMutationFinished(Result<String, String>),
    ScanFinished(Result<Vec<WifiScanDto>, String>),
    SettingsNotice(String),

    SubscriptionMutationStarted,
    SubscriptionMutationFinished(String),
    DeviceMutationStarted,
    DeviceMutationFinished(String),
    TailscaleMutationStarted,
    TailscaleMutationFinished(Result<(TailscaleStatus, Option<String>, String), String>),
}

fn clear_protected(state: &mut AppState) {
    state.network = None;
    state.network_meta = ResourceMeta::default();
    state.pending_network = None;
    state.pending_meta = ResourceMeta::default();
    state.subscription = None;
    state.subscription_meta = ResourceMeta::default();
    state.subscription_notice = None;
    state.subscription_busy = false;
    state.device_policies = None;
    state.device_meta = ResourceMeta::default();
    state.device_notice = None;
    state.device_busy = false;
    state.tailscale = None;
    state.tailscale_meta = ResourceMeta::default();
    state.tailscale_peers = None;
    state.tailscale_peers_meta = ResourceMeta::default();
    state.tailscale_peers_error = None;
    state.tailscale_login_url = None;
    state.tailscale_notice = None;
    state.tailscale_busy = false;
    state.scan_entries.clear();
}

impl Reducible for AppState {
    type Action = Action;

    fn reduce(self: Rc<Self>, action: Self::Action) -> Rc<Self> {
        let mut next = (*self).clone();
        match action {
            Action::StatusStarted(id) => {
                next.status_meta.started(id);
                next.loading = next.snapshot.is_none();
            }
            Action::StatusFinished(id, result) => {
                if next.status_meta.finished(id, &result) {
                    next.loading = false;
                    match result {
                        Ok(snapshot) => {
                            next.snapshot = Some(snapshot);
                            next.last_update = next.status_meta.last_success.clone();
                            next.poll_error = None;
                        }
                        Err(error) => next.poll_error = Some(error),
                    }
                }
            }
            Action::PanelStarted(id) => next.panel_meta.started(id),
            Action::PanelFinished(id, result) => {
                if next.panel_meta.finished(id, &result) {
                    if let Ok(panel) = result {
                        next.panel = Some(panel);
                    }
                }
            }
            Action::NetworkStarted(id, epoch) if epoch == next.auth_epoch => {
                next.network_meta.started(id)
            }
            Action::NetworkFinished(id, epoch, result) if epoch == next.auth_epoch => {
                if next.network_meta.finished(id, &result) {
                    if let Ok(network) = result {
                        next.network = Some(network);
                    }
                }
            }
            Action::PendingStarted(id, epoch) if epoch == next.auth_epoch => {
                next.pending_meta.started(id)
            }
            Action::PendingFinished(id, epoch, result) if epoch == next.auth_epoch => {
                if next.pending_meta.finished(id, &result) {
                    if let Ok(pending) = result {
                        next.pending_network = Some(pending);
                    }
                }
            }
            Action::SubscriptionStarted(id, epoch) if epoch == next.auth_epoch => {
                next.subscription_meta.started(id)
            }
            Action::SubscriptionFinished(id, epoch, result) if epoch == next.auth_epoch => {
                if next.subscription_meta.finished(id, &result) {
                    if let Ok(subscription) = result {
                        next.subscription = Some(subscription);
                    }
                }
            }
            Action::DevicesStarted(id, epoch) if epoch == next.auth_epoch => {
                next.device_meta.started(id)
            }
            Action::DevicesFinished(id, epoch, result) if epoch == next.auth_epoch => {
                if next.device_meta.finished(id, &result) {
                    if let Ok(snapshot) = result {
                        next.device_policies = Some(snapshot);
                    }
                }
            }
            Action::TailscaleStarted(id, epoch) if epoch == next.auth_epoch => {
                next.tailscale_meta.started(id)
            }
            Action::TailscaleFinished(id, epoch, result) if epoch == next.auth_epoch => {
                if next.tailscale_meta.finished(id, &result) {
                    if let Ok(tailscale) = result {
                        next.tailscale = Some(tailscale);
                    }
                }
            }
            Action::TailscalePeersStarted(id, epoch) if epoch == next.auth_epoch => {
                next.tailscale_peers_meta.started(id)
            }
            Action::TailscalePeersFinished(id, epoch, result) if epoch == next.auth_epoch => {
                if next.tailscale_peers_meta.finished(id, &result) {
                    match result {
                        Ok(peers) => {
                            next.tailscale_peers = Some(peers);
                            next.tailscale_peers_error = None;
                        }
                        Err(error) => next.tailscale_peers_error = Some(error),
                    }
                }
            }
            Action::AppsStarted(id) => next.apps_meta.started(id),
            Action::AppsFinished(id, result) => {
                if next.apps_meta.finished(id, &result) {
                    match result {
                        Ok(apps) => {
                            next.apps = Some(apps);
                            next.apps_error = None;
                        }
                        Err(error) => next.apps_error = Some(error),
                    }
                }
            }
            Action::SessionFinished(result) => {
                next.session_checked = true;
                match result {
                    Ok(session) => {
                        let changed = next.session.as_ref().map(|old| old.authenticated)
                            != Some(session.authenticated);
                        if changed {
                            next.auth_epoch = next.auth_epoch.wrapping_add(1);
                        }
                        if !session.authenticated {
                            clear_protected(&mut next);
                        }
                        next.session = Some(session);
                    }
                    Err(error) => {
                        next.session = None;
                        next.settings_notice = Some(format!("无法检查登录状态：{error}"));
                        clear_protected(&mut next);
                    }
                }
            }
            Action::AuthStarted => {
                next.settings_busy = true;
                next.settings_notice = None;
            }
            Action::AuthFinished(result) => {
                next.settings_busy = false;
                match result {
                    Ok((session, message)) => {
                        next.auth_epoch = next.auth_epoch.wrapping_add(1);
                        if !session.authenticated || session.must_change {
                            clear_protected(&mut next);
                        }
                        next.session_checked = true;
                        next.session = Some(session);
                        next.settings_notice = Some(message);
                    }
                    Err(error) => next.settings_notice = Some(format!("操作失败：{error}")),
                }
            }
            Action::AuthenticationExpired(epoch, message) if epoch == next.auth_epoch => {
                next.auth_epoch = next.auth_epoch.wrapping_add(1);
                clear_protected(&mut next);
                next.session_checked = true;
                next.session = Some(AuthSessionDto {
                    authenticated: false,
                    must_change: false,
                });
                next.settings_busy = false;
                next.settings_notice = Some(message);
            }
            Action::ControlStarted(area) => match area {
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
            },
            Action::ControlFinished(area, result) => {
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
            }
            Action::ProxyDelaysFinished(result) => {
                next.node_busy = false;
                match result {
                    Ok(groups) => {
                        if let Some(bootstrap) = &mut next.panel {
                            bootstrap.panel.proxy_groups = Component::available(groups);
                        }
                        next.node_notice = None;
                    }
                    Err(error) => next.node_notice = Some(format!("测速失败：{error}")),
                }
            }
            Action::SettingsStarted => {
                next.settings_busy = true;
                next.settings_notice = None;
            }
            Action::SettingsMutationFinished(result) => {
                next.settings_busy = false;
                next.settings_notice = Some(match result {
                    Ok(message) => message,
                    Err(error) => format!("操作失败：{error}"),
                });
            }
            Action::ScanFinished(result) => {
                next.settings_busy = false;
                match result {
                    Ok(entries) => {
                        next.scan_entries = entries;
                        next.settings_notice = Some("STA 扫描已完成".to_owned());
                    }
                    Err(error) => next.settings_notice = Some(format!("扫描失败：{error}")),
                }
            }
            Action::SettingsNotice(message) => next.settings_notice = Some(message),
            Action::SubscriptionMutationStarted => {
                next.subscription_busy = true;
                next.subscription_notice = None;
            }
            Action::SubscriptionMutationFinished(message) => {
                next.subscription_busy = false;
                next.subscription_notice = Some(message);
            }
            Action::DeviceMutationStarted => {
                next.device_busy = true;
                next.device_notice = None;
            }
            Action::DeviceMutationFinished(message) => {
                next.device_busy = false;
                next.device_notice = Some(message);
            }
            Action::TailscaleMutationStarted => {
                next.tailscale_busy = true;
                next.tailscale_notice = None;
            }
            Action::TailscaleMutationFinished(result) => {
                next.tailscale_busy = false;
                match result {
                    Ok((tailscale, login_url, message)) => {
                        let logged_out = tailscale.authenticated == Some(false)
                            && tailscale.desired_mode == Some(TailscaleMode::Disabled);
                        next.tailscale = Some(tailscale);
                        next.tailscale_meta.error = None;
                        next.tailscale_meta.last_success = Some(current_time());
                        next.tailscale_meta.stale = false;
                        next.tailscale_login_url = login_url;
                        next.tailscale_notice = Some(message);
                        if logged_out {
                            next.tailscale_peers = None;
                            next.tailscale_peers_meta = ResourceMeta::default();
                            next.tailscale_peers_error = None;
                        }
                    }
                    Err(error) => next.tailscale_notice = Some(format!("操作失败：{error}")),
                }
            }
            _ => {}
        }
        next.into()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ResourceKey {
    Status,
    Panel,
    Network,
    Pending,
    Subscription,
    Devices,
    Tailscale,
    TailscalePeers,
    Apps,
}

thread_local! {
    static RESOURCE_IN_FLIGHT: RefCell<BTreeSet<ResourceKey>> = RefCell::new(BTreeSet::new());
    static NEXT_REQUEST_ID: Cell<u64> = const { Cell::new(1) };
}

fn begin_resource(key: ResourceKey) -> Option<u64> {
    let inserted = RESOURCE_IN_FLIGHT.with(|set| set.borrow_mut().insert(key));
    if !inserted {
        return None;
    }
    Some(NEXT_REQUEST_ID.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1).max(1));
        id
    }))
}

fn finish_resource(key: ResourceKey) {
    RESOURCE_IN_FLIGHT.with(|set| {
        set.borrow_mut().remove(&key);
    });
}

async fn wait_for_resource(key: ResourceKey) -> Result<u64, String> {
    for _ in 0..40 {
        if let Some(id) = begin_resource(key) {
            return Ok(id);
        }
        TimeoutFuture::new(50).await;
    }
    Err("已有读取长时间未完成".to_owned())
}

fn maybe_expire_auth(state: &UseReducerHandle<AppState>, epoch: u64, error: &str) {
    if error.contains("HTTP 401") {
        state.dispatch(Action::AuthenticationExpired(
            epoch,
            "登录已失效，请重新登录后继续当前页面".to_owned(),
        ));
    }
}

pub(crate) fn dispatch_status_refresh(state: UseReducerHandle<AppState>) {
    let Some(id) = begin_resource(ResourceKey::Status) else {
        return;
    };
    state.dispatch(Action::StatusStarted(id));
    spawn_local(async move {
        let result = fetch_json::<StatusSnapshot>(STATUS_ENDPOINT, "状态").await;
        state.dispatch(Action::StatusFinished(id, result));
        finish_resource(ResourceKey::Status);
    });
}

pub(crate) fn dispatch_panel_refresh(state: UseReducerHandle<AppState>) {
    let Some(id) = begin_resource(ResourceKey::Panel) else {
        return;
    };
    state.dispatch(Action::PanelStarted(id));
    spawn_local(async move {
        let result = fetch_json::<PanelBootstrap>(PANEL_ENDPOINT, "控制面").await;
        state.dispatch(Action::PanelFinished(id, result));
        finish_resource(ResourceKey::Panel);
    });
}

async fn read_network_now(
    state: UseReducerHandle<AppState>,
    epoch: u64,
    wait: bool,
) -> Result<NetworkConfigDto, String> {
    let id = if wait {
        wait_for_resource(ResourceKey::Network).await?
    } else {
        begin_resource(ResourceKey::Network).ok_or_else(|| "读取已在进行".to_owned())?
    };
    state.dispatch(Action::NetworkStarted(id, epoch));
    let result = fetch_json::<NetworkConfigResponseDto>(NETWORK_CONFIG_ENDPOINT, "网络配置")
        .await
        .map(|response| response.config);
    if let Err(error) = &result {
        maybe_expire_auth(&state, epoch, error);
    }
    state.dispatch(Action::NetworkFinished(id, epoch, result.clone()));
    finish_resource(ResourceKey::Network);
    result
}

pub(crate) fn dispatch_network_refresh(state: UseReducerHandle<AppState>) {
    let epoch = state.auth_epoch;
    spawn_local(async move {
        let _ = read_network_now(state, epoch, false).await;
    });
}

async fn read_pending_now(
    state: UseReducerHandle<AppState>,
    epoch: u64,
    wait: bool,
) -> Result<NetworkPendingDto, String> {
    let id = if wait {
        wait_for_resource(ResourceKey::Pending).await?
    } else {
        begin_resource(ResourceKey::Pending).ok_or_else(|| "读取已在进行".to_owned())?
    };
    state.dispatch(Action::PendingStarted(id, epoch));
    let result = fetch_json::<NetworkPendingDto>(NETWORK_PENDING_ENDPOINT, "待确认网络配置").await;
    if let Err(error) = &result {
        maybe_expire_auth(&state, epoch, error);
    }
    state.dispatch(Action::PendingFinished(id, epoch, result.clone()));
    finish_resource(ResourceKey::Pending);
    result
}

pub(crate) fn dispatch_pending_refresh(state: UseReducerHandle<AppState>) {
    let epoch = state.auth_epoch;
    spawn_local(async move {
        let _ = read_pending_now(state, epoch, false).await;
    });
}

async fn read_subscription_now(
    state: UseReducerHandle<AppState>,
    epoch: u64,
    wait: bool,
) -> Result<SubscriptionDto, String> {
    let id = if wait {
        wait_for_resource(ResourceKey::Subscription).await?
    } else {
        begin_resource(ResourceKey::Subscription).ok_or_else(|| "读取已在进行".to_owned())?
    };
    state.dispatch(Action::SubscriptionStarted(id, epoch));
    let result = fetch_json::<SubscriptionResponseDto>(SUBSCRIPTION_ENDPOINT, "订阅状态")
        .await
        .map(|response| response.subscription);
    if let Err(error) = &result {
        maybe_expire_auth(&state, epoch, error);
    }
    state.dispatch(Action::SubscriptionFinished(id, epoch, result.clone()));
    finish_resource(ResourceKey::Subscription);
    result
}

pub(crate) fn dispatch_subscription_refresh_read(state: UseReducerHandle<AppState>) {
    let epoch = state.auth_epoch;
    spawn_local(async move {
        let _ = read_subscription_now(state, epoch, false).await;
    });
}

async fn read_devices_now(
    state: UseReducerHandle<AppState>,
    epoch: u64,
    wait: bool,
) -> Result<DevicePolicySnapshotDto, String> {
    let id = if wait {
        wait_for_resource(ResourceKey::Devices).await?
    } else {
        begin_resource(ResourceKey::Devices).ok_or_else(|| "读取已在进行".to_owned())?
    };
    state.dispatch(Action::DevicesStarted(id, epoch));
    let result = fetch_json::<DevicePolicySnapshotDto>(DEVICE_POLICIES_ENDPOINT, "设备状态").await;
    if let Err(error) = &result {
        maybe_expire_auth(&state, epoch, error);
    }
    state.dispatch(Action::DevicesFinished(id, epoch, result.clone()));
    finish_resource(ResourceKey::Devices);
    result
}

pub(crate) fn dispatch_devices_refresh(state: UseReducerHandle<AppState>) {
    let epoch = state.auth_epoch;
    spawn_local(async move {
        let _ = read_devices_now(state, epoch, false).await;
    });
}

async fn read_tailscale_now(
    state: UseReducerHandle<AppState>,
    epoch: u64,
    wait: bool,
) -> Result<TailscaleStatus, String> {
    let id = if wait {
        wait_for_resource(ResourceKey::Tailscale).await?
    } else {
        begin_resource(ResourceKey::Tailscale).ok_or_else(|| "读取已在进行".to_owned())?
    };
    state.dispatch(Action::TailscaleStarted(id, epoch));
    let result = fetch_json::<TailscaleResponseDto>(TAILSCALE_ENDPOINT, "Tailscale 状态")
        .await
        .map(|response| response.tailscale);
    if let Err(error) = &result {
        maybe_expire_auth(&state, epoch, error);
    }
    state.dispatch(Action::TailscaleFinished(id, epoch, result.clone()));
    finish_resource(ResourceKey::Tailscale);
    result
}

pub(crate) fn dispatch_tailscale_refresh(state: UseReducerHandle<AppState>) {
    let epoch = state.auth_epoch;
    spawn_local(async move {
        let _ = read_tailscale_now(state, epoch, false).await;
    });
}

async fn read_tailscale_peers_now(
    state: UseReducerHandle<AppState>,
    epoch: u64,
    wait: bool,
) -> Result<TailscalePeerSnapshot, String> {
    let id = if wait {
        wait_for_resource(ResourceKey::TailscalePeers).await?
    } else {
        begin_resource(ResourceKey::TailscalePeers).ok_or_else(|| "读取已在进行".to_owned())?
    };
    state.dispatch(Action::TailscalePeersStarted(id, epoch));
    let result =
        fetch_json::<TailscalePeersResponseDto>(TAILSCALE_PEERS_ENDPOINT, "Tailscale 设备列表")
            .await
            .map(|response| response.peers);
    if let Err(error) = &result {
        maybe_expire_auth(&state, epoch, error);
    }
    state.dispatch(Action::TailscalePeersFinished(id, epoch, result.clone()));
    finish_resource(ResourceKey::TailscalePeers);
    result
}

pub(crate) fn dispatch_tailscale_peers_refresh(state: UseReducerHandle<AppState>) {
    let epoch = state.auth_epoch;
    spawn_local(async move {
        let _ = read_tailscale_peers_now(state, epoch, false).await;
    });
}

pub(crate) fn dispatch_apps_refresh(state: UseReducerHandle<AppState>) {
    let Some(id) = begin_resource(ResourceKey::Apps) else {
        return;
    };
    state.dispatch(Action::AppsStarted(id));
    spawn_local(async move {
        let result = fetch_json::<AppsResponseDto>(APPS_ENDPOINT, "已部署应用")
            .await
            .map(|response| response.apps);
        state.dispatch(Action::AppsFinished(id, result));
        finish_resource(ResourceKey::Apps);
    });
}

pub(crate) fn dispatch_auth<T: serde::Serialize + 'static>(
    state: UseReducerHandle<AppState>,
    endpoint: &'static str,
    csrf: String,
    body: T,
    success: &'static str,
) {
    state.dispatch(Action::AuthStarted);
    spawn_local(async move {
        let result = post_json_response::<_, AuthSessionDto>(endpoint, &csrf, &body, "认证")
            .await
            .map(|session| (session, success.to_owned()));
        state.dispatch(Action::AuthFinished(result));
    });
}

pub(crate) fn dispatch_scan(state: UseReducerHandle<AppState>, csrf: String) {
    state.dispatch(Action::SettingsStarted);
    let epoch = state.auth_epoch;
    spawn_local(async move {
        let result = post_json_response::<_, NetworkScanResponseDto>(
            STA_SCAN_ENDPOINT,
            &csrf,
            &EmptyRequest {},
            "STA 扫描",
        )
        .await
        .map(|response| response.entries);
        if let Err(error) = &result {
            maybe_expire_auth(&state, epoch, error);
        }
        state.dispatch(Action::ScanFinished(result));
    });
}

pub(crate) fn dispatch_network_mutation<T: serde::Serialize + 'static>(
    state: UseReducerHandle<AppState>,
    endpoint: &'static str,
    csrf: String,
    body: T,
    label: &'static str,
    success: &'static str,
    paint_delay: bool,
) {
    state.dispatch(Action::SettingsStarted);
    let epoch = state.auth_epoch;
    spawn_local(async move {
        if paint_delay {
            TimeoutFuture::new(NETWORK_APPLY_PAINT_DELAY_MS).await;
        }
        let result = post_json(endpoint, &csrf, &body, label).await;
        let message = match result {
            Ok(_) => {
                let mut readback_error = None;
                let refresh_network = matches!(
                    endpoint,
                    STA_APPLY_ENDPOINT | AP_CONFIRM_ENDPOINT | AP_CANCEL_ENDPOINT
                );
                if refresh_network {
                    if let Err(error) = read_network_now(state.clone(), epoch, true).await {
                        readback_error = Some(error);
                    }
                }
                if let Err(error) = read_pending_now(state.clone(), epoch, true).await {
                    readback_error.get_or_insert(error);
                }
                if readback_error.is_some() {
                    Ok(format!("{success}；状态刷新失败"))
                } else {
                    Ok(success.to_owned())
                }
            }
            Err(error) => {
                maybe_expire_auth(&state, epoch, &error);
                Err(error)
            }
        };
        state.dispatch(Action::SettingsMutationFinished(message));
    });
}

pub(crate) fn dispatch_subscription_source(
    state: UseReducerHandle<AppState>,
    csrf: String,
    url: String,
) {
    state.dispatch(Action::SubscriptionMutationStarted);
    let epoch = state.auth_epoch;
    spawn_local(async move {
        let result = post_json(
            SUBSCRIPTION_SOURCE_ENDPOINT,
            &csrf,
            &SubscriptionSourceRequest { url },
            "订阅来源",
        )
        .await;
        let message = match result {
            Ok(_) => match read_subscription_now(state.clone(), epoch, true).await {
                Ok(_) => "订阅来源已保存并更新".to_owned(),
                Err(_) => "订阅来源已保存，状态刷新失败".to_owned(),
            },
            Err(error) => {
                maybe_expire_auth(&state, epoch, &error);
                format!("操作失败：{error}")
            }
        };
        state.dispatch(Action::SubscriptionMutationFinished(message));
    });
}

pub(crate) fn dispatch_subscription_refresh(state: UseReducerHandle<AppState>, csrf: String) {
    state.dispatch(Action::SubscriptionMutationStarted);
    let epoch = state.auth_epoch;
    spawn_local(async move {
        let result = post_json(
            SUBSCRIPTION_REFRESH_ENDPOINT,
            &csrf,
            &EmptyRequest {},
            "订阅更新",
        )
        .await;
        let message = match result {
            Ok(_) => match read_subscription_now(state.clone(), epoch, true).await {
                Ok(_) => "订阅已更新".to_owned(),
                Err(_) => "订阅已更新，状态刷新失败".to_owned(),
            },
            Err(error) => {
                maybe_expire_auth(&state, epoch, &error);
                format!("操作失败：{error}")
            }
        };
        state.dispatch(Action::SubscriptionMutationFinished(message));
    });
}

pub(crate) fn dispatch_device_policy_update(
    state: UseReducerHandle<AppState>,
    csrf: String,
    generation: u64,
    entries: Vec<DevicePolicyEntryDto>,
) {
    state.dispatch(Action::DeviceMutationStarted);
    let epoch = state.auth_epoch;
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
        let message = match result {
            Ok(_) => match read_devices_now(state.clone(), epoch, true).await {
                Ok(snapshot) if !snapshot.effective => "设备策略已保存，等待启用 TUN".to_owned(),
                Ok(_) => "设备策略已保存，生效状态待确认".to_owned(),
                Err(_) => "设备策略已保存，状态刷新失败".to_owned(),
            },
            Err(error) if error.contains("HTTP 409") => {
                let _ = read_devices_now(state.clone(), epoch, true).await;
                "设备策略更新未完成（HTTP 409）；已刷新服务器状态，请核对后重试".to_owned()
            }
            Err(error) => {
                maybe_expire_auth(&state, epoch, &error);
                format!("操作失败：{error}")
            }
        };
        state.dispatch(Action::DeviceMutationFinished(message));
    });
}

pub(crate) fn dispatch_tailscale_mutation<T: serde::Serialize + 'static>(
    state: UseReducerHandle<AppState>,
    endpoint: &'static str,
    csrf: String,
    body: T,
    success: &'static str,
) {
    state.dispatch(Action::TailscaleMutationStarted);
    let epoch = state.auth_epoch;
    spawn_local(async move {
        let result = post_json_response::<_, TailscaleMutationResponseDto>(
            endpoint,
            &csrf,
            &body,
            "Tailscale",
        )
        .await
        .map(|response| (response.tailscale, response.login_url, success.to_owned()));
        if let Err(error) = &result {
            maybe_expire_auth(&state, epoch, error);
        }
        let refresh_peers = result.is_ok() && endpoint != TAILSCALE_LOGOUT_ENDPOINT;
        state.dispatch(Action::TailscaleMutationFinished(result));
        if refresh_peers {
            let _ = read_tailscale_peers_now(state.clone(), epoch, true).await;
        }
    });
}

pub(crate) fn dispatch_control<T>(
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
        let result = post_json(endpoint, &csrf_token, &body, "控制")
            .await
            .map(|_| success);
        let ok = result.is_ok();
        state.dispatch(Action::ControlFinished(area, result));
        if ok {
            match area {
                ControlArea::Display => dispatch_panel_refresh(state.clone()),
                ControlArea::LanTun | ControlArea::LocalSystemProxy => {
                    dispatch_status_refresh(state.clone());
                    dispatch_devices_refresh(state.clone());
                }
                ControlArea::Nodes => {}
            }
        }
    });
}

pub(crate) fn dispatch_delay_refresh(state: UseReducerHandle<AppState>, csrf_token: String) {
    state.dispatch(Action::ControlStarted(ControlArea::Nodes));
    spawn_local(async move {
        let result = post_json_response::<_, DelayRefreshControlResponse>(
            PROXY_DELAYS_ENDPOINT,
            &csrf_token,
            &ProxyDelayRefreshRequest {},
            "代理测速",
        )
        .await
        .and_then(|response| match response {
            DelayRefreshControlResponse::ProxyDelays { groups } => Ok(groups),
        });
        state.dispatch(Action::ProxyDelaysFinished(result));
    });
}

pub(crate) fn reveal_and_focus_after_render(reveal_node: NodeRef, focus_node: NodeRef) {
    spawn_local(async move {
        TimeoutFuture::new(0).await;
        if let Some(element) = reveal_node.cast::<HtmlElement>() {
            element.scroll_into_view();
        }
        if let Some(element) = focus_node.cast::<HtmlElement>() {
            let _ = element.focus();
        }
    });
}
