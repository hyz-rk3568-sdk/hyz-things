use hyz_router::{
    adapters::{
        inbound::{
            control::{
                acquire_daemon_ownership, bind_control_socket, remove_control_socket, request,
                require_root, serve_control, ControlHandler, ControlOperation, ControlProxyMode,
                ControlResult,
            },
            dhcp_hook,
            http::{
                app_with_admin_control, app_with_admin_control_at_address,
                bind_fixed_lan_with_retry, DEFAULT_BIND_ATTEMPTS, DEFAULT_HTTP_PORT,
            },
            ota_cli::{parse_ota_cli, OtaCommand, OTA_USAGE},
        },
        outbound::{
            admin::AdminFileAdapter,
            firmware::FirmwareAdapter,
            subscription::{
                SubscriptionStore, SystemSubscriptionResolver, UreqSubscriptionTransport,
            },
            LinuxMihomoFailOpenPlatform, LinuxRouterPlatform, LinuxTailscalePlatform,
        },
    },
    application::{
        admin::{AdminApplication, AdminError},
        device_policy::DevicePolicyApplication,
        dhcp::{DhcpApplication, DhcpEvent, DhcpPlatformPort},
        fail_open::{MihomoFailOpenApplication, WatcherInvocation},
        ota::{InstallMode, OtaService},
        panel::PanelApplication,
        ports::{
            ClockPort, DevicePolicyStorePort, LifecycleLease, PlatformError, SystemProbePort,
            TailnetPeerReadPort, TailscalePlatformPort, TailscaleProbePort,
        },
        proxy::{
            MihomoDirectRecoveryApplication, MihomoDirectRecoveryResult, ProxyFeatureCoordinator,
        },
        router::RouterApplication,
        shutdown::ShutdownApplication,
        status::{
            tailscale_status_component_from_observed, ReadStatus, StatusTailscalePlatformPort,
        },
        subscription::{SubscriptionApplication, SubscriptionRuntimePorts},
        tailscale::{ReadTailnetPeers, TailscaleApplication, TailscaleReconcileState},
        wifi::{WifiApplication, AP_CONFIRM_TIMEOUT_SECS},
    },
    domain::{
        network::{NetworkDesired, OwnedResource, Probe},
        proxy::ProxyDesired,
        status::{Component, Issue, TailscaleStatus},
        tailscale::{
            TailscaleAction, TailscaleDesired, TailscaleLoginUrl, TailscaleMode, TailscaleObserved,
            TAILSCALE_MANAGEMENT_HTTP_PORT,
        },
    },
};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    error::Error,
    net::{Ipv4Addr, SocketAddr},
    path::Path,
    sync::{mpsc as std_mpsc, Arc, Mutex as StdMutex, Weak},
    time::{Duration, Instant},
};
use tokio::{
    sync::{mpsc, oneshot, watch, Mutex as AsyncMutex},
    time::{timeout_at, Instant as TokioInstant},
};

const DAEMON_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const MIHOMO_DIRECT_RECOVERY_INTERVAL: Duration = Duration::from_secs(2);

fn arm_shutdown_deadline(deadline: TokioInstant) {
    std::thread::spawn(move || {
        std::thread::sleep(deadline.saturating_duration_since(TokioInstant::now()));
        eprintln!(
            "hyz-router: daemon shutdown deadline expired; forcing process exit with ownership evidence retained"
        );
        std::process::exit(1);
    });
}

const USAGE: &str = "Usage:\n  hyz-router daemon\n  hyz-router status [--json]\n  hyz-router router enable|disable\n  hyz-router proxy lan-tun enable|disable\n  hyz-router proxy tailscale enable|disable\n  hyz-router wifi status|scan\n  hyz-router wifi ap apply|confirm|cancel\n  hyz-router subscription get [--json]\n  hyz-router subscription refresh\n  hyz-router ota ...";

#[derive(Clone)]
struct ProductionTailscalePlatform {
    linux: LinuxTailscalePlatform,
    listener: Arc<StdMutex<Option<RunningTailscaleListener>>>,
    http: Arc<StdMutex<Option<TailscaleHttpConfig>>>,
    router: Arc<LinuxRouterPlatform>,
}

struct TailscaleHttpConfig {
    runtime: tokio::runtime::Handle,
    owner: Weak<ProductionRuntime>,
    csrf_token: String,
    port: u16,
}

const TAILSCALE_ACCEPT_STOP_TIMEOUT: Duration = Duration::from_secs(1);
const TAILSCALE_GRACEFUL_JOIN_TIMEOUT: Duration = Duration::from_millis(250);
const TAILSCALE_ABORT_REAP_TIMEOUT: Duration = Duration::from_secs(1);

struct RunningTailscaleListener {
    token: String,
    ipv4: Ipv4Addr,
    runtime: tokio::runtime::Handle,
    shutdown: Option<oneshot::Sender<()>>,
    accept_stopped: std_mpsc::Receiver<()>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

fn bind_exact_tailscale_listener(address: SocketAddr) -> std::io::Result<std::net::TcpListener> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&address.into())?;
    socket.listen(128)?;
    Ok(socket.into())
}

struct ConfirmedTailscaleListener {
    listener: tokio::net::TcpListener,
    accept_stopped: Option<std_mpsc::Sender<()>>,
}

impl Drop for ConfirmedTailscaleListener {
    fn drop(&mut self) {
        if let Some(accept_stopped) = self.accept_stopped.take() {
            let _ = accept_stopped.send(());
        }
    }
}

impl axum::serve::Listener for ConfirmedTailscaleListener {
    type Io = tokio::net::TcpStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        axum::serve::Listener::accept(&mut self.listener).await
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

impl RunningTailscaleListener {
    fn wait_for_task(
        &mut self,
        timeout: Duration,
    ) -> Option<Result<std::io::Result<()>, tokio::task::JoinError>> {
        let runtime = self.runtime.clone();
        runtime.block_on(async { tokio::time::timeout(timeout, &mut self.task).await.ok() })
    }

    fn log_completion(result: Result<std::io::Result<()>, tokio::task::JoinError>) {
        match result {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                eprintln!("hyz-router: Tailscale management listener failed: {error}");
            }
            Err(error) if error.is_cancelled() => {}
            Err(error) => {
                eprintln!("hyz-router: Tailscale management listener task failed: {error}");
            }
        }
    }

    fn reap_finished(mut self) {
        let runtime = self.runtime.clone();
        Self::log_completion(runtime.block_on(&mut self.task));
    }
}

impl ProductionTailscalePlatform {
    fn new(router: Arc<LinuxRouterPlatform>) -> Self {
        Self {
            linux: LinuxTailscalePlatform::new(),
            listener: Arc::new(StdMutex::new(None)),
            http: Arc::new(StdMutex::new(None)),
            router,
        }
    }

    fn attach_http(
        &self,
        owner: Weak<ProductionRuntime>,
        csrf_token: String,
        port: u16,
        runtime: tokio::runtime::Handle,
    ) -> Result<(), PlatformError> {
        let mut config = self.http.lock().map_err(|_| {
            PlatformError::InvalidState("Tailscale HTTP config lock poisoned".to_owned())
        })?;
        if config.is_some() {
            return Err(PlatformError::Conflict(
                "Tailscale HTTP composition is already attached".to_owned(),
            ));
        }
        *config = Some(TailscaleHttpConfig {
            runtime,
            owner,
            csrf_token,
            port,
        });
        Ok(())
    }

