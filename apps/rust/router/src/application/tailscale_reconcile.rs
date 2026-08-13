use crate::{
    application::ports::PlatformError,
    domain::{
        network::{NetworkDesired, NetworkObserved, OwnedResource, Probe},
        tailscale::{
            TailscaleAction, TailscaleBackendState, TailscaleDesired, TailscaleMode,
            TailscaleObserved, TailscalePreferences, TailscaleProcessState,
        },
    },
};

pub fn tailscale_bootstrap_plan(
    observed: &TailscaleObserved,
    token: &str,
) -> Result<Vec<TailscaleAction>, PlatformError> {
    validate_tailscale_observation(observed)?;
    match (&observed.process, &observed.backend_state) {
        (
            Probe::Known(TailscaleProcessState::Absent),
            Probe::Known(TailscaleBackendState::Stopped),
        ) => Ok(vec![
            TailscaleAction::StartBackend {
                token: token.to_owned(),
            },
            TailscaleAction::WaitForBackend,
            TailscaleAction::SetFixedPreferences,
        ]),
        (
            Probe::Known(TailscaleProcessState::OwnedExited { token: stale_token }),
            Probe::Known(TailscaleBackendState::Stopped),
        ) => Ok(vec![
            TailscaleAction::StopBackend {
                token: stale_token.clone(),
            },
            TailscaleAction::StartBackend {
                token: token.to_owned(),
            },
            TailscaleAction::WaitForBackend,
            TailscaleAction::SetFixedPreferences,
        ]),
        (
            Probe::Known(TailscaleProcessState::OwnedLive { .. }),
            Probe::Known(TailscaleBackendState::NeedsLogin | TailscaleBackendState::Running),
        ) => Ok(Vec::new()),
        _ => Err(PlatformError::Conflict(
            "Tailscale process, socket, interface and backend observations are inconsistent"
                .to_owned(),
        )),
    }
}

pub fn tailscale_router_only_runtime_plan(
    observed: &TailscaleObserved,
) -> Result<Vec<TailscaleAction>, PlatformError> {
    validate_tailscale_observation(observed)?;
    let mut actions = Vec::new();
    remove_lan_path(observed, &mut actions)?;
    Ok(actions)
}

pub fn tailscale_shutdown_plan(
    observed: &TailscaleObserved,
) -> Result<Vec<TailscaleAction>, PlatformError> {
    validate_tailscale_observation(observed)?;
    let mut actions = Vec::new();
    remove_lan_path(observed, &mut actions)?;
    remove_router_surface(observed, &mut actions)?;
    match &observed.process {
        Probe::Known(
            TailscaleProcessState::OwnedLive { token }
            | TailscaleProcessState::OwnedExited { token },
        ) => {
            actions.push(TailscaleAction::StopBackend {
                token: token.clone(),
            });
        }
        Probe::Known(TailscaleProcessState::Absent) => {}
        _ => unreachable!("observation validation rejects unsafe process state"),
    }
    Ok(actions)
}

pub fn tailscale_plan(
    desired: &TailscaleDesired,
    observed: &TailscaleObserved,
    network: &NetworkObserved,
    token: &str,
) -> Result<Vec<TailscaleAction>, PlatformError> {
    validate_tailscale_observation(observed)?;
    let previous_mode = match &observed.persisted_mode {
        Probe::Known(mode) => *mode,
        Probe::Unknown(ref reason) => {
            return Err(PlatformError::ProbeFailed(format!(
                "persisted Tailscale mode is unknown: {reason}"
            )))
        }
    };
    let ordinary_router_ready = network.ready_for(&NetworkDesired::forwarding());
    if matches!(
        observed.readiness(desired, ordinary_router_ready),
        crate::domain::tailscale::TailscaleReadiness::Ready { effective_mode }
            if effective_mode == desired.mode
                || (desired.mode == TailscaleMode::LanSubnetAccess
                    && !ordinary_router_ready
                    && effective_mode == TailscaleMode::RouterOnly)
    ) {
        return Ok(Vec::new());
    }

    let mut actions = Vec::new();
    match desired.mode {
        TailscaleMode::Disabled => {
            remove_lan_path(observed, &mut actions)?;
            remove_router_surface(observed, &mut actions)?;
            match &observed.process {
                Probe::Known(
                    TailscaleProcessState::OwnedLive { token }
                    | TailscaleProcessState::OwnedExited { token },
                ) => {
                    actions.push(TailscaleAction::StopBackend {
                        token: token.clone(),
                    });
                }
                Probe::Known(TailscaleProcessState::Absent) => {}
                _ => unreachable!("observation validation rejects unsafe process state"),
            }
        }
        TailscaleMode::RouterOnly | TailscaleMode::LanSubnetAccess => {
            require_authenticated_backend(observed)?;
            remove_lan_path(observed, &mut actions)?;
            ensure_fixed_preferences(observed, &mut actions)?;
            ensure_router_surface(observed, token, &mut actions)?;

            if desired.mode == TailscaleMode::LanSubnetAccess
                && network.ready_for(&NetworkDesired::forwarding())
            {
                actions.push(TailscaleAction::AdvertiseLanRoute);
                actions.push(TailscaleAction::InstallSubnetFirewall {
                    token: token.to_owned(),
                });
            }
        }
    }
    if previous_mode != Some(desired.mode) || !actions.is_empty() {
        actions.push(TailscaleAction::CommitDesiredMode { mode: desired.mode });
    }
    Ok(actions)
}

