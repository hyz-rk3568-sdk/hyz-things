//! Exact Tailscale IPv4 management listener for the portal.
//!
//! The router core no longer owns any HTTP; the portal binds the exact
//! observed Tailscale IPv4 on the fixed management port and serves the same
//! typed app with `allow_tailscale_self_stop = false`, so Tailscale
//! disable/logout and proxy feature mutations are refused from the remote
//! management listener itself.

use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{mpsc as std_mpsc, Arc, Mutex as StdMutex},
    time::Duration,
};

use hyz_contract::router::{ControlOperation, ControlResult};
use socket2::{Domain, Protocol, Socket, Type};

use crate::{
    adapters::inbound::http::app_with_admin_camera_control_at_address,
    application::{
        admin::AdminApplication,
        camera::CameraApplication,
        ports::{ClockPort, PlatformError, PortalControlHandler},
        status::PortalStatus,
    },
    domain::{status::TailscaleStatus, tailscale::TAILSCALE_MANAGEMENT_HTTP_PORT},
};

const ACCEPT_STOP_TIMEOUT: Duration = Duration::from_secs(1);
const GRACEFUL_JOIN_TIMEOUT: Duration = Duration::from_millis(250);
const ABORT_REAP_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub struct TailscaleListenerManager {
    state: Arc<StdMutex<Option<RunningTailscaleListener>>>,
}

struct RunningTailscaleListener {
    token: String,
    ipv4: Ipv4Addr,
    runtime: tokio::runtime::Handle,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    accept_stopped: std_mpsc::Receiver<()>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl Default for TailscaleListenerManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TailscaleListenerManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(StdMutex::new(None)),
        }
    }

    pub fn observe(&self) -> Option<Ipv4Addr> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let running = state.as_mut()?;
        if running.task.is_finished() {
            let running = state.take().expect("finished listener must exist");
            drop(state);
            running.reap_finished();
            return None;
        }
        running.shutdown.as_ref()?;
        Some(running.ipv4)
    }

    pub fn reconcile(
        &self,
        status: &TailscaleStatus,
        app: PortalListenerApp,
    ) -> Result<(), PlatformError> {
        let desired = if status.authenticated == Some(true) {
            status.ipv4
        } else {
            None
        };
        let current = self.observe();
        match (desired, current) {
            (Some(ipv4), Some(bound)) if ipv4 == bound => Ok(()),
            (Some(ipv4), Some(bound)) => {
                self.stop(&bound.to_string())?;
                self.start(ipv4, app)
            }
            (Some(ipv4), None) => self.start(ipv4, app),
            (None, Some(bound)) => {
                self.stop(&bound.to_string())?;
                Ok(())
            }
            (None, None) => Ok(()),
        }
    }

    fn start(&self, ipv4: Ipv4Addr, app: PortalListenerApp) -> Result<(), PlatformError> {
        let mut state = self.state.lock().map_err(|_| {
            PlatformError::InvalidState("Tailscale listener lock poisoned".to_owned())
        })?;
        if state.is_some() {
            return Err(PlatformError::Conflict(
                "Tailscale management listener already exists".to_owned(),
            ));
        }
        let token = app.clock.ownership_token("hyz-things-tailscale")?;
        let address = SocketAddr::from((ipv4, TAILSCALE_MANAGEMENT_HTTP_PORT));
        let listener = bind_exact_tailscale_listener(address).map_err(|error| {
            PlatformError::Io(format!(
                "bind exact Tailscale HTTP listener {address}: {error}"
            ))
        })?;
        let listener = {
            let _enter = app.runtime.enter();
            tokio::net::TcpListener::from_std(listener).map_err(|error| {
                PlatformError::Io(format!("adopt exact Tailscale HTTP listener: {error}"))
            })?
        };
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let (accept_stopped, accept_stopped_rx) = std_mpsc::channel();
        let listener = ConfirmedTailscaleListener {
            listener,
            accept_stopped: Some(accept_stopped),
        };
        let http_app = app_with_admin_camera_control_at_address(
            app.status,
            app.control,
            app.admin,
            app.camera,
            app.csrf_token,
            ipv4,
            TAILSCALE_MANAGEMENT_HTTP_PORT,
        );
        let runtime = app.runtime.clone();
        let task = runtime.spawn(async move {
            axum::serve(listener, http_app)
                .with_graceful_shutdown(async move {
                    let _ = stopped.await;
                })
                .await
        });
        *state = Some(RunningTailscaleListener {
            token,
            ipv4,
            runtime,
            shutdown: Some(shutdown),
            accept_stopped: accept_stopped_rx,
            task,
        });
        Ok(())
    }

    fn stop(&self, token: &str) -> Result<(), PlatformError> {
        let mut state = self.state.lock().map_err(|_| {
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
            .recv_timeout(ACCEPT_STOP_TIMEOUT)
            .is_ok();
        if accept_confirmed {
            if let Some(result) = running.wait_for_task(GRACEFUL_JOIN_TIMEOUT) {
                RunningTailscaleListener::log_completion(result);
                return Ok(());
            }
        }

        running.task.abort();
        if let Some(result) = running.wait_for_task(ABORT_REAP_TIMEOUT) {
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

        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        *state = Some(running);
        Err(PlatformError::UnsafeToCutOver(
            "Tailscale management listener task did not stop within the bounded abort/reap window"
                .to_owned(),
        ))
    }

    pub fn stop_all(&self) -> Result<(), PlatformError> {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        let Some(mut running) = state.take() else {
            return Ok(());
        };
        drop(state);
        if let Some(shutdown) = running.shutdown.take() {
            let _ = shutdown.send(());
        } else {
            running.task.abort();
        }
        let accept_confirmed = running
            .accept_stopped
            .recv_timeout(ACCEPT_STOP_TIMEOUT)
            .is_ok();
        if accept_confirmed {
            if let Some(result) = running.wait_for_task(GRACEFUL_JOIN_TIMEOUT) {
                RunningTailscaleListener::log_completion(result);
                return Ok(());
            }
        }
        running.task.abort();
        if let Some(result) = running.wait_for_task(ABORT_REAP_TIMEOUT) {
            RunningTailscaleListener::log_completion(result);
            return Ok(());
        }
        Err(PlatformError::UnsafeToCutOver(
            "Tailscale management listener did not stop within the bounded window".to_owned(),
        ))
    }
}

#[derive(Clone)]
pub struct PortalListenerApp {
    pub status: PortalStatus,
    pub control: Arc<dyn PortalControlHandler>,
    pub admin: Arc<AdminApplication>,
    pub camera: Arc<CameraApplication>,
    pub csrf_token: String,
    pub runtime: tokio::runtime::Handle,
    pub clock: Arc<dyn ClockPort>,
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
                eprintln!("hyz-things: Tailscale management listener failed: {error}");
            }
            Err(error) if error.is_cancelled() => {}
            Err(error) => {
                eprintln!("hyz-things: Tailscale management listener task failed: {error}");
            }
        }
    }

    fn reap_finished(mut self) {
        let runtime = self.runtime.clone();
        Self::log_completion(runtime.block_on(&mut self.task));
    }
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

pub async fn fetch_tailscale_status(
    control: &dyn PortalControlHandler,
) -> Result<TailscaleStatus, String> {
    match control.handle(ControlOperation::TailscaleGet {}).await? {
        ControlResult::Tailscale { status } => Ok(status),
        _ => Err("Tailscale status contract mismatch".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_port_is_the_management_port() {
        assert_eq!(TAILSCALE_MANAGEMENT_HTTP_PORT, 8080);
    }
}