    fn start_listener(&self, token: &str, ipv4: Ipv4Addr) -> Result<(), PlatformError> {
        let mut state = self.listener.lock().map_err(|_| {
            PlatformError::InvalidState("Tailscale listener lock poisoned".to_owned())
        })?;
        if state.is_some() {
            return Err(PlatformError::Conflict(
                "Tailscale management listener already exists".to_owned(),
            ));
        }
        let config = self.http.lock().map_err(|_| {
            PlatformError::InvalidState("Tailscale HTTP config lock poisoned".to_owned())
        })?;
        let config = config.as_ref().ok_or_else(|| {
            PlatformError::InvalidState("Tailscale HTTP composition is not attached".to_owned())
        })?;
        if config.port != TAILSCALE_MANAGEMENT_HTTP_PORT {
            return Err(PlatformError::InvalidState(format!(
                "Tailscale management listener requires fixed HTTP port {TAILSCALE_MANAGEMENT_HTTP_PORT}"
            )));
        }
        let owner = config.owner.upgrade().ok_or_else(|| {
            PlatformError::InvalidState("Tailscale HTTP runtime owner is unavailable".to_owned())
        })?;
        let address = SocketAddr::from((ipv4, config.port));
        let listener = bind_exact_tailscale_listener(address).map_err(|error| {
            PlatformError::Io(format!(
                "bind exact Tailscale HTTP listener {address}: {error}"
            ))
        })?;
        let listener = {
            let _enter = config.runtime.enter();
            tokio::net::TcpListener::from_std(listener).map_err(|error| {
                PlatformError::Io(format!("adopt exact Tailscale HTTP listener: {error}"))
            })?
        };
        let (shutdown, stopped) = oneshot::channel();
        let (accept_stopped, accept_stopped_rx) = std_mpsc::channel();
        let listener = ConfirmedTailscaleListener {
            listener,
            accept_stopped: Some(accept_stopped),
        };
        let status = owner.status();
        let control: Arc<dyn ControlHandler> = owner.clone();
        let admin = owner.admin();
        let app = app_with_admin_control_at_address(
            status,
            control,
            admin,
            config.csrf_token.clone(),
            ipv4,
            config.port,
        );
        let runtime = config.runtime.clone();
        let task = runtime.spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await
        });
        *state = Some(RunningTailscaleListener {
            token: token.to_owned(),
            ipv4,
            runtime,
            shutdown: Some(shutdown),
            accept_stopped: accept_stopped_rx,
            task,
        });
        Ok(())
    }

    fn stop_listener(&self, token: &str) -> Result<(), PlatformError> {
        let mut state = self.listener.lock().map_err(|_| {
            PlatformError::InvalidState("Tailscale listener lock poisoned".to_owned())
        })?;
        let mut running = state.take().ok_or_else(|| {
            PlatformError::Conflict("Tailscale management listener is absent".to_owned())
        })?;
        if running.token != token {
            *state = Some(running);
            return Err(PlatformError::Conflict(
                "refusing to stop a Tailscale listener with a different ownership token".to_owned(),
            ));
        }
        drop(state);

        if let Some(shutdown) = running.shutdown.take() {
            let _ = shutdown.send(());
        } else {
            running.task.abort();
        }

        let accept_confirmed = running
            .accept_stopped
            .recv_timeout(TAILSCALE_ACCEPT_STOP_TIMEOUT)
            .is_ok();
        if accept_confirmed {
            if let Some(result) = running.wait_for_task(TAILSCALE_GRACEFUL_JOIN_TIMEOUT) {
                RunningTailscaleListener::log_completion(result);
                return Ok(());
            }
        }

        running.task.abort();
        if let Some(result) = running.wait_for_task(TAILSCALE_ABORT_REAP_TIMEOUT) {
            RunningTailscaleListener::log_completion(result);
            let accept_confirmed = accept_confirmed || running.accept_stopped.try_recv().is_ok();
            return if accept_confirmed {
                Ok(())
            } else {
                Err(PlatformError::UnsafeToCutOver(
                    "Tailscale management listener stopped without accept-loop confirmation"
                        .to_owned(),
                ))
            };
        }

        let mut state = match self.listener.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        *state = Some(running);
        Err(PlatformError::UnsafeToCutOver(
            "Tailscale management listener task did not stop within the bounded abort/reap window"
                .to_owned(),
        ))
    }

    fn listener_observation(&self) -> (Probe<OwnedResource>, Probe<Option<Ipv4Addr>>) {
        let mut state = match self.listener.lock() {
            Ok(state) => state,
            Err(_) => {
                let reason = "Tailscale listener lock poisoned".to_owned();
                return (Probe::Unknown(reason.clone()), Probe::Unknown(reason));
            }
        };
        if state
            .as_ref()
            .is_some_and(|listener| listener.task.is_finished())
        {
            let listener = state.take().expect("finished listener must exist");
            drop(state);
            listener.reap_finished();
            return (Probe::Known(OwnedResource::Absent), Probe::Known(None));
        }
        match state.as_ref() {
            None => (Probe::Known(OwnedResource::Absent), Probe::Known(None)),
            Some(listener) if listener.shutdown.is_none() => {
                let reason =
                    "Tailscale management listener termination is not confirmed".to_owned();
                (Probe::Unknown(reason.clone()), Probe::Unknown(reason))
            }
            Some(listener) => (
                Probe::Known(OwnedResource::Owned {
                    token: listener.token.clone(),
                }),
                Probe::Known(Some(listener.ipv4)),
            ),
        }
    }
}

impl TailscalePlatformPort for ProductionTailscalePlatform {
    fn acquire_tailscale_lock(&self) -> Result<LifecycleLease, PlatformError> {
        self.linux.acquire_tailscale_lock()
    }

    fn release_tailscale_lock(&self, lease: &LifecycleLease) -> Result<(), PlatformError> {
        self.linux.release_tailscale_lock(lease)
    }

    fn apply_tailscale(&self, action: &TailscaleAction) -> Result<(), PlatformError> {
        match action {
            TailscaleAction::StartManagementListener { token, ipv4 } => {
                self.start_listener(token, *ipv4)
            }
            TailscaleAction::StopManagementListener { token } => self.stop_listener(token),
            _ => self.linux.apply_tailscale(action),
        }
    }

    fn request_login(&self) -> Result<TailscaleLoginUrl, PlatformError> {
        self.linux.request_login()
    }

    fn logout(&self) -> Result<(), PlatformError> {
        self.linux.logout()
    }
}

impl TailscaleProbePort for ProductionTailscalePlatform {
    fn observe_tailscale(&self) -> Result<TailscaleObserved, PlatformError> {
        let mut observed = self.linux.observe_tailscale()?;
        let (listener, listener_ipv4) = self.listener_observation();
        observed.management_listener = listener;
        observed.management_listener_ipv4 = listener_ipv4;
        Ok(observed)
    }

    fn probe_explicit_proxy_path(&self) -> Result<bool, PlatformError> {
        self.linux.probe_explicit_proxy_path()
    }
}

impl TailnetPeerReadPort for ProductionTailscalePlatform {
    fn read_tailnet_peers(
        &self,
    ) -> Result<hyz_router::domain::tailscale::TailscalePeerSnapshot, PlatformError> {
        self.linux.read_tailnet_peers()
    }
}

#[async_trait::async_trait]
impl StatusTailscalePlatformPort for ProductionTailscalePlatform {
    async fn read_tailscale_status(&self) -> Component<TailscaleStatus> {
        let tailscale = self.clone();
        tokio::task::spawn_blocking(move || {
            let observed = tailscale
                .observe_tailscale()
                .map_err(|error| error.to_string())?;
            let network = tailscale
                .router
                .observe_network()
                .map_err(|error| error.to_string())?;
            let proxy = tailscale
                .router
                .observe_proxy()
                .map_err(|error| error.to_string())?;
            let explicit_proxy_path = match (&proxy.persisted_features, &observed.environment) {
                (
                    Probe::Known(features),
                    Probe::Known(
                        hyz_router::domain::tailscale::TailscaleEnvironment::MihomoExplicit,
                    ),
                ) if features.supported() && features.tailscale_explicit_proxy_enabled => {
                    match tailscale.probe_explicit_proxy_path() {
                        Ok(ready) => Probe::Known(ready),
                        Err(error) => Probe::Unknown(error.to_string()),
                    }
                }
                _ => Probe::Known(false),
            };
            Ok::<_, String>(tailscale_status_component_from_observed(
                &observed,
                &proxy.persisted_features,
                network.ready_for(&NetworkDesired::forwarding()),
                &explicit_proxy_path,
            ))
        })
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_else(|| {
            Component::unavailable(Issue::new(
                "tailscale_probe_failed",
                "Tailscale status is unavailable",
            ))
        })
    }
}

struct DhcpDispatch {
    event: DhcpEvent,
}

enum DhcpWorkerCommand {
    Dispatch(DhcpDispatch),
    Stop,
}

struct DhcpDispatcherState {
    sender: Option<mpsc::Sender<DhcpWorkerCommand>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Clone)]
struct DhcpDispatcher {
    state: Arc<AsyncMutex<DhcpDispatcherState>>,
}

impl DhcpDispatcher {
    fn new(router: Arc<LinuxRouterPlatform>) -> Self {
        let (sender, mut receiver) = mpsc::channel::<DhcpWorkerCommand>(8);
        let task = tokio::spawn(async move {
            while let Some(command) = receiver.recv().await {
                let DhcpWorkerCommand::Dispatch(dispatch) = command else {
                    break;
                };
                let DhcpDispatch { event } = dispatch;
                let platform = router.clone();
                let result = tokio::task::spawn_blocking(move || {
                    if platform.active_dhcp_generation()?.as_ref() != Some(&event.generation) {
                        return Err(PlatformError::Conflict(
                            "DHCP callback generation is stale".to_owned(),
                        ));
                    }
                    DhcpApplication::new(platform.as_ref(), platform.as_ref()).execute(&event)
                })
                .await
                .map_err(|_| {
                    PlatformError::CommandFailed("DHCP worker terminated unexpectedly".to_owned())
                })
                .and_then(|result| result);
                if let Err(error) = result {
                    eprintln!("hyz-router: queued DHCP event failed: {error}");
                }
            }
        });
        Self {
            state: Arc::new(AsyncMutex::new(DhcpDispatcherState {
                sender: Some(sender),
                task: Some(task),
            })),
        }
    }

    async fn dispatch(&self, event: DhcpEvent) -> Result<(), PlatformError> {
        let state = self.state.lock().await;
        let sender = state
            .sender
            .as_ref()
            .ok_or_else(|| PlatformError::InvalidState("DHCP dispatcher is stopping".to_owned()))?;
        sender
            .try_send(DhcpWorkerCommand::Dispatch(DhcpDispatch { event }))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    PlatformError::Busy("DHCP dispatcher queue is full".to_owned())
                }
                mpsc::error::TrySendError::Closed(_) => {
                    PlatformError::InvalidState("DHCP dispatcher is unavailable".to_owned())
                }
            })
    }

    async fn stop_and_join(&self) -> Result<(), PlatformError> {
        let mut state = self.state.lock().await;
        let Some(sender) = state.sender.take() else {
            return Ok(());
        };
        sender.send(DhcpWorkerCommand::Stop).await.map_err(|_| {
            PlatformError::CommandFailed("DHCP dispatcher stopped unexpectedly".to_owned())
        })?;
        drop(sender);
        let task = state.task.take().ok_or_else(|| {
            PlatformError::InvalidState("DHCP dispatcher task is absent".to_owned())
        })?;
        drop(state);
        task.await.map_err(|error| {
            PlatformError::CommandFailed(format!(
                "DHCP dispatcher task terminated unexpectedly: {error}"
            ))
        })
    }
}

struct ProductionRuntime {
    router: Arc<LinuxRouterPlatform>,
    tailscale: Arc<ProductionTailscalePlatform>,
    admin: Arc<AdminApplication>,
    firmware: FirmwareAdapter,
    subscription_store: SubscriptionStore,
    subscription_transport: UreqSubscriptionTransport,
    dhcp: DhcpDispatcher,
    router_proxy: AsyncMutex<()>,
    proxy_delay_last: AsyncMutex<Option<Instant>>,
    display: AsyncMutex<()>,
    ota: AsyncMutex<()>,
}