fn validate_tailscale_observation(observed: &TailscaleObserved) -> Result<(), PlatformError> {
    validate_process(&observed.process)?;
    validate_owned(&observed.socket, "socket")?;
    validate_owned(&observed.interface, "interface")?;
    validate_owned(&observed.router_firewall, "router firewall")?;
    validate_owned(&observed.subnet_firewall, "subnet firewall")?;
    validate_owned(&observed.management_listener, "management listener")?;
    require_known(
        &observed.management_listener_ipv4,
        "management listener IPv4",
    )?;
    require_known(&observed.backend_state, "backend state")?;
    require_known(&observed.authenticated, "authentication state")?;
    require_known(&observed.ipv4, "Tailscale IPv4")?;
    require_known(&observed.preferences, "preferences")?;
    require_known(&observed.route_advertised, "route advertisement")?;

    match (&observed.process, &observed.socket, &observed.interface) {
        (
            Probe::Known(TailscaleProcessState::Absent),
            Probe::Known(OwnedResource::Absent),
            Probe::Known(OwnedResource::Absent),
        ) if observed.backend_state == Probe::Known(TailscaleBackendState::Stopped) => {}
        (
            Probe::Known(TailscaleProcessState::OwnedLive { token: process }),
            Probe::Known(OwnedResource::Owned { token: socket }),
            Probe::Known(OwnedResource::Owned { token: interface }),
        ) if process == socket
            && process == interface
            && matches!(
                &observed.backend_state,
                Probe::Known(TailscaleBackendState::NeedsLogin | TailscaleBackendState::Running)
            ) => {}
        (Probe::Known(TailscaleProcessState::OwnedExited { token }), socket, interface)
            if observed.backend_state == Probe::Known(TailscaleBackendState::Stopped)
                && stale_node_is_absent_or_owned(socket, token)
                && stale_node_is_absent_or_owned(interface, token) => {}
        _ => {
            return Err(PlatformError::Conflict(
                "refusing inconsistent or partially observed Tailscale runtime state".to_owned(),
            ));
        }
    }
    match (
        &observed.management_listener,
        &observed.management_listener_ipv4,
    ) {
        (Probe::Known(OwnedResource::Absent), Probe::Known(None))
        | (Probe::Known(OwnedResource::Owned { .. }), Probe::Known(Some(_))) => {}
        _ => {
            return Err(PlatformError::Conflict(
                "Tailscale management listener ownership and IPv4 are inconsistent".to_owned(),
            ));
        }
    }
    if matches!(
        &observed.backend_state,
        Probe::Known(TailscaleBackendState::Unknown)
    ) {
        return Err(PlatformError::ProbeFailed(
            "Tailscale backend reported an unknown state".to_owned(),
        ));
    }
    Ok(())
}

fn validate_process(process: &Probe<TailscaleProcessState>) -> Result<(), PlatformError> {
    match process {
        Probe::Known(
            TailscaleProcessState::Absent
            | TailscaleProcessState::OwnedLive { .. }
            | TailscaleProcessState::OwnedExited { .. },
        ) => Ok(()),
        Probe::Known(TailscaleProcessState::Foreign) => Err(PlatformError::Conflict(
            "refusing foreign Tailscale process".to_owned(),
        )),
        Probe::Unknown(reason) => Err(PlatformError::ProbeFailed(format!(
            "Tailscale process ownership is unknown: {reason}"
        ))),
    }
}

fn stale_node_is_absent_or_owned(resource: &Probe<OwnedResource>, token: &str) -> bool {
    match resource {
        Probe::Known(OwnedResource::Absent) => true,
        Probe::Known(OwnedResource::Owned {
            token: resource_token,
        }) => resource_token == token,
        _ => false,
    }
}

