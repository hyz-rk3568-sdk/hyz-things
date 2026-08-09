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
                app_with_control, bind_fixed_lan_with_retry, DEFAULT_BIND_ATTEMPTS,
                DEFAULT_HTTP_PORT,
            },
            ota_cli::{parse_ota_cli, OtaCommand, OTA_USAGE},
        },
        outbound::{firmware::FirmwareAdapter, LinuxMihomoFailOpenPlatform, LinuxRouterPlatform},
    },
    application::{
        dhcp::DhcpPlatformPort,
        fail_open::{MihomoFailOpenApplication, WatcherInvocation},
        ota::{InstallMode, OtaService},
        panel::PanelApplication,
        ports::{ClockPort, PlatformError, SystemProbePort},
        proxy::ProxyApplication,
        router::RouterApplication,
        shutdown::ShutdownApplication,
        status::ReadStatus,
    },
    domain::{
        network::NetworkDesired,
        proxy::{ProxyDesired, ProxyMode},
    },
};
use std::{
    error::Error,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{watch, Mutex};

const USAGE: &str = "Usage:\n  hyz-router daemon\n  hyz-router status [--json]\n  hyz-router router enable|disable\n  hyz-router proxy explicit|tun|disable\n  hyz-router ota ...";

struct ProductionRuntime {
    router: Arc<LinuxRouterPlatform>,
    firmware: FirmwareAdapter,
    router_proxy: Mutex<()>,
    proxy_delay_last: Mutex<Option<Instant>>,
    display: Mutex<()>,
    ota: Mutex<()>,
}

impl ProductionRuntime {
    fn build() -> Self {
        Self {
            router: Arc::new(LinuxRouterPlatform::new()),
            firmware: FirmwareAdapter::default(),
            router_proxy: Mutex::new(()),
            proxy_delay_last: Mutex::new(None),
            display: Mutex::new(()),
            ota: Mutex::new(()),
        }
    }

    fn status(&self) -> ReadStatus {
        ReadStatus::new(
            self.router.clone(),
            self.router.clone(),
            self.router.clone(),
        )
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
        // Router/proxy commands may connect while initialization runs, but only DHCP bypasses this
        // guard. That lets udhcpc install its route without deadlocking the startup transaction.
        let _serial = self.router_proxy.lock().await;
        if let Err(error) = self
            .reconcile_network(NetworkDesired::management_only())
            .await
        {
            return Err(format!(
                "startup management reconciliation failed and network safety is unconfirmed: {error}"
            ));
        }
        if let Err(error) = self.reconcile_network(NetworkDesired::forwarding()).await {
            match self
                .reconcile_network(NetworkDesired::management_only())
                .await
            {
                Ok(()) => {
                    eprintln!(
                        "hyz-router: startup forwarding is unavailable ({error}); continuing in strictly confirmed management-only mode"
                    );
                    return Ok(());
                }
                Err(degraded_error) => {
                    return Err(startup_failure(
                        "router forwarding reconciliation",
                        error,
                        degraded_error,
                    ));
                }
            }
        }

        let platform = self.router.clone();
        let proxy = tokio::task::spawn_blocking(move || {
            let observed = platform.observe_proxy()?;
            let mode = match observed.effective_persisted_mode() {
                hyz_router::domain::network::Probe::Known(mode) => mode,
                hyz_router::domain::network::Probe::Unknown(reason) => {
                    return Err(PlatformError::ProbeFailed(format!(
                        "effective persisted proxy mode is unknown: {reason}"
                    )));
                }
            };
            ProxyApplication::new(platform.as_ref(), platform.as_ref(), platform.as_ref())
                .reconcile(&ProxyDesired { mode })
                .map(|_| ())
        })
        .await
        .map_err(|_| "startup proxy worker terminated unexpectedly".to_owned())?;

        if let Err(error) = proxy {
            match self
                .reconcile_network(NetworkDesired::management_only())
                .await
            {
                Ok(()) => {
                    eprintln!(
                        "hyz-router: persisted proxy mode is unavailable ({error}); continuing in management-only mode"
                    );
                    return Ok(());
                }
                Err(degraded_error) => {
                    return Err(startup_failure(
                        "persisted proxy-mode restoration",
                        error,
                        degraded_error,
                    ));
                }
            }
        }
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), String> {
        let _serial = self.router_proxy.lock().await;
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

fn startup_failure(phase: &str, error: PlatformError, degraded_error: PlatformError) -> String {
    format!(
        "startup {phase} failed: {error}; forwarding-disable reconciliation also failed and network safety is unconfirmed: {degraded_error}"
    )
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
            ControlOperation::Dhcp { event } => {
                // Deliberately independent of router_proxy: udhcpc callbacks must run while router
                // startup waits for the DHCP-owned metric-600 route. The DHCP port never acquires
                // /run/hyz-network.lock.
                let platform = self.router.clone();
                tokio::task::spawn_blocking(move || platform.apply_dhcp_event(event))
                    .await
                    .map_err(|_| "DHCP worker terminated unexpectedly".to_owned())?
                    .map_err(|error| error.to_string())?;
                Ok(completed("DHCP event applied"))
            }
            ControlOperation::Router { enabled } => {
                let _serial = self.router_proxy.lock().await;
                let platform = self.router.clone();
                let actions_applied = tokio::task::spawn_blocking(move || {
                    let mut actions_applied = 0;
                    if !enabled {
                        // TUN interception must be removed while ordinary NAT is still confirmed.
                        // Committing disabled also makes a later router enable come back as plain NAT
                        // until the operator explicitly selects another proxy mode.
                        let proxy = ProxyApplication::new(
                            platform.as_ref(),
                            platform.as_ref(),
                            platform.as_ref(),
                        )
                        .reconcile(&ProxyDesired {
                            mode: ProxyMode::Disabled,
                        })?;
                        actions_applied += proxy.actions_applied;
                    }
                    let desired = if enabled {
                        NetworkDesired::forwarding()
                    } else {
                        NetworkDesired::management_only()
                    };
                    let router = RouterApplication::new(
                        platform.as_ref(),
                        platform.as_ref(),
                        platform.as_ref(),
                    )
                    .reconcile(&desired)?;
                    Ok::<usize, PlatformError>(actions_applied + router.actions_applied)
                })
                .await
                .map_err(|_| "router worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(completed(format!(
                    "router reconciled; actions={actions_applied}"
                )))
            }
            ControlOperation::Proxy { mode } => {
                let _serial = self.router_proxy.lock().await;
                let platform = self.router.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let mode = match mode {
                        ControlProxyMode::Explicit => ProxyMode::Explicit,
                        ControlProxyMode::Tun => ProxyMode::Tun,
                        ControlProxyMode::Disabled => ProxyMode::Disabled,
                    };
                    ProxyApplication::new(platform.as_ref(), platform.as_ref(), platform.as_ref())
                        .reconcile(&ProxyDesired { mode })
                })
                .await
                .map_err(|_| "proxy worker terminated unexpectedly".to_owned())?
                .map_err(|error| error.to_string())?;
                Ok(completed(format!(
                    "proxy reconciled; actions={}",
                    result.actions_applied
                )))
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
        [group, action] if group == "proxy" => {
            let mode = match action.as_str() {
                "explicit" => ControlProxyMode::Explicit,
                "tun" => ControlProxyMode::Tun,
                "disable" => ControlProxyMode::Disabled,
                _ => return Err(usage_error(USAGE)),
            };
            print_completed(request(ControlOperation::Proxy { mode }).await?)?;
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

async fn run_daemon() -> Result<(), Box<dyn Error>> {
    require_root("daemon")?;
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

    // This is the sole production construction point for privileged outbound adapters.
    let runtime = Arc::new(ProductionRuntime::build());
    let http_status = runtime.status();
    let web_token = match runtime.web_token() {
        Ok(token) => token,
        Err(error) => {
            remove_control_socket(&ownership)?;
            ownership.release()?;
            return Err(error.into());
        }
    };
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
    if let Err(error) = runtime.initialize().await {
        let _ = shutdown_tx.send(true);
        let control_result = join_control(control.await);
        let cleanup_result = runtime.shutdown().await;
        remove_control_socket(&ownership)?;
        ownership.release()?;
        let mut message = error;
        if let Err(control_error) = control_result {
            message.push_str(&format!("; control shutdown failed: {control_error}"));
        }
        if let Err(cleanup_error) = cleanup_result {
            message.push_str(&format!("; runtime cleanup failed: {cleanup_error}"));
        }
        return Err(message.into());
    }

    // HTTP binding is the externally visible readiness boundary and occurs only after a strictly
    // confirmed normal or management-only network state has been reached.
    let http = serve_http(http_status, http_control, web_token, port, shutdown_rx);
    tokio::pin!(http);

    enum Trigger {
        Signal,
        Control(std::io::Result<()>),
        Http(std::io::Result<()>),
    }
    let trigger = tokio::select! {
        result = &mut control => Trigger::Control(join_control(result)),
        result = &mut http => Trigger::Http(result),
        _ = shutdown_signal() => Trigger::Signal,
    };
    let _ = shutdown_tx.send(true);
    let services = match trigger {
        Trigger::Signal => combine_service_results(join_control(control.await), http.await),
        Trigger::Control(control_result) => combine_service_results(control_result, http.await),
        Trigger::Http(http_result) => {
            combine_service_results(join_control(control.await), http_result)
        }
    };
    // Control accepts are stopped and every in-flight operation (including OTA) has drained before
    // runtime-owned packet paths and child processes are removed. Persisted desired mode remains.
    let cleanup = runtime.shutdown().await;
    let result = combine_runtime_results(services, cleanup);

    remove_control_socket(&ownership)?;
    ownership.release()?;
    result.map_err(|error| Box::new(error) as Box<dyn Error>)
}

async fn serve_http(
    status: ReadStatus,
    control: Arc<dyn ControlHandler>,
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
        app_with_control(status, control, csrf_token, port),
    )
    .with_graceful_shutdown(async move {
        while !*shutdown.borrow() && shutdown.changed().await.is_ok() {}
    })
    .await
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

fn expect_completed(result: ControlResult) -> Result<String, Box<dyn Error>> {
    match result {
        ControlResult::Completed { message } => Ok(message),
        ControlResult::Status { .. }
        | ControlResult::PanelStatus { .. }
        | ControlResult::ProxyDelay { .. }
        | ControlResult::ProxyDelays { .. } => {
            Err("daemon returned an unexpected mutation response".into())
        }
    }
}

fn print_completed(result: ControlResult) -> Result<(), Box<dyn Error>> {
    println!("{}", expect_completed(result)?);
    Ok(())
}

fn usage_error(message: &'static str) -> Box<dyn Error> {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message).into()
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}

#[cfg(test)]
mod source_boundaries {
    #[test]
    fn daemon_serves_control_before_initialization_and_http_afterward() {
        let production = include_str!("main.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let control = production.find("tokio::spawn(serve_control").unwrap();
        let initialize = production.find("runtime.initialize().await").unwrap();
        let http = production.find("let http = serve_http").unwrap();
        assert!(control < initialize && initialize < http);
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
        let proxy = branch.find("ProxyApplication::new").unwrap();
        let router = branch.find("RouterApplication::new").unwrap();
        assert!(disabled_guard < proxy && proxy < router);
        assert!(branch.contains("mode: ProxyMode::Disabled"));
        assert!(branch.contains("NetworkDesired::management_only()"));
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
            .split("pub(crate) fn apply_dhcp_event")
            .nth(1)
            .unwrap()
            .split("impl DhcpPlatformPort")
            .next()
            .unwrap();
        assert!(!dhcp_apply.contains("NETWORK_LOCK"));
    }
}