impl ProductionRuntime {
    fn build() -> Result<Self, AdminError> {
        let router = Arc::new(LinuxRouterPlatform::new());
        let tailscale = Arc::new(ProductionTailscalePlatform::new(router.clone()));
        let dhcp = DhcpDispatcher::new(router.clone());
        let admin_adapter = Arc::new(AdminFileAdapter::default());
        let admin = Arc::new(AdminApplication::initialize(
            admin_adapter.clone(),
            admin_adapter,
            router.clone(),
        )?);
        let resolver = Arc::new(SystemSubscriptionResolver);
        Ok(Self {
            router,
            tailscale,
            admin,
            firmware: FirmwareAdapter::default(),
            subscription_store: SubscriptionStore::default(),
            subscription_transport: UreqSubscriptionTransport::new(resolver),
            dhcp,
            router_proxy: AsyncMutex::new(()),
            proxy_delay_last: AsyncMutex::new(None),
            display: AsyncMutex::new(()),
            ota: AsyncMutex::new(()),
        })
    }

    fn admin(&self) -> Arc<AdminApplication> {
        self.admin.clone()
    }

    fn status(&self) -> ReadStatus {
        ReadStatus::new_with_tailscale(
            self.router.clone(),
            self.tailscale.clone(),
            self.router.clone(),
            self.router.clone(),
        )
    }

    async fn proxy_desired(&self) -> Result<ProxyDesired, PlatformError> {
        let platform = self.router.clone();
        tokio::task::spawn_blocking(move || {
            let observed = platform.observe_proxy()?;
            let features = match observed.persisted_features {
                Probe::Known(features) if features.supported() => features,
                Probe::Known(_) => {
                    return Err(PlatformError::ProbeFailed(
                        "persisted proxy feature version is unsupported".to_owned(),
                    ))
                }
                Probe::Unknown(reason) => {
                    return Err(PlatformError::ProbeFailed(format!(
                        "persisted proxy features are unknown: {reason}"
                    )))
                }
            };
            Ok(ProxyDesired {
                lan_tun_enabled: features.lan_tun_enabled,
                tailscale_explicit_proxy_enabled: features.tailscale_explicit_proxy_enabled,
                direct_macs: platform.load_device_policy()?.direct_macs(),
            })
        })
        .await
        .map_err(|_| {
            PlatformError::CommandFailed("proxy desired probe terminated unexpectedly".to_owned())
        })?
    }

    async fn reconcile_proxy_features(
        &self,
        desired: ProxyDesired,
    ) -> Result<usize, PlatformError> {
        let router = self.router.clone();
        let tailscale = self.tailscale.clone();
        tokio::task::spawn_blocking(move || {
            ProxyFeatureCoordinator::new(
                router.as_ref(),
                router.as_ref(),
                tailscale.as_ref(),
                tailscale.as_ref(),
                router.as_ref(),
            )
            .reconcile(&desired)
            .map(|result| result.proxy_actions_applied + result.tailscale_actions_applied)
        })
        .await
        .map_err(|_| {
            PlatformError::CommandFailed("proxy feature worker terminated unexpectedly".to_owned())
        })?
    }

    async fn reconcile_proxy_runtime_preserving_features(
        &self,
        desired: ProxyDesired,
    ) -> Result<usize, PlatformError> {
        let router = self.router.clone();
        let tailscale = self.tailscale.clone();
        tokio::task::spawn_blocking(move || {
            ProxyFeatureCoordinator::new(
                router.as_ref(),
                router.as_ref(),
                tailscale.as_ref(),
                tailscale.as_ref(),
                router.as_ref(),
            )
            .reconcile_runtime_preserving_features(&desired)
            .map(|result| result.proxy_actions_applied + result.tailscale_actions_applied)
        })
        .await
        .map_err(|_| {
            PlatformError::CommandFailed(
                "proxy runtime recovery worker terminated unexpectedly".to_owned(),
            )
        })?
    }

    async fn recover_tailscale_direct_if_mihomo_unavailable(
        &self,
    ) -> Result<MihomoDirectRecoveryResult, PlatformError> {
        let router = self.router.clone();
        let tailscale = self.tailscale.clone();
        tokio::task::spawn_blocking(move || {
            MihomoDirectRecoveryApplication::new(
                router.as_ref(),
                router.as_ref(),
                tailscale.as_ref(),
                tailscale.as_ref(),
                router.as_ref(),
            )
            .recover_if_core_unavailable()
        })
        .await
        .map_err(|_| {
            PlatformError::CommandFailed(
                "Mihomo Direct recovery worker terminated unexpectedly".to_owned(),
            )
        })?
    }

    async fn tailscale_desired(&self) -> Result<TailscaleDesired, PlatformError> {
        let tailscale = self.tailscale.clone();
        let observed = tokio::task::spawn_blocking(move || tailscale.observe_tailscale())
            .await
            .map_err(|_| {
                PlatformError::CommandFailed(
                    "Tailscale desired-mode probe terminated unexpectedly".to_owned(),
                )
            })??;
        match observed.persisted_mode {
            Probe::Known(Some(mode)) => Ok(TailscaleDesired { mode }),
            Probe::Known(None) => Ok(TailscaleDesired::disabled()),
            Probe::Unknown(reason) => Err(PlatformError::ProbeFailed(format!(
                "persisted Tailscale mode is unknown: {reason}"
            ))),
        }
    }

    async fn reconcile_tailscale(
        &self,
        desired: TailscaleDesired,
    ) -> Result<(TailscaleStatus, Option<TailscaleLoginUrl>), PlatformError> {
        let tailscale = self.tailscale.clone();
        let router = self.router.clone();
        let result = tokio::task::spawn_blocking(move || {
            TailscaleApplication::new(
                tailscale.as_ref(),
                tailscale.as_ref(),
                router.as_ref(),
                router.as_ref(),
            )
            .reconcile(&desired)
        })
        .await
        .map_err(|_| {
            PlatformError::CommandFailed("Tailscale worker terminated unexpectedly".to_owned())
        })??;
        let login_url = match result.state {
            TailscaleReconcileState::Ready { .. } => None,
            TailscaleReconcileState::NeedsLogin { login_url } => Some(login_url),
        };
        let status = self.tailscale.read_tailscale_status().await;
        let status = status.data.ok_or_else(|| {
            PlatformError::ProbeFailed("Tailscale status is unavailable after reconcile".to_owned())
        })?;
        Ok((status, login_url))
    }

    async fn reconcile_persisted_tailscale(
        &self,
    ) -> Result<(TailscaleStatus, Option<TailscaleLoginUrl>), PlatformError> {
        let desired = self.tailscale_desired().await?;
        self.reconcile_tailscale(desired).await
    }

    async fn shutdown_tailscale(&self) -> Result<(), PlatformError> {
        let tailscale = self.tailscale.clone();
        let router = self.router.clone();
        tokio::task::spawn_blocking(move || {
            TailscaleApplication::new(
                tailscale.as_ref(),
                tailscale.as_ref(),
                router.as_ref(),
                router.as_ref(),
            )
            .shutdown()
            .map(|_| ())
        })
        .await
        .map_err(|_| {
            PlatformError::CommandFailed(
                "Tailscale shutdown worker terminated unexpectedly".to_owned(),
            )
        })?
    }

    async fn degrade_tailscale_to_router_only(&self) -> Result<(), PlatformError> {
        let tailscale = self.tailscale.clone();
        let router = self.router.clone();
        tokio::task::spawn_blocking(move || {
            TailscaleApplication::new(
                tailscale.as_ref(),
                tailscale.as_ref(),
                router.as_ref(),
                router.as_ref(),
            )
            .degrade_to_router_only()
            .map(|_| ())
        })
        .await
        .map_err(|_| {
            PlatformError::CommandFailed(
                "Tailscale degradation worker terminated unexpectedly".to_owned(),
            )
        })?
    }

    async fn logout_tailscale(&self) -> Result<TailscaleStatus, PlatformError> {
        let tailscale = self.tailscale.clone();
        let router = self.router.clone();
        tokio::task::spawn_blocking(move || {
            TailscaleApplication::new(
                tailscale.as_ref(),
                tailscale.as_ref(),
                router.as_ref(),
                router.as_ref(),
            )
            .logout()
        })
        .await
        .map_err(|_| {
            PlatformError::CommandFailed(
                "Tailscale logout worker terminated unexpectedly".to_owned(),
            )
        })??;
        self.tailscale
            .read_tailscale_status()
            .await
            .data
            .ok_or_else(|| {
                PlatformError::ProbeFailed(
                    "Tailscale status is unavailable after logout".to_owned(),
                )
            })
    }

    fn web_token(&self) -> Result<String, PlatformError> {
        self.router.ownership_token("hyz-web")
    }

    async fn reconcile_network(&self, desired: NetworkDesired) -> Result<(), PlatformError> {
        let platform = self.router.clone();
        tokio::task::spawn_blocking(move || {
            RouterApplication::new(platform.as_ref(), platform.as_ref(), platform.as_ref())
                .reconcile(&desired)
                .map(|_| ())
        })
        .await
        .map_err(|_| {
            PlatformError::CommandFailed("router worker terminated unexpectedly".to_owned())
        })?
    }

    async fn initialize(&self) -> Result<(), String> {
        // DHCP bypasses the async router/proxy guard, but its bounded worker shares the lifecycle
        // lock. Startup commits only strict management readiness; WAN-dependent restoration runs
        // afterward so an absent DHCP lease cannot delay the LAN HTTP boundary.
        let _serial = self.router_proxy.lock().await;
        WifiApplication::new(self.router.as_ref())
            .recover()
            .map_err(|error| format!("recover interrupted AP transaction: {error}"))?;
        self.reconcile_network(NetworkDesired::management_only())
            .await
            .map_err(|error| {
                format!(
                    "startup management reconciliation failed and network safety is unconfirmed: {error}"
                )
            })?;
        self.recover_device_policy().await?;
        Ok(())
    }