fn validate_owned(resource: &Probe<OwnedResource>, label: &str) -> Result<(), PlatformError> {
    match resource {
        Probe::Known(OwnedResource::Absent | OwnedResource::Owned { .. }) => Ok(()),
        Probe::Known(OwnedResource::Foreign) => Err(PlatformError::Conflict(format!(
            "refusing foreign Tailscale {label}"
        ))),
        Probe::Unknown(reason) => Err(PlatformError::ProbeFailed(format!(
            "Tailscale {label} ownership is unknown: {reason}"
        ))),
    }
}

fn require_known<T>(probe: &Probe<T>, label: &str) -> Result<(), PlatformError> {
    match probe {
        Probe::Known(_) => Ok(()),
        Probe::Unknown(reason) => Err(PlatformError::ProbeFailed(format!(
            "Tailscale {label} is unknown: {reason}"
        ))),
    }
}

fn require_authenticated_backend(observed: &TailscaleObserved) -> Result<(), PlatformError> {
    if !matches!(
        &observed.process,
        Probe::Known(TailscaleProcessState::OwnedLive { .. })
    ) || observed.backend_state != Probe::Known(TailscaleBackendState::Running)
    {
        return Err(PlatformError::InvalidState(
            "Tailscale backend is not running".to_owned(),
        ));
    }
    if observed.authenticated != Probe::Known(true) {
        return Err(PlatformError::InvalidState(
            "Tailscale authentication is required".to_owned(),
        ));
    }
    if !matches!(&observed.ipv4, Probe::Known(Some(_))) {
        return Err(PlatformError::InvalidState(
            "authenticated Tailscale backend has no confirmed IPv4".to_owned(),
        ));
    }
    Ok(())
}

fn remove_lan_path(
    observed: &TailscaleObserved,
    actions: &mut Vec<TailscaleAction>,
) -> Result<(), PlatformError> {
    if let Probe::Known(OwnedResource::Owned { token }) = &observed.subnet_firewall {
        actions.push(TailscaleAction::RemoveSubnetFirewall {
            token: token.clone(),
        });
    }
    match &observed.route_advertised {
        Probe::Known(true) => actions.push(TailscaleAction::ClearAdvertisedRoute),
        Probe::Known(false) => {}
        Probe::Unknown(_) => unreachable!("observation validation rejects unknown route state"),
    }
    Ok(())
}

fn remove_router_surface(
    observed: &TailscaleObserved,
    actions: &mut Vec<TailscaleAction>,
) -> Result<(), PlatformError> {
    if let Probe::Known(OwnedResource::Owned { token }) = &observed.management_listener {
        actions.push(TailscaleAction::StopManagementListener {
            token: token.clone(),
        });
    }
    if let Probe::Known(OwnedResource::Owned { token }) = &observed.router_firewall {
        actions.push(TailscaleAction::RemoveRouterFirewall {
            token: token.clone(),
        });
    }
    Ok(())
}

fn ensure_fixed_preferences(
    observed: &TailscaleObserved,
    actions: &mut Vec<TailscaleAction>,
) -> Result<(), PlatformError> {
    match &observed.preferences {
        Probe::Known(TailscalePreferences::FIXED) => {}
        Probe::Known(_) => actions.push(TailscaleAction::SetFixedPreferences),
        Probe::Unknown(_) => unreachable!("observation validation rejects unknown preferences"),
    }
    Ok(())
}

fn ensure_router_surface(
    observed: &TailscaleObserved,
    token: &str,
    actions: &mut Vec<TailscaleAction>,
) -> Result<(), PlatformError> {
    let ipv4 = match observed.ipv4 {
        Probe::Known(Some(ipv4)) => ipv4,
        _ => unreachable!("authenticated observation validation requires one IPv4"),
    };
    if observed.router_firewall == Probe::Known(OwnedResource::Absent) {
        actions.push(TailscaleAction::InstallRouterFirewall {
            token: token.to_owned(),
        });
    }
    if let Probe::Known(OwnedResource::Owned { token }) = &observed.management_listener {
        if observed.management_listener_ipv4 != Probe::Known(Some(ipv4)) {
            actions.push(TailscaleAction::StopManagementListener {
                token: token.clone(),
            });
        }
    }
    if observed.management_listener == Probe::Known(OwnedResource::Absent)
        || observed.management_listener_ipv4 != Probe::Known(Some(ipv4))
    {
        actions.push(TailscaleAction::StartManagementListener {
            token: token.to_owned(),
            ipv4,
        });
    }
    Ok(())
}
