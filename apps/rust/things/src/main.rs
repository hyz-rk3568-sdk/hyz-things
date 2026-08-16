use hyz_things::{
    adapters::{
        inbound::http::{
            app_with_admin_camera_control, bind_fixed_lan_with_retry, DEFAULT_BIND_ATTEMPTS,
            DEFAULT_HTTP_PORT, LAN_ADDRESS,
        },
        outbound::{
            admin::AdminFileAdapter,
            camera::CameraUnixAdapter,
            registry::RegistryAdapter,
            router::RouterControlClient,
            tailscale::{fetch_tailscale_status, PortalListenerApp, TailscaleListenerManager},
        },
    },
    application::{
        admin::AdminApplication,
        camera::CameraApplication,
        ports::{ClockPort, PlatformError, PortalControlHandler},
        status::PortalStatus,
    },
};
use std::{error::Error, os::unix::fs::MetadataExt, sync::Arc, time::Duration};

const ROUTER_READY_MARKER: &str = "/run/hyz-router/ready";
const ROUTER_READY_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const ROUTER_READY_POLL: Duration = Duration::from_secs(1);
const TAILSCALE_RECONCILE_INTERVAL: Duration = Duration::from_secs(2);

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args();
    let _program = arguments.next();
    if arguments.next().as_deref() != Some("daemon") || arguments.next().is_some() {
        return Err("Usage: hyz-things daemon".into());
    }
    if unsafe { libc::geteuid() } != 0 {
        return Err("hyz-things must run as root".into());
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run_daemon())?;
    Ok(())
}

struct SystemClock;

impl ClockPort for SystemClock {
    fn unix_time_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }
}

async fn wait_for_router_ready() -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + ROUTER_READY_TIMEOUT;
    loop {
        match std::fs::symlink_metadata(ROUTER_READY_MARKER) {
            Ok(metadata)
                if metadata.uid() == 0
                    && !metadata.file_type().is_symlink()
                    && metadata.file_type().is_file() =>
            {
                return Ok(());
            }
            _ if tokio::time::Instant::now() >= deadline => {
                return Err(format!(
                    "router core did not confirm management readiness within {ROUTER_READY_TIMEOUT:?}"
                ));
            }
            _ => tokio::time::sleep(ROUTER_READY_POLL).await,
        }
    }
}

fn csrf_token() -> Result<String, PlatformError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| PlatformError::Io("secure random source is unavailable".to_owned()))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

async fn run_daemon() -> Result<(), Box<dyn Error>> {
    wait_for_router_ready().await?;

    // The portal owns the administrator credential store (same root-only path
    // as before the headless split so OTA preserves the credential).
    let admin_adapter = Arc::new(AdminFileAdapter::default());
    let admin = Arc::new(AdminApplication::initialize(
        admin_adapter.clone(),
        admin_adapter,
        Arc::new(SystemClock),
    )?);
    let camera = Arc::new(CameraApplication::new(Arc::new(
        CameraUnixAdapter::default(),
    )));
    let control: Arc<dyn PortalControlHandler> = Arc::new(RouterControlClient);
    let status = PortalStatus::new(control.clone());
    let csrf_token = csrf_token()?;
    let port = std::env::var("HYZ_THINGS_HTTP_PORT")
        .map(|value| value.parse::<u16>())
        .unwrap_or(Ok(DEFAULT_HTTP_PORT))?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let mut shutdown_rx = shutdown_rx;
    let mut signals = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    let tailscale = TailscaleListenerManager::new();
    let tailscale_app = PortalListenerApp {
        status: status.clone(),
        control: control.clone(),
        admin: admin.clone(),
        camera: camera.clone(),
        csrf_token: csrf_token.clone(),
        runtime: tokio::runtime::Handle::current(),
        clock: Arc::new(SystemClock),
    };
    let tailscale_manager = tailscale.clone();
    let tailscale_control = control.clone();
    let mut tailscale_shutdown = shutdown_rx.clone();
    let tailscale_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(TAILSCALE_RECONCILE_INTERVAL);
        interval.tick().await;
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match fetch_tailscale_status(tailscale_control.as_ref()).await {
                        Ok(status) => {
                            if let Err(error) = tailscale_manager.reconcile(&status, tailscale_app.clone()) {
                                eprintln!("hyz-things: Tailscale management listener reconcile failed: {error}");
                            }
                        }
                        Err(error) => {
                            eprintln!("hyz-things: Tailscale status unavailable: {error}");
                        }
                    }
                }
                changed = tailscale_shutdown.changed() => {
                    let _ = changed;
                    break;
                }
            }
        }
    });

    // Binding the fixed LAN address is the externally visible management
    // readiness boundary, gated on the router's confirmed readiness above.
    let bind = bind_fixed_lan_with_retry(port, DEFAULT_BIND_ATTEMPTS, Duration::from_secs(1));
    let listener = tokio::select! {
        result = bind => result?,
        _ = signals.recv() => return Ok(()),
        _ = interrupt.recv() => return Ok(()),
    };
    let app = app_with_admin_camera_control(
        status,
        control,
        admin,
        camera,
        csrf_token,
        port,
        Some(Arc::new(RegistryAdapter::default())),
    );
    eprintln!("hyz-things: management portal ready at http://{LAN_ADDRESS}:{port}");

    tokio::select! {
        result = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                while !*shutdown_rx.borrow() && shutdown_rx.changed().await.is_ok() {}
            }) => {
            result?;
        }
        _ = signals.recv() => {}
        _ = interrupt.recv() => {}
    }

    let _ = shutdown_tx.send(true);
    let _ = tailscale_task.await;
    match tailscale.stop_all() {
        Ok(()) => Ok(()),
        Err(error) => Err(format!("Tailscale management listener cleanup failed: {error}").into()),
    }
}