    async fn wan_route_ready(&self) -> Result<bool, PlatformError> {
        let platform = self.router.clone();
        let observed = tokio::task::spawn_blocking(move || platform.observe_network())
            .await
            .map_err(|_| {
                PlatformError::CommandFailed(
                    "deferred WAN readiness probe terminated unexpectedly".to_owned(),
                )
            })??;
        match observed.wan_default_route_present {
            Probe::Known(ready) => Ok(ready),
            Probe::Unknown(reason) => Err(PlatformError::ProbeFailed(format!(
                "deferred WAN route readiness is unknown: {reason}"
            ))),
        }
    }

    async fn restore_persisted_runtime_if_wan_ready(&self) -> Result<bool, PlatformError> {
        if !self.wan_route_ready().await? {
            return Ok(false);
        }

        self.reconcile_network(NetworkDesired::forwarding()).await?;
        if let Err(error) = self.reconcile_persisted_tailscale().await {
            match self.shutdown_tailscale().await {
                Ok(()) => eprintln!(
                    "hyz-router: deferred Tailscale restoration is unavailable ({error}); exact Tailscale runtime was cleaned"
                ),
                Err(cleanup_error) => {
                    return Err(PlatformError::InvalidState(format!(
                        "deferred Tailscale restoration failed ({error}); exact cleanup also failed: {cleanup_error}"
                    )))
                }
            }
        }
        let desired = self.proxy_desired().await?;
        self.reconcile_proxy_features(desired).await?;
        Ok(true)
    }

    async fn recover_device_policy(&self) -> Result<(), String> {
        let platform = self.router.clone();
        tokio::task::spawn_blocking(move || {
            DevicePolicyApplication::new(
                platform.as_ref(),
                platform.as_ref(),
                platform.as_ref(),
                platform.as_ref(),
                platform.as_ref(),
            )
            .recover()
        })
        .await
        .map_err(|_| "startup device-policy recovery worker terminated unexpectedly".to_owned())?
        .map_err(|error| format!("recover interrupted device-policy transaction: {error}"))
    }

    async fn expire_pending_ap(&self) -> Result<bool, String> {
        let _serial = self.router_proxy.lock().await;
        let now = self.router.unix_time_millis();
        let platform = self.router.clone();
        tokio::task::spawn_blocking(move || WifiApplication::new(platform.as_ref()).expire_ap(now))
            .await
            .map_err(|_| "Wi-Fi timeout worker terminated unexpectedly".to_owned())?
            .map_err(|error| error.to_string())
    }

    async fn shutdown(&self) -> Result<(), String> {
        let _serial = self.router_proxy.lock().await;
        self.dhcp
            .stop_and_join()
            .await
            .map_err(|error| format!("DHCP dispatcher shutdown failed: {error}"))?;
        self.shutdown_tailscale()
            .await
            .map_err(|error| format!("Tailscale-first shutdown failed: {error}"))?;
        let platform = self.router.clone();
        tokio::task::spawn_blocking(move || {
            ShutdownApplication::new(platform.as_ref(), platform.as_ref())
                .execute()
                .map(|_| ())
        })
        .await
        .map_err(|_| "shutdown worker terminated unexpectedly".to_owned())?
        .map_err(|error| error.to_string())
    }
}

#[async_trait::async_trait]
impl ControlHandler for ProductionRuntime {
    async fn handle(&self, operation: ControlOperation) -> Result<ControlResult, String> {
        match operation {
            ControlOperation::Status { .. } => Ok(ControlResult::Status {
                snapshot: Box::new(self.status().execute().await),
            }),
            ControlOperation::PanelStatus { .. } => {
                let platform = self.router.clone();
                let snapshot = tokio::task::spawn_blocking(move || {
                    PanelApplication::new(platform.as_ref()).snapshot()
                })
                .await
                .map_err(|_| "panel status worker terminated unexpectedly".to_owned())?;
                Ok(ControlResult::PanelStatus {
                    snapshot: Box::new(snapshot),
                })
            }
            ControlOperation::Display { request } => {
                let _serial = self.display.lock().await;
                let platform = self.router.clone();
                let observed = tokio::task::spawn_blocking(move || {
                    PanelApplication::new(platform.as_ref()).set_display(&request)
                })
                .await
                .map_err(|_| "display worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(completed(format!(
                    "display reconciled; enabled={}; brightness={}",
                    observed.enabled, observed.actual_brightness
                )))
            }
            ControlOperation::ProxySelection { request } => {
                let _serial = self.router_proxy.lock().await;
                let platform = self.router.clone();
                tokio::task::spawn_blocking(move || {
                    PanelApplication::new(platform.as_ref()).select_proxy(&request)
                })
                .await
                .map_err(|_| "proxy selection worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(completed("proxy selection applied"))
            }
            ControlOperation::ProxyDelay { request } => {
                let _serial = self.router_proxy.lock().await;
                let mut last = self.proxy_delay_last.lock().await;
                if last.is_some_and(|last| last.elapsed() < Duration::from_secs(5)) {
                    return Err("proxy delay tests are rate limited".to_owned());
                }
                *last = Some(Instant::now());
                drop(last);
                let platform = self.router.clone();
                let result = tokio::task::spawn_blocking(move || {
                    PanelApplication::new(platform.as_ref()).measure_proxy_delay(&request.proxy)
                })
                .await
                .map_err(|_| "proxy delay worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::ProxyDelay { result })
            }
            ControlOperation::ProxyDelayRefresh { .. } => {
                let _serial = self.router_proxy.lock().await;
                let mut last = self.proxy_delay_last.lock().await;
                let should_refresh =
                    !last.is_some_and(|last| last.elapsed() < Duration::from_secs(5));
                if should_refresh {
                    *last = Some(Instant::now());
                }
                drop(last);
                let platform = self.router.clone();
                let groups = tokio::task::spawn_blocking(move || {
                    let panel = PanelApplication::new(platform.as_ref());
                    if should_refresh {
                        panel.refresh_proxy_delays()
                    } else {
                        panel.proxy_groups()
                    }
                })
                .await
                .map_err(|_| "proxy delay refresh worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::ProxyDelays { groups })
            }
            ControlOperation::DevicePoliciesGet { .. } => {
                let platform = self.router.clone();
                let snapshot = tokio::task::spawn_blocking(move || {
                    DevicePolicyApplication::new(
                        platform.as_ref(),
                        platform.as_ref(),
                        platform.as_ref(),
                        platform.as_ref(),
                        platform.as_ref(),
                    )
                    .snapshot()
                })
                .await
                .map_err(|_| "device-policy discovery worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::DevicePolicies { snapshot })
            }
            ControlOperation::DevicePoliciesSet { request } => {
                let _serial = self.router_proxy.lock().await;
                let platform = self.router.clone();
                let snapshot = tokio::task::spawn_blocking(move || {
                    DevicePolicyApplication::new(
                        platform.as_ref(),
                        platform.as_ref(),
                        platform.as_ref(),
                        platform.as_ref(),
                        platform.as_ref(),
                    )
                    .update(request)
                })
                .await
                .map_err(|_| "device-policy worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::DevicePolicies { snapshot })
            }
            ControlOperation::SubscriptionGet { .. } => {
                let store = self.subscription_store.clone();
                let transport = self.subscription_transport.clone();
                let platform = self.router.clone();
                let tailscale = self.tailscale.clone();
                let summary = tokio::task::spawn_blocking(move || {
                    SubscriptionApplication::new(
                        &store,
                        &transport,
                        platform.as_ref(),
                        SubscriptionRuntimePorts {
                            platform: platform.as_ref(),
                            probe: platform.as_ref(),
                            tailscale: tailscale.as_ref(),
                            tailscale_probe: tailscale.as_ref(),
                            clock: platform.as_ref(),
                        },
                    )
                    .summary()
                })
                .await
                .map_err(|_| "subscription summary worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::Subscription { summary })
            }
            ControlOperation::SubscriptionSet { url } => {
                let _serial = self.router_proxy.lock().await;
                let store = self.subscription_store.clone();
                let transport = self.subscription_transport.clone();
                let platform = self.router.clone();
                let tailscale = self.tailscale.clone();
                let summary = tokio::task::spawn_blocking(move || {
                    let subscription = SubscriptionApplication::new(
                        &store,
                        &transport,
                        platform.as_ref(),
                        SubscriptionRuntimePorts {
                            platform: platform.as_ref(),
                            probe: platform.as_ref(),
                            tailscale: tailscale.as_ref(),
                            tailscale_probe: tailscale.as_ref(),
                            clock: platform.as_ref(),
                        },
                    );
                    subscription.set_url(url.expose().to_owned())?;
                    subscription.refresh()
                })
                .await
                .map_err(|_| "subscription URL worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::Subscription { summary })
            }
            ControlOperation::SubscriptionRefresh { .. } => {
                let _serial = self.router_proxy.lock().await;
                let store = self.subscription_store.clone();
                let transport = self.subscription_transport.clone();
                let platform = self.router.clone();
                let tailscale = self.tailscale.clone();
                let summary = tokio::task::spawn_blocking(move || {
                    SubscriptionApplication::new(
                        &store,
                        &transport,
                        platform.as_ref(),
                        SubscriptionRuntimePorts {
                            platform: platform.as_ref(),
                            probe: platform.as_ref(),
                            tailscale: tailscale.as_ref(),
                            tailscale_probe: tailscale.as_ref(),
                            clock: platform.as_ref(),
                        },
                    )
                    .refresh()
                })
                .await
                .map_err(|_| "subscription refresh worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::Subscription { summary })
            }
            ControlOperation::TailscaleGet { .. } => {
                let status = self
                    .tailscale
                    .read_tailscale_status()
                    .await
                    .data
                    .ok_or_else(|| "Tailscale status is unavailable".to_owned())?;
                Ok(ControlResult::Tailscale { status })
            }
            ControlOperation::TailscalePeersGet { .. } => {
                let tailscale = self.tailscale.clone();
                let snapshot = tokio::task::spawn_blocking(move || {
                    ReadTailnetPeers::new(tailscale.as_ref()).execute()
                })
                .await
                .map_err(|_| "Tailscale peer probe terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::TailscalePeers { snapshot })
            }
            ControlOperation::TailscaleMode { mode } => {
                let _serial = self.router_proxy.lock().await;
                let (status, login_url) = self
                    .reconcile_tailscale(TailscaleDesired { mode })
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(ControlResult::TailscaleMutation { status, login_url })
            }
            ControlOperation::TailscaleLogin { .. } => {
                let _serial = self.router_proxy.lock().await;
                let desired = self
                    .tailscale_desired()
                    .await
                    .map_err(|error| error.to_string())?;
                if desired.mode == TailscaleMode::Disabled {
                    return Err(
                        "select RouterOnly or LAN subnet access before requesting login".to_owned(),
                    );
                }
                let (status, login_url) = self
                    .reconcile_tailscale(desired)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(ControlResult::TailscaleMutation { status, login_url })
            }
            ControlOperation::TailscaleLogout { .. } => {
                let _serial = self.router_proxy.lock().await;
                let status = self
                    .logout_tailscale()
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(ControlResult::TailscaleMutation {
                    status,
                    login_url: None,
                })
            }
            ControlOperation::Dhcp { event } => {
                self.dhcp
                    .dispatch(event)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(completed("DHCP event queued"))
            }
            ControlOperation::Router { enabled } => {
                let _serial = self.router_proxy.lock().await;
                let mut actions_applied = 0;
                if !enabled {
                    // Close remote LAN access and LAN interception while ordinary forwarding is
                    // still confirmed. Persisted Tailscale access mode and proxy intent survive.
                    self.degrade_tailscale_to_router_only()
                        .await
                        .map_err(|error| error.to_string())?;
                    let mut desired = self
                        .proxy_desired()
                        .await
                        .map_err(|error| error.to_string())?;
                    desired.lan_tun_enabled = false;
                    actions_applied += self
                        .reconcile_proxy_runtime_preserving_features(desired)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                let platform = self.router.clone();
                let router_actions = tokio::task::spawn_blocking(move || {
                    let desired = if enabled {
                        NetworkDesired::forwarding()
                    } else {
                        NetworkDesired::management_only()
                    };
                    RouterApplication::new(platform.as_ref(), platform.as_ref(), platform.as_ref())
                        .reconcile(&desired)
                        .map(|result| result.actions_applied)
                })
                .await
                .map_err(|_| "router worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                actions_applied += router_actions;
                if let Err(error) = self.reconcile_persisted_tailscale().await {
                    eprintln!(
                        "hyz-router: router reconciliation succeeded but Tailscale remains degraded: {error}"
                    );
                }
                if enabled {
                    let desired = self
                        .proxy_desired()
                        .await
                        .map_err(|error| error.to_string())?;
                    actions_applied += self
                        .reconcile_proxy_features(desired)
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Ok(completed(format!(
                    "router reconciled; actions={actions_applied}"
                )))
            }
            ControlOperation::Proxy { mode } => {
                let _serial = self.router_proxy.lock().await;
                let mut desired = self
                    .proxy_desired()
                    .await
                    .map_err(|error| error.to_string())?;
                match mode {
                    ControlProxyMode::Explicit | ControlProxyMode::Disabled => {
                        desired.lan_tun_enabled = false;
                        desired.tailscale_explicit_proxy_enabled = false;
                    }
                    ControlProxyMode::Tun => desired.lan_tun_enabled = true,
                }
                let actions = self
                    .reconcile_proxy_features(desired)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(completed(format!(
                    "proxy features reconciled; actions={actions}"
                )))
            }
            ControlOperation::ProxyLanTun { enabled } => {
                let _serial = self.router_proxy.lock().await;
                let mut desired = self
                    .proxy_desired()
                    .await
                    .map_err(|error| error.to_string())?;
                desired.lan_tun_enabled = enabled;
                let actions = self
                    .reconcile_proxy_features(desired)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(completed(format!("LAN TUN reconciled; actions={actions}")))
            }
            ControlOperation::ProxyTailscale { enabled } => {
                let _serial = self.router_proxy.lock().await;
                let mut desired = self
                    .proxy_desired()
                    .await
                    .map_err(|error| error.to_string())?;
                desired.tailscale_explicit_proxy_enabled = enabled;
                let actions = self
                    .reconcile_proxy_features(desired)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(completed(format!(
                    "Tailscale proxy reconciled; actions={actions}"
                )))
            }
            ControlOperation::WifiStatus { .. } => {
                let _serial = self.router_proxy.lock().await;
                let platform = self.router.clone();
                let config = tokio::task::spawn_blocking(move || {
                    WifiApplication::new(platform.as_ref()).committed()
                })
                .await
                .map_err(|_| "Wi-Fi status worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::WifiConfig { config })
            }
            ControlOperation::WifiPending { .. } => {
                let _serial = self.router_proxy.lock().await;
                let platform = self.router.clone();
                let (pending, applied) = tokio::task::spawn_blocking(move || {
                    WifiApplication::new(platform.as_ref()).pending_status()
                })
                .await
                .map_err(|_| "Wi-Fi pending worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                let remaining_seconds = if applied {
                    pending.as_ref().map(|pending| {
                        let deadline = pending
                            .staged_at_unix_ms
                            .saturating_add(AP_CONFIRM_TIMEOUT_SECS * 1_000);
                        deadline
                            .saturating_sub(self.router.unix_time_millis())
                            .saturating_add(999)
                            / 1_000
                    })
                } else {
                    None
                };
                Ok(ControlResult::WifiPendingStatus {
                    pending,
                    applied,
                    remaining_seconds,
                })
            }
            ControlOperation::WifiScan { .. } => {
                let _serial = self.router_proxy.lock().await;
                let platform = self.router.clone();
                let entries = tokio::task::spawn_blocking(move || {
                    WifiApplication::new(platform.as_ref()).scan()
                })
                .await
                .map_err(|_| "Wi-Fi scan worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::WifiScan { entries })
            }
            ControlOperation::WifiStaApply { request } => {
                let _serial = self.router_proxy.lock().await;
                let platform = self.router.clone();
                let config = tokio::task::spawn_blocking(move || {
                    WifiApplication::new(platform.as_ref()).apply_sta(request)
                })
                .await
                .map_err(|_| "Wi-Fi STA worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::WifiConfig { config })
            }
            ControlOperation::WifiApPrepare { request } => {
                let _serial = self.router_proxy.lock().await;
                let staged_at = self.router.unix_time_millis();
                let platform = self.router.clone();
                let pending = tokio::task::spawn_blocking(move || {
                    WifiApplication::new(platform.as_ref()).prepare_ap(request, staged_at)
                })
                .await
                .map_err(|_| "Wi-Fi AP prepare worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::WifiPending { pending })
            }
            ControlOperation::WifiApApply { .. } => {
                let _serial = self.router_proxy.lock().await;
                let applied_at = self.router.unix_time_millis();
                let platform = self.router.clone();
                let pending = tokio::task::spawn_blocking(move || {
                    WifiApplication::new(platform.as_ref()).apply_ap(applied_at)
                })
                .await
                .map_err(|_| "Wi-Fi AP apply worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::WifiPending { pending })
            }
            ControlOperation::WifiApConfirm { .. } => {
                let _serial = self.router_proxy.lock().await;
                let platform = self.router.clone();
                let config = tokio::task::spawn_blocking(move || {
                    WifiApplication::new(platform.as_ref()).confirm_ap()
                })
                .await
                .map_err(|_| "Wi-Fi AP confirm worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::WifiConfig { config })
            }
            ControlOperation::WifiApCancel { .. } => {
                let _serial = self.router_proxy.lock().await;
                let platform = self.router.clone();
                let config = tokio::task::spawn_blocking(move || {
                    WifiApplication::new(platform.as_ref()).cancel_ap()
                })
                .await
                .map_err(|_| "Wi-Fi AP cancel worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(ControlResult::WifiConfig { config })
            }
            ControlOperation::Ota { command } => {
                let _serial = self.ota.lock().await;
                let firmware = self.firmware.clone();
                tokio::task::spawn_blocking(move || run_ota_command(firmware, command))
                    .await
                    .map_err(|_| "OTA worker terminated unexpectedly".to_owned())?
            }
        }
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = run(std::env::args().skip(1).collect()).await {
        eprintln!("hyz-router: {error}");
        std::process::exit(1);
    }
}

async fn run(args: Vec<String>) -> Result<(), Box<dyn Error>> {
    if let Some(invocation) = WatcherInvocation::parse_hidden(&args) {
        require_root("internal Mihomo watcher")?;
        // Sole privileged exception to daemon-owned mutation: the same ELF constructs only the
        // minimal Linux fail-open adapter, bound to its exact root-only watcher identity record.
        let platform = LinuxMihomoFailOpenPlatform::new();
        MihomoFailOpenApplication::new(&platform).execute(invocation?)?;
        return Ok(());
    }
    if let Some(event) = dhcp_hook::try_event(&args) {
        let result = request(ControlOperation::Dhcp { event: event? }).await?;
        expect_completed(result)?;
        return Ok(());
    }

    match args.as_slice() {
        [command] if command == "daemon" => run_daemon().await?,
        [command] if command == "status" => print_status(false).await?,
        [command, flag] if command == "status" && flag == "--json" => print_status(true).await?,
        [group, action] if group == "router" && matches!(action.as_str(), "enable" | "disable") => {
            print_completed(
                request(ControlOperation::Router {
                    enabled: action == "enable",
                })
                .await?,
            )?;
        }
        [group, feature, action]
            if group == "proxy"
                && matches!(feature.as_str(), "lan-tun" | "tailscale")
                && matches!(action.as_str(), "enable" | "disable") =>
        {
            let enabled = action == "enable";
            let operation = if feature == "lan-tun" {
                ControlOperation::ProxyLanTun { enabled }
            } else {
                ControlOperation::ProxyTailscale { enabled }
            };
            print_completed(request(operation).await?)?;
        }
        [group, action] if group == "wifi" && action == "status" => {
            print_wifi_result(request(ControlOperation::WifiStatus {}).await?)?;
        }
        [group, action] if group == "wifi" && action == "scan" => {
            print_wifi_result(request(ControlOperation::WifiScan {}).await?)?;
        }
        [group, role, action] if group == "wifi" && role == "ap" => {
            let operation = match action.as_str() {
                "apply" => ControlOperation::WifiApApply {},
                "confirm" => ControlOperation::WifiApConfirm {},
                "cancel" => ControlOperation::WifiApCancel {},
                _ => return Err(usage_error(USAGE)),
            };
            print_wifi_result(request(operation).await?)?;
        }
        [group, action] if group == "subscription" && action == "get" => {
            print_subscription(request(ControlOperation::SubscriptionGet {}).await?, false)?;
        }
        [group, action, flag] if group == "subscription" && action == "get" && flag == "--json" => {
            print_subscription(request(ControlOperation::SubscriptionGet {}).await?, true)?;
        }
        [group, action] if group == "subscription" && action == "refresh" => {
            print_subscription(
                request(ControlOperation::SubscriptionRefresh {}).await?,
                false,
            )?;
        }
        [group, rest @ ..] if group == "ota" => {
            let command = parse_ota_cli(rest.iter().map(String::as_str))
                .map_err(|_| usage_error(OTA_USAGE))?;
            print_completed(request(ControlOperation::Ota { command }).await?)?;
        }
        _ => return Err(usage_error(USAGE)),
    }
    Ok(())
}

struct ShutdownSignals {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
}

impl ShutdownSignals {
    fn install() -> std::io::Result<Self> {
        Ok(Self {
            interrupt: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?,
            terminate: tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?,
        })
    }

    async fn recv(&mut self) {
        tokio::select! {
            _ = self.interrupt.recv() => {}
            _ = self.terminate.recv() => {}
        }
    }
}

async fn run_daemon() -> Result<(), Box<dyn Error>> {
    require_root("daemon")?;
    let mut signals = ShutdownSignals::install()?;
    let port = std::env::var("HYZ_ROUTER_HTTP_PORT")
        .map(|value| value.parse::<u16>())
        .unwrap_or(Ok(DEFAULT_HTTP_PORT))?;
    let mut ownership = acquire_daemon_ownership()?;
    let control_listener = match bind_control_socket(&mut ownership) {
        Ok(listener) => listener,
        Err(error) => {
            ownership.release()?;
            return Err(error.into());
        }
    };

    // This is the sole production construction point for privileged outbound adapters. Admin
    // credential initialization is fail-closed: HTTP and control services are not exposed if the
    // root-owned credential record cannot be loaded or bootstrapped.
    let runtime = match ProductionRuntime::build() {
        Ok(runtime) => Arc::new(runtime),
        Err(error) => {
            remove_control_socket(&ownership)?;
            ownership.release()?;
            return Err(error.into());
        }
    };
    let http_status = runtime.status();
    let http_admin = runtime.admin();
    let web_token = match runtime.web_token() {
        Ok(token) => token,
        Err(error) => {
            remove_control_socket(&ownership)?;
            ownership.release()?;
            return Err(error.into());
        }
    };
    if let Err(error) = runtime.tailscale.attach_http(
        Arc::downgrade(&runtime),
        web_token.clone(),
        port,
        tokio::runtime::Handle::current(),
    ) {
        remove_control_socket(&ownership)?;
        ownership.release()?;
        return Err(error.into());
    }
    let control_runtime: Arc<dyn ControlHandler> = runtime.clone();
    let http_control = control_runtime.clone();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // The control socket must be actively serving before management startup launches udhcpc.
    // DHCP operations deliberately bypass router_proxy; all other router/proxy requests queue
    // behind initialization and therefore cannot observe a successful startup prematurely.
    let mut control = tokio::spawn(serve_control(
        control_listener,
        control_runtime,
        shutdown_rx.clone(),
    ));
    let initialize_runtime = runtime.clone();
    let mut initialize = tokio::spawn(async move { initialize_runtime.initialize().await });
    let (initialize_result, startup_deadline) = tokio::select! {
        result = &mut initialize => (join_initialize(result), None),
        _ = signals.recv() => {
            let deadline = TokioInstant::now() + DAEMON_SHUTDOWN_TIMEOUT;
            arm_shutdown_deadline(deadline);
            let _ = shutdown_tx.send(true);
            let result = match timeout_at(deadline, &mut initialize).await {
                Ok(result) => join_initialize(result),
                Err(_) => {
                    return Err("daemon shutdown deadline expired while initialization was still active; ownership evidence retained".into());
                }
            };
            (result, Some(deadline))
        }
    };
    if let Err(error) = initialize_result {
        let deadline =
            startup_deadline.unwrap_or_else(|| TokioInstant::now() + DAEMON_SHUTDOWN_TIMEOUT);
        if startup_deadline.is_none() {
            arm_shutdown_deadline(deadline);
        }
        let _ = shutdown_tx.send(true);
        let control_result = match timeout_at(deadline, &mut control).await {
            Ok(result) => join_control(result),
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "control drain exceeded the daemon shutdown deadline",
            )),
        };
        let cleanup_result = if control_result.is_ok() {
            shutdown_runtime_before(runtime.clone(), deadline).await
        } else {
            Err("runtime cleanup skipped because control operations did not drain".to_owned())
        };
        if let Err(cleanup_error) = &cleanup_result {
            let _ = runtime.router.record_shutdown_failure(&format!(
                "startup cleanup failed after initialization error ({error}): {cleanup_error}"
            ));
        }
        if cleanup_result.is_ok() {
            remove_control_socket(&ownership)?;
            ownership.release()?;
        }
        let mut message = error;
        if let Err(control_error) = control_result {
            message.push_str(&format!("; control shutdown failed: {control_error}"));
        }
        if let Err(cleanup_error) = cleanup_result {
            message.push_str(&format!("; runtime cleanup failed: {cleanup_error}"));
        }
        return Err(message.into());
    }
    runtime.router.clear_shutdown_failure_log()?;

    if let Some(deadline) = startup_deadline {
        let _ = shutdown_tx.send(true);
        let control_result = match timeout_at(deadline, &mut control).await {
            Ok(result) => join_control(result),
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "control drain exceeded the daemon shutdown deadline",
            )),
        };
        let cleanup = if control_result.is_ok() {
            shutdown_runtime_before(runtime.clone(), deadline).await
        } else {
            Err("runtime cleanup skipped because control operations did not drain".to_owned())
        };
        if let Err(error) = &cleanup {
            let _ = runtime
                .router
                .record_shutdown_failure(&format!("runtime shutdown failed: {error}"));
        }
        let cleanup_succeeded = cleanup.is_ok();
        let result = combine_runtime_results(control_result, cleanup);
        if cleanup_succeeded {
            remove_control_socket(&ownership)?;
            ownership.release()?;
        }
        return result.map_err(|error| Box::new(error) as Box<dyn Error>);
    }

    let timeout_runtime = runtime.clone();
    let mut timeout_shutdown = shutdown_rx.clone();
    let mut wifi_timeout = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(error) = timeout_runtime.expire_pending_ap().await {
                        eprintln!("hyz-router: AP transaction timeout rollback failed: {error}");
                    }
                }
                changed = timeout_shutdown.changed() => {
                    let _ = changed;
                    break;
                }
            }
        }
    });

    let recovery_runtime = runtime.clone();
    let mut recovery_shutdown = shutdown_rx.clone();
    let mut direct_recovery = tokio::spawn(async move {
        let mut interval = tokio::time::interval(MIHOMO_DIRECT_RECOVERY_INTERVAL);
        let mut runtime_restored = false;
        let mut last_restore_error = None;
        let mut last_direct_error = None;
        interval.tick().await;
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let _serial = recovery_runtime.router_proxy.lock().await;
                    match recovery_runtime.wan_route_ready().await {
                        Ok(false) => {
                            runtime_restored = false;
                            last_restore_error = None;
                        }
                        Ok(true) if !runtime_restored => {
                            match recovery_runtime.restore_persisted_runtime_if_wan_ready().await {
                                Ok(true) => {
                                    runtime_restored = true;
                                    last_restore_error = None;
                                    eprintln!("hyz-router: deferred WAN, Tailscale, and proxy runtime restoration completed");
                                }
                                Ok(false) => {
                                    runtime_restored = false;
                                    last_restore_error = None;
                                }
                                Err(error) => {
                                    let detail = error.to_string();
                                    if last_restore_error.as_deref() != Some(detail.as_str()) {
                                        eprintln!("hyz-router: deferred runtime restoration remains pending: {detail}");
                                        last_restore_error = Some(detail);
                                    }
                                }
                            }
                        }
                        Ok(true) => {}
                        Err(error) => {
                            runtime_restored = false;
                            let detail = error.to_string();
                            if last_restore_error.as_deref() != Some(detail.as_str()) {
                                eprintln!("hyz-router: deferred WAN readiness remains unknown: {detail}");
                                last_restore_error = Some(detail);
                            }
                        }
                    }
                    match recovery_runtime.recover_tailscale_direct_if_mihomo_unavailable().await {
                        Ok(MihomoDirectRecoveryResult::Restored { tailscale_actions_applied }) => {
                            last_direct_error = None;
                            eprintln!(
                                "hyz-router: Mihomo core unavailable; restored tailscaled Direct environment with {tailscale_actions_applied} typed actions"
                            );
                        }
                        Ok(MihomoDirectRecoveryResult::NotNeeded | MihomoDirectRecoveryResult::AlreadyDirect) => {
                            last_direct_error = None;
                        }
                        Err(error) => {
                            let detail = error.to_string();
                            if last_direct_error.as_deref() != Some(detail.as_str()) {
                                eprintln!("hyz-router: bounded tailscaled Direct recovery failed: {detail}");
                                last_direct_error = Some(detail);
                            }
                        }
                    }
                }
                changed = recovery_shutdown.changed() => {
                    let _ = changed;
                    break;
                }
            }
        }
    });

    // HTTP binding is the externally visible readiness boundary and occurs only after a strictly
    // confirmed normal or management-only network state has been reached.
    let mut http = tokio::spawn(serve_http(
        http_status,
        http_control,
        http_admin,
        web_token,
        port,
        shutdown_rx,
    ));

    enum Trigger {
        Signal,
        Control(std::io::Result<()>),
        Http(std::io::Result<()>),
    }
    let trigger = tokio::select! {
        result = &mut control => Trigger::Control(join_control(result)),
        result = &mut http => Trigger::Http(join_http(result)),
        _ = signals.recv() => Trigger::Signal,
    };
    let deadline = TokioInstant::now() + DAEMON_SHUTDOWN_TIMEOUT;
    arm_shutdown_deadline(deadline);
    let _ = shutdown_tx.send(true);
    let drain = async {
        match trigger {
            Trigger::Signal => {
                let (control, http) = tokio::join!(&mut control, &mut http);
                combine_service_results(join_control(control), join_http(http))
            }
            Trigger::Control(control_result) => {
                combine_service_results(control_result, join_http((&mut http).await))
            }
            Trigger::Http(http_result) => {
                combine_service_results(join_control((&mut control).await), http_result)
            }
        }
    };
    let services = match timeout_at(deadline, drain).await {
        Ok(result) => result,
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "HTTP/control drain exceeded the daemon shutdown deadline",
        )),
    };
    let timeout_task = match timeout_at(deadline, &mut wifi_timeout).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(std::io::Error::other(format!(
            "AP timeout task terminated unexpectedly: {error}"
        ))),
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "AP timeout task drain exceeded the daemon shutdown deadline",
        )),
    };
    let services = combine_service_results(services, timeout_task);
    let recovery_task = match timeout_at(deadline, &mut direct_recovery).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(std::io::Error::other(format!(
            "Mihomo Direct recovery task terminated unexpectedly: {error}"
        ))),
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "Mihomo Direct recovery task drain exceeded the daemon shutdown deadline",
        )),
    };
    let services = combine_service_results(services, recovery_task);

    // Runtime cleanup starts only after all request handlers have drained. If drain misses the
    // absolute deadline, the process exits with ownership evidence intact instead of cancelling a
    // blocking mutation and releasing its lock while the blocking worker is still running.
    let cleanup = if services.is_ok() {
        shutdown_runtime_before(runtime.clone(), deadline).await
    } else {
        Err("runtime cleanup skipped because HTTP/control operations did not drain".to_owned())
    };
    if let Err(error) = &cleanup {
        let _ = runtime
            .router
            .record_shutdown_failure(&format!("runtime shutdown failed: {error}"));
    }
    let cleanup_succeeded = cleanup.is_ok();
    let result = combine_runtime_results(services, cleanup);

    // A failed strict cleanup intentionally leaves the root-owned daemon lock and control socket
    // as durable evidence. SysV stop must fail and automatic restart must remain blocked until an
    // operator investigates residual processes, routes, firewall state, and ownership records.
    if cleanup_succeeded {
        remove_control_socket(&ownership)?;
        ownership.release()?;
    }
    result.map_err(|error| Box::new(error) as Box<dyn Error>)
}

async fn serve_http(
    status: ReadStatus,
    control: Arc<dyn ControlHandler>,
    admin: Arc<AdminApplication>,
    csrf_token: String,
    port: u16,
    mut shutdown: watch::Receiver<bool>,
) -> std::io::Result<()> {
    if *shutdown.borrow() {
        return Ok(());
    }
    let bind = bind_fixed_lan_with_retry(port, DEFAULT_BIND_ATTEMPTS, Duration::from_secs(1));
    tokio::pin!(bind);
    let listener = tokio::select! {
        result = &mut bind => result?,
        changed = shutdown.changed() => {
            let _ = changed;
            return Ok(());
        }
    };
    axum::serve(
        listener,
        app_with_admin_control(status, control, admin, csrf_token, port),
    )
    .with_graceful_shutdown(async move {
        while !*shutdown.borrow() && shutdown.changed().await.is_ok() {}
    })
    .await
}

fn join_initialize(
    result: Result<Result<(), String>, tokio::task::JoinError>,
) -> Result<(), String> {
    result.unwrap_or_else(|error| {
        Err(format!(
            "initialization task terminated unexpectedly: {error}"
        ))
    })
}

async fn shutdown_runtime_before(
    runtime: Arc<ProductionRuntime>,
    deadline: TokioInstant,
) -> Result<(), String> {
    let mut cleanup = tokio::spawn(async move { runtime.shutdown().await });
    match timeout_at(deadline, &mut cleanup).await {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => Err(format!(
            "runtime cleanup task terminated unexpectedly: {error}"
        )),
        Err(_) => Err("runtime cleanup exceeded the daemon shutdown deadline".to_owned()),
    }
}

fn join_control(
    result: Result<std::io::Result<()>, tokio::task::JoinError>,
) -> std::io::Result<()> {
    result.unwrap_or_else(|error| {
        Err(std::io::Error::other(format!(
            "control service terminated unexpectedly: {error}"
        )))
    })
}

fn join_http(result: Result<std::io::Result<()>, tokio::task::JoinError>) -> std::io::Result<()> {
    result.unwrap_or_else(|error| {
        Err(std::io::Error::other(format!(
            "HTTP service terminated unexpectedly: {error}"
        )))
    })
}

fn combine_service_results(
    control: std::io::Result<()>,
    http: std::io::Result<()>,
) -> std::io::Result<()> {
    control?;
    http
}

fn combine_runtime_results(
    services: std::io::Result<()>,
    cleanup: Result<(), String>,
) -> std::io::Result<()> {
    match (services, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(cleanup)) => Err(std::io::Error::other(format!(
            "runtime cleanup failed: {cleanup}"
        ))),
        (Err(error), Err(cleanup)) => Err(std::io::Error::other(format!(
            "service shutdown failed: {error}; runtime cleanup also failed: {cleanup}"
        ))),
    }
}

async fn print_status(compact: bool) -> Result<(), Box<dyn Error>> {
    let ControlResult::Status { snapshot } = request(ControlOperation::Status {}).await? else {
        return Err("daemon returned an unexpected status response".into());
    };
    if compact {
        println!("{}", serde_json::to_string(&snapshot)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    }
    Ok(())
}

fn run_ota_command(
    firmware: FirmwareAdapter,
    command: OtaCommand,
) -> Result<ControlResult, String> {
    let service = OtaService::new(firmware);
    let message = match command {
        OtaCommand::Verify { firmware, expected } => {
            service
                .verify(Path::new(&firmware), &expected)
                .map_err(|error| error.to_string())?;
            format!("verified {firmware}")
        }
        OtaCommand::Download { source, expected } => {
            let path = service
                .download(&source, &expected)
                .map_err(|error| error.to_string())?;
            format!("downloaded and verified {}", path.display())
        }
        OtaCommand::Install {
            firmware,
            expected,
            reboot,
        } => {
            let path = service
                .install(
                    Path::new(&firmware),
                    &expected,
                    InstallMode::RecoveryFree,
                    reboot,
                )
                .map_err(|error| error.to_string())?;
            format!("staged verified firmware {}", path.display())
        }
        OtaCommand::InstallRecovery {
            firmware,
            expected,
            reboot,
        } => {
            let path = service
                .install(
                    Path::new(&firmware),
                    &expected,
                    InstallMode::IncludeRecovery,
                    reboot,
                )
                .map_err(|error| error.to_string())?;
            format!("staged verified firmware {}", path.display())
        }
        OtaCommand::Apply {
            source,
            expected,
            reboot,
        } => {
            let path = service
                .apply(&source, &expected, reboot)
                .map_err(|error| error.to_string())?;
            format!("staged verified firmware {}", path.display())
        }
    };
    Ok(completed(message))
}

fn completed(message: impl Into<String>) -> ControlResult {
    ControlResult::Completed {
        message: message.into(),
    }
}

fn print_wifi_result(result: ControlResult) -> Result<(), Box<dyn Error>> {
    match result {
        ControlResult::WifiConfig { config } => {
            println!("{}", serde_json::to_string_pretty(&config)?);
        }
        ControlResult::WifiPending { pending } => {
            println!("{}", serde_json::to_string_pretty(&pending)?);
        }
        ControlResult::WifiScan { entries } => {
            println!("{}", serde_json::to_string_pretty(&entries)?);
        }
        _ => return Err("daemon returned an unexpected Wi-Fi response".into()),
    }
    Ok(())
}

fn expect_completed(result: ControlResult) -> Result<String, Box<dyn Error>> {
    match result {
        ControlResult::Completed { message } => Ok(message),
        ControlResult::Status { .. }
        | ControlResult::PanelStatus { .. }
        | ControlResult::ProxyDelay { .. }
        | ControlResult::ProxyDelays { .. }
        | ControlResult::WifiConfig { .. }
        | ControlResult::WifiPending { .. }
        | ControlResult::WifiPendingStatus { .. }
        | ControlResult::WifiScan { .. }
        | ControlResult::DevicePolicies { .. }
        | ControlResult::Subscription { .. }
        | ControlResult::Tailscale { .. }
        | ControlResult::TailscalePeers { .. }
        | ControlResult::TailscaleMutation { .. } => {
            Err("daemon returned an unexpected mutation response".into())
        }
    }
}

fn print_subscription(result: ControlResult, json: bool) -> Result<(), Box<dyn Error>> {
    let ControlResult::Subscription { summary } = result else {
        return Err("daemon returned an unexpected subscription response".into());
    };
    if json {
        println!("{}", serde_json::to_string(&summary)?);
    } else {
        let state = match summary.state {
            hyz_router::domain::subscription::SubscriptionSummaryState::Idle => "idle",
            hyz_router::domain::subscription::SubscriptionSummaryState::Fetching => "fetching",
            hyz_router::domain::subscription::SubscriptionSummaryState::Active => "active",
            hyz_router::domain::subscription::SubscriptionSummaryState::Failed => "failed",
        };
        println!("configured={} state={state}", summary.configured);
    }
    Ok(())
}

fn print_completed(result: ControlResult) -> Result<(), Box<dyn Error>> {
    println!("{}", expect_completed(result)?);
    Ok(())
}

fn usage_error(message: &'static str) -> Box<dyn Error> {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message).into()
}

#[cfg(test)]
mod source_boundaries {
    use super::*;
    use axum::{routing::get, Router};
    use tokio::{io::AsyncWriteExt, sync::Notify};

    fn test_listener_platform(listener: RunningTailscaleListener) -> ProductionTailscalePlatform {
        let platform = ProductionTailscalePlatform::new(Arc::new(LinuxRouterPlatform::new()));
        *platform.listener.lock().unwrap() = Some(listener);
        platform
    }
    #[test]
    fn daemon_serves_control_before_initialization_and_http_afterward() {
        let production = include_str!("main.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let control = production.find("tokio::spawn(serve_control").unwrap();
        let initialize = production.find("initialize_runtime.initialize()").unwrap();
        let http = production.find("tokio::spawn(serve_http").unwrap();
        assert!(control < initialize && initialize < http);
    }

    #[test]
    fn shutdown_signals_and_drains_share_one_absolute_deadline() {
        let production = include_str!("main.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let install = production.find("ShutdownSignals::install()").unwrap();
        let initialize = production.find("initialize_runtime.initialize()").unwrap();
        assert!(install < initialize);
        assert!(production.contains("timeout_at(deadline, &mut initialize)"));
        assert!(production.contains("timeout_at(deadline, drain)"));
        assert!(production.contains("shutdown_runtime_before(runtime.clone(), deadline)"));
        assert!(production.contains("arm_shutdown_deadline(deadline)"));
        let ota = include_str!("application/ota.rs");
        assert!(ota.contains("OTA_OPERATION_TIMEOUT: Duration = Duration::from_secs(90)"));
        assert!(ota.contains("download_to_staging_temporary(source, deadline)"));
        assert!(ota.contains("stage_with_update_engine(firmware, deadline)"));
        assert!(ota.contains("reboot(deadline)"));
        assert!(production
            .contains("runtime cleanup skipped because HTTP/control operations did not drain"));
    }

    #[test]
    fn management_only_startup_does_not_wait_for_wan_or_proxy() {
        let production = include_str!("main.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let initialize = production
            .split("async fn initialize(&self)")
            .nth(1)
            .unwrap()
            .split("async fn wan_route_ready")
            .next()
            .unwrap();
        assert!(initialize.contains("NetworkDesired::management_only()"));
        assert!(initialize.contains("self.recover_device_policy().await?;"));
        assert!(!initialize.contains("NetworkDesired::forwarding()"));
        assert!(!initialize.contains("reconcile_persisted_tailscale"));
        assert!(!initialize.contains("reconcile_proxy_features"));
    }

    #[test]
    fn deferred_runtime_restores_forwarding_before_tailscale_and_proxy() {
        let production = include_str!("main.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let restore = production
            .split("async fn restore_persisted_runtime_if_wan_ready")
            .nth(1)
            .unwrap()
            .split("async fn recover_device_policy")
            .next()
            .unwrap();
        let forwarding = restore
            .find("reconcile_network(NetworkDesired::forwarding())")
            .unwrap();
        let tailscale = restore.find("reconcile_persisted_tailscale()").unwrap();
        let proxy = restore.find("reconcile_proxy_features(desired)").unwrap();
        assert!(forwarding < tailscale && tailscale < proxy);
    }

    #[test]
    fn router_disable_cleans_proxy_before_disabling_forwarding() {
        let production = include_str!("main.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let branch = production
            .split("ControlOperation::Router { enabled } =>")
            .nth(1)
            .unwrap()
            .split("ControlOperation::Proxy { mode } =>")
            .next()
            .unwrap();
        let disabled_guard = branch.find("if !enabled").unwrap();
        let tailscale = branch.find("degrade_tailscale_to_router_only").unwrap();
        let proxy = branch
            .find("reconcile_proxy_runtime_preserving_features(desired)")
            .unwrap();
        let router = branch.find("RouterApplication::new").unwrap();
        assert!(disabled_guard < tailscale && tailscale < proxy && proxy < router);
        assert!(!branch[..router].contains("reconcile_proxy_features(desired)"));
        assert!(branch.contains("desired.lan_tun_enabled = false"));
        assert!(branch.contains("NetworkDesired::management_only()"));
    }

    #[test]
    fn tailscale_listener_is_exact_and_shutdown_precedes_proxy_network_cleanup() {
        let production = include_str!("main.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let listener = production
            .split("fn start_listener")
            .nth(1)
            .unwrap()
            .split("fn stop_listener")
            .next()
            .unwrap();
        assert!(listener.contains("SocketAddr::from((ipv4, config.port))"));
        assert!(!listener.contains("0.0.0.0"));
        assert!(listener.contains("app_with_admin_control_at_address"));

        let shutdown = production
            .split("async fn shutdown(&self)")
            .nth(1)
            .unwrap()
            .split("fn startup_failure")
            .next()
            .unwrap();
        let tailscale = shutdown.find("self.shutdown_tailscale()").unwrap();
        let proxy_network = shutdown.find("ShutdownApplication::new").unwrap();
        assert!(tailscale < proxy_network);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tailscale_listener_stop_is_bounded_reaped_and_allows_exact_rebind() {
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let app = Router::new().route(
            "/",
            get({
                let entered = entered.clone();
                let release = release.clone();
                move || {
                    let entered = entered.clone();
                    let release = release.clone();
                    async move {
                        entered.notify_one();
                        release.notified().await;
                        "stopped"
                    }
                }
            }),
        );
        let listener =
            bind_exact_tailscale_listener(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let (shutdown, stopped) = oneshot::channel();
        let (accept_stopped, accept_stopped_rx) = std_mpsc::channel();
        let listener = ConfirmedTailscaleListener {
            listener,
            accept_stopped: Some(accept_stopped),
        };
        let runtime = tokio::runtime::Handle::current();
        let task = runtime.spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await
        });
        let platform = Arc::new(test_listener_platform(RunningTailscaleListener {
            token: "listener-owned".to_owned(),
            ipv4: Ipv4Addr::LOCALHOST,
            runtime,
            shutdown: Some(shutdown),
            accept_stopped: accept_stopped_rx,
            task,
        }));

        let mut request = tokio::net::TcpStream::connect(address).await.unwrap();
        request
            .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .unwrap();

        let stopping = platform.clone();
        let stopped = tokio::task::spawn_blocking(move || stopping.stop_listener("listener-owned"));
        tokio::time::timeout(Duration::from_secs(3), stopped)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let rebound = bind_exact_tailscale_listener(address).unwrap();
        assert!(platform.listener.lock().unwrap().is_none());

        release.notify_waiters();
        drop(request);
        drop(rebound);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn finished_owned_listener_task_is_reaped_and_observed_as_absent() {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let (accept_stopped, accept_stopped_rx) = std_mpsc::channel();
        let listener = ConfirmedTailscaleListener {
            listener,
            accept_stopped: Some(accept_stopped),
        };
        let runtime = tokio::runtime::Handle::current();
        let task = runtime.spawn(async move {
            drop(listener);
            Err(std::io::Error::other("injected listener failure"))
        });
        while !task.is_finished() {
            tokio::task::yield_now().await;
        }
        let platform = Arc::new(test_listener_platform(RunningTailscaleListener {
            token: "listener-owned".to_owned(),
            ipv4: Ipv4Addr::LOCALHOST,
            runtime,
            shutdown: None,
            accept_stopped: accept_stopped_rx,
            task,
        }));

        let observing = platform.clone();
        let observation = tokio::task::spawn_blocking(move || observing.listener_observation())
            .await
            .unwrap();
        assert_eq!(
            observation,
            (Probe::Known(OwnedResource::Absent), Probe::Known(None))
        );
        assert!(platform.listener.lock().unwrap().is_none());
    }

    #[test]
    fn failed_strict_cleanup_retains_daemon_ownership_evidence() {
        let production = include_str!("main.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let shutdown = production
            .rsplit_once("let cleanup_succeeded = cleanup.is_ok()")
            .unwrap()
            .1;
        let guard = shutdown.find("if cleanup_succeeded").unwrap();
        let socket = shutdown.find("remove_control_socket(&ownership)").unwrap();
        let release = shutdown.find("ownership.release()").unwrap();
        assert!(guard < socket && socket < release);
        assert!(production.contains("let cleanup_succeeded = cleanup.is_ok()"));
    }

    #[test]
    fn client_and_dhcp_modules_do_not_construct_privileged_adapters() {
        let main = include_str!("main.rs");
        let production = main.split("#[cfg(test)]").next().unwrap();
        assert_eq!(production.matches("ProductionRuntime::build()").count(), 1);
        let hook = include_str!("adapters/inbound/dhcp_hook.rs");
        assert!(!hook.contains("adapters::outbound"));
        assert!(!hook.contains("LinuxRouterPlatform"));
        let control = include_str!("adapters/inbound/control.rs");
        assert!(!control.contains("adapters::outbound"));
        assert!(!control.contains("FirmwareAdapter"));
        let ota_cli = include_str!("adapters/inbound/ota_cli.rs");
        let ota_production = ota_cli.split("#[cfg(test)]").next().unwrap();
        assert!(!ota_production.contains(concat!("Ota", "Service")));
        assert!(!ota_production.contains(concat!("Firmware", "PlatformPort")));
        let management = include_str!("adapters/outbound/management.rs");
        let dhcp_apply = management
            .split("fn apply_dhcp_event_locked")
            .nth(1)
            .unwrap()
            .split("impl DhcpPlatformPort")
            .next()
            .unwrap();
        assert!(!dhcp_apply.contains("NETWORK_LOCK"));
        assert!(production.contains("DhcpApplication::new(platform.as_ref(), platform.as_ref())"));
    }
}
