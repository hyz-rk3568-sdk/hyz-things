use crate::{
    application::{
        ports::{
            ClockPort, PlatformError, SystemProbePort, TailnetPeerReadPort, TailscalePlatformPort,
            TailscaleProbePort,
        },
        reconcile::{
            tailscale_bootstrap_plan, tailscale_plan, tailscale_router_only_runtime_plan,
            tailscale_shutdown_plan,
        },
    },
    domain::{
        network::{NetworkDesired, OwnedResource, Probe},
        tailscale::{
            TailscaleAction, TailscaleBackendState, TailscaleDesired, TailscaleLoginUrl,
            TailscaleMode, TailscaleObserved, TailscalePeerSnapshot, TailscalePreferences,
            TailscaleProcessState, TailscaleReadiness,
        },
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailscaleReconcileState {
    Ready { effective_mode: TailscaleMode },
    NeedsLogin { login_url: TailscaleLoginUrl },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleReconcileResult {
    pub observed: TailscaleObserved,
    pub state: TailscaleReconcileState,
    pub actions_applied: usize,
}

pub struct ReadTailnetPeers<'a> {
    reader: &'a dyn TailnetPeerReadPort,
}

impl<'a> ReadTailnetPeers<'a> {
    pub const fn new(reader: &'a dyn TailnetPeerReadPort) -> Self {
        Self { reader }
    }

    pub fn execute(&self) -> Result<TailscalePeerSnapshot, PlatformError> {
        self.reader.read_tailnet_peers()
    }
}

pub struct TailscaleApplication<'a> {
    platform: &'a dyn TailscalePlatformPort,
    probe: &'a dyn TailscaleProbePort,
    router_probe: &'a dyn SystemProbePort,
    clock: &'a dyn ClockPort,
}

impl<'a> TailscaleApplication<'a> {
    pub fn new(
        platform: &'a dyn TailscalePlatformPort,
        probe: &'a dyn TailscaleProbePort,
        router_probe: &'a dyn SystemProbePort,
        clock: &'a dyn ClockPort,
    ) -> Self {
        Self {
            platform,
            probe,
            router_probe,
            clock,
        }
    }

    pub fn reconcile(
        &self,
        desired: &TailscaleDesired,
    ) -> Result<TailscaleReconcileResult, PlatformError> {
        let lease = self.platform.acquire_tailscale_lock()?;
        let result = self.reconcile_locked(desired);
        let release = self.platform.release_tailscale_lock(&lease);
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(result), Ok(())) => Ok(result),
        }
    }

    fn reconcile_locked(
        &self,
        desired: &TailscaleDesired,
    ) -> Result<TailscaleReconcileResult, PlatformError> {
        let initial = self.probe.observe_tailscale()?;
        let previous_mode = known_previous_mode(&initial)?;
        let token = self.clock.ownership_token("hyz-tailscale")?;
        let mut observed = initial.clone();
        let mut applied = Vec::new();

        if desired.mode != TailscaleMode::Disabled {
            let bootstrap = tailscale_bootstrap_plan(&observed, &token)?;
            if let Err(error) = self.apply_actions(&bootstrap, &mut applied) {
                self.rollback(&applied, &initial, previous_mode);
                return Err(error);
            }
            if !bootstrap.is_empty() {
                observed = match self.probe.observe_tailscale() {
                    Ok(observed) => observed,
                    Err(error) => {
                        self.rollback(&applied, &initial, previous_mode);
                        return Err(error);
                    }
                };
                if let Err(error) = tailscale_bootstrap_plan(&observed, &token) {
                    self.rollback(&applied, &initial, previous_mode);
                    return Err(error);
                }
            }

            match (&observed.authenticated, &observed.backend_state) {
                (Probe::Known(false), Probe::Known(TailscaleBackendState::NeedsLogin)) => {
                    if has_remote_surface(&observed) {
                        self.rollback(&applied, &initial, previous_mode);
                        return Err(PlatformError::Conflict(
                            "unauthenticated Tailscale state exposes a remote surface".to_owned(),
                        ));
                    }
                    if previous_mode != Some(desired.mode) {
                        let action = TailscaleAction::CommitDesiredMode { mode: desired.mode };
                        if let Err(error) = self.platform.apply_tailscale(&action) {
                            self.rollback(&applied, &initial, previous_mode);
                            return Err(error);
                        }
                        applied.push(action);
                    }
                    let login_url = match self.platform.request_login() {
                        Ok(url) => url,
                        Err(error) => {
                            self.rollback(&applied, &initial, previous_mode);
                            return Err(error);
                        }
                    };
                    return Ok(TailscaleReconcileResult {
                        observed,
                        state: TailscaleReconcileState::NeedsLogin { login_url },
                        actions_applied: applied.len(),
                    });
                }
                (Probe::Known(true), Probe::Known(TailscaleBackendState::Running)) => {}
                (Probe::Unknown(reason), _) | (_, Probe::Unknown(reason)) => {
                    self.rollback(&applied, &initial, previous_mode);
                    return Err(PlatformError::ProbeFailed(format!(
                        "Tailscale authentication readiness is unknown: {reason}"
                    )));
                }
                _ => {
                    self.rollback(&applied, &initial, previous_mode);
                    return Err(PlatformError::InvalidState(
                        "Tailscale backend and authentication state are inconsistent".to_owned(),
                    ));
                }
            }
        }

        let network = match self.router_probe.observe_network() {
            Ok(network) => network,
            Err(error) => {
                self.rollback(&applied, &initial, previous_mode);
                return Err(error);
            }
        };
        let actions = match tailscale_plan(desired, &observed, &network, &token) {
            Ok(actions) => actions,
            Err(error) => {
                self.rollback(&applied, &initial, previous_mode);
                return Err(error);
            }
        };
        for action in &actions {
            if let TailscaleAction::CommitDesiredMode { mode } = action {
                let mut precommit = match self.probe.observe_tailscale() {
                    Ok(observed) => observed,
                    Err(error) => {
                        self.rollback(&applied, &initial, previous_mode);
                        return Err(error);
                    }
                };
                precommit.persisted_mode = Probe::Known(Some(*mode));
                if !matches!(
                    precommit.readiness(desired, network.ready_for(&NetworkDesired::forwarding())),
                    TailscaleReadiness::Ready { .. }
                ) {
                    self.rollback(&applied, &initial, previous_mode);
                    return Err(PlatformError::UnsafeToCutOver(
                        "Tailscale data plane was not ready at desired-mode commit point"
                            .to_owned(),
                    ));
                }
            }
            if let Err(error) = self.platform.apply_tailscale(action) {
                self.rollback(&applied, &initial, previous_mode);
                return Err(error);
            }
            applied.push(action.clone());
        }

        let observed = match self.probe.observe_tailscale() {
            Ok(observed) => observed,
            Err(error) => {
                self.rollback(&applied, &initial, previous_mode);
                return Err(error);
            }
        };
        let state =
            match observed.readiness(desired, network.ready_for(&NetworkDesired::forwarding())) {
                TailscaleReadiness::Ready { effective_mode } => {
                    TailscaleReconcileState::Ready { effective_mode }
                }
                _ => {
                    self.rollback(&applied, &initial, previous_mode);
                    return Err(PlatformError::UnsafeToCutOver(
                        "Tailscale actions completed but strict readiness probe failed".to_owned(),
                    ));
                }
            };
        Ok(TailscaleReconcileResult {
            observed,
            state,
            actions_applied: applied.len(),
        })
    }

    pub fn degrade_to_router_only(&self) -> Result<usize, PlatformError> {
        let lease = self.platform.acquire_tailscale_lock()?;
        let result = (|| {
            let observed = self.probe.observe_tailscale()?;
            let actions = tailscale_router_only_runtime_plan(&observed)?;
            for action in &actions {
                self.platform.apply_tailscale(action)?;
            }
            let final_observed = self.probe.observe_tailscale()?;
            if final_observed.route_advertised != Probe::Known(false)
                || final_observed.subnet_firewall != Probe::Known(OwnedResource::Absent)
            {
                return Err(PlatformError::UnsafeToCutOver(
                    "Tailscale LAN path remained after RouterOnly degradation".to_owned(),
                ));
            }
            Ok(actions.len())
        })();
        finish_locked(result, self.platform.release_tailscale_lock(&lease))
    }

    pub fn shutdown(&self) -> Result<usize, PlatformError> {
        let lease = self.platform.acquire_tailscale_lock()?;
        let result = (|| {
            let observed = self.probe.observe_tailscale()?;
            let persisted_mode = known_previous_mode(&observed)?;
            let actions = tailscale_shutdown_plan(&observed)?;
            for action in &actions {
                self.platform.apply_tailscale(action)?;
            }
            let final_observed = self.probe.observe_tailscale()?;
            if known_previous_mode(&final_observed)? != persisted_mode
                || !runtime_absent(&final_observed)
            {
                return Err(PlatformError::UnsafeToCutOver(
                    "Tailscale shutdown did not preserve desired mode or remove owned runtime"
                        .to_owned(),
                ));
            }
            Ok(actions.len())
        })();
        finish_locked(result, self.platform.release_tailscale_lock(&lease))
    }

    pub fn logout(&self) -> Result<usize, PlatformError> {
        let lease = self.platform.acquire_tailscale_lock()?;
        let result = (|| {
            let mut observed = self.probe.observe_tailscale()?;
            let token = self.clock.ownership_token("hyz-tailscale")?;
            let bootstrap = tailscale_bootstrap_plan(&observed, &token)?;
            for action in &bootstrap {
                self.platform.apply_tailscale(action)?;
            }
            if !bootstrap.is_empty() {
                observed = self.probe.observe_tailscale()?;
            }
            let shutdown = tailscale_shutdown_plan(&observed)?;
            let stop = shutdown
                .iter()
                .position(|action| matches!(action, TailscaleAction::StopBackend { .. }));
            let before_stop = stop.unwrap_or(shutdown.len());
            for action in &shutdown[..before_stop] {
                self.platform.apply_tailscale(action)?;
            }
            if observed.backend_state == Probe::Known(TailscaleBackendState::Running) {
                self.platform.logout()?;
            } else if observed.backend_state != Probe::Known(TailscaleBackendState::NeedsLogin) {
                return Err(PlatformError::InvalidState(
                    "Tailscale backend is not in a logout-capable state".to_owned(),
                ));
            }
            for action in &shutdown[before_stop..] {
                self.platform.apply_tailscale(action)?;
            }
            self.platform
                .apply_tailscale(&TailscaleAction::CommitDesiredMode {
                    mode: TailscaleMode::Disabled,
                })?;
            let final_observed = self.probe.observe_tailscale()?;
            if final_observed.persisted_mode != Probe::Known(Some(TailscaleMode::Disabled))
                || !runtime_absent(&final_observed)
            {
                return Err(PlatformError::UnsafeToCutOver(
                    "Tailscale logout did not confirm disabled runtime state".to_owned(),
                ));
            }
            Ok(bootstrap.len() + shutdown.len() + 1)
        })();
        finish_locked(result, self.platform.release_tailscale_lock(&lease))
    }

    fn apply_actions(
        &self,
        actions: &[TailscaleAction],
        applied: &mut Vec<TailscaleAction>,
    ) -> Result<(), PlatformError> {
        for action in actions {
            self.platform.apply_tailscale(action)?;
            applied.push(action.clone());
        }
        Ok(())
    }

    fn rollback(
        &self,
        applied: &[TailscaleAction],
        initial: &TailscaleObserved,
        previous_mode: Option<TailscaleMode>,
    ) {
        for action in applied.iter().rev() {
            for compensation in rollback_compensations(action, initial, previous_mode) {
                let _ = self.platform.apply_tailscale(&compensation);
            }
        }
    }
}

fn finish_locked<T>(
    result: Result<T, PlatformError>,
    release: Result<(), PlatformError>,
) -> Result<T, PlatformError> {
    match (result, release) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(result), Ok(())) => Ok(result),
    }
}

fn runtime_absent(observed: &TailscaleObserved) -> bool {
    observed.backend_state == Probe::Known(TailscaleBackendState::Stopped)
        && observed.process == Probe::Known(TailscaleProcessState::Absent)
        && observed.socket == Probe::Known(OwnedResource::Absent)
        && observed.interface == Probe::Known(OwnedResource::Absent)
        && observed.route_advertised == Probe::Known(false)
        && observed.router_firewall == Probe::Known(OwnedResource::Absent)
        && observed.subnet_firewall == Probe::Known(OwnedResource::Absent)
        && observed.management_listener == Probe::Known(OwnedResource::Absent)
        && observed.management_listener_ipv4 == Probe::Known(None)
}

fn known_previous_mode(
    observed: &TailscaleObserved,
) -> Result<Option<TailscaleMode>, PlatformError> {
    match &observed.persisted_mode {
        Probe::Known(mode) => Ok(*mode),
        Probe::Unknown(ref reason) => Err(PlatformError::ProbeFailed(format!(
            "persisted Tailscale mode is unknown: {reason}"
        ))),
    }
}

fn has_remote_surface(observed: &TailscaleObserved) -> bool {
    observed.route_advertised != Probe::Known(false)
        || observed.router_firewall != Probe::Known(OwnedResource::Absent)
        || observed.subnet_firewall != Probe::Known(OwnedResource::Absent)
        || observed.management_listener != Probe::Known(OwnedResource::Absent)
}

fn rollback_compensations(
    action: &TailscaleAction,
    initial: &TailscaleObserved,
    previous_mode: Option<TailscaleMode>,
) -> Vec<TailscaleAction> {
    match action {
        TailscaleAction::StartBackend { token, .. } => vec![TailscaleAction::StopBackend {
            token: token.clone(),
        }],
        TailscaleAction::StopBackend { token }
            if initial.process
                == Probe::Known(TailscaleProcessState::OwnedLive {
                    token: token.clone(),
                }) =>
        {
            vec![
                TailscaleAction::StartBackend {
                    token: token.clone(),
                    environment: crate::domain::tailscale::TailscaleEnvironment::Direct,
                },
                TailscaleAction::WaitForBackend,
            ]
        }
        TailscaleAction::SetFixedPreferences => match &initial.preferences {
            Probe::Known(preferences) if *preferences != TailscalePreferences::FIXED => {
                vec![TailscaleAction::RestorePreferences {
                    preferences: *preferences,
                }]
            }
            _ => Vec::new(),
        },
        TailscaleAction::AdvertiseLanRoute => vec![TailscaleAction::ClearAdvertisedRoute],
        TailscaleAction::ClearAdvertisedRoute if initial.route_advertised == Probe::Known(true) => {
            vec![TailscaleAction::AdvertiseLanRoute]
        }
        TailscaleAction::InstallRouterFirewall { token } => {
            vec![TailscaleAction::RemoveRouterFirewall {
                token: token.clone(),
            }]
        }
        TailscaleAction::RemoveRouterFirewall { token } => {
            vec![TailscaleAction::InstallRouterFirewall {
                token: token.clone(),
            }]
        }
        TailscaleAction::InstallSubnetFirewall { token } => {
            vec![TailscaleAction::RemoveSubnetFirewall {
                token: token.clone(),
            }]
        }
        TailscaleAction::RemoveSubnetFirewall { token } => {
            vec![TailscaleAction::InstallSubnetFirewall {
                token: token.clone(),
            }]
        }
        TailscaleAction::StartManagementListener { token, .. } => {
            vec![TailscaleAction::StopManagementListener {
                token: token.clone(),
            }]
        }
        TailscaleAction::StopManagementListener { token } => {
            vec![TailscaleAction::StartManagementListener {
                token: token.clone(),
                ipv4: match &initial.management_listener_ipv4 {
                    Probe::Known(Some(ipv4)) => *ipv4,
                    _ => return Vec::new(),
                },
            }]
        }
        TailscaleAction::CommitDesiredMode { .. } => {
            vec![TailscaleAction::RestoreDesiredMode {
                mode: previous_mode,
            }]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tailscale::TailscalePeer;
    use std::net::Ipv4Addr;

    struct PeerReader {
        result: Result<TailscalePeerSnapshot, PlatformError>,
    }

    impl TailnetPeerReadPort for PeerReader {
        fn read_tailnet_peers(&self) -> Result<TailscalePeerSnapshot, PlatformError> {
            self.result.clone()
        }
    }

    #[test]
    fn peer_read_use_case_returns_typed_snapshot_without_lifecycle_mutation() {
        let snapshot = TailscalePeerSnapshot::new(vec![TailscalePeer::new(
            "laptop",
            Ipv4Addr::new(100, 64, 0, 8),
            true,
            None,
        )
        .unwrap()])
        .unwrap();
        let reader = PeerReader {
            result: Ok(snapshot.clone()),
        };
        assert_eq!(ReadTailnetPeers::new(&reader).execute().unwrap(), snapshot);

        let reader = PeerReader {
            result: Err(PlatformError::ProbeFailed("unavailable".to_owned())),
        };
        assert_eq!(
            ReadTailnetPeers::new(&reader).execute(),
            Err(PlatformError::ProbeFailed("unavailable".to_owned()))
        );
    }
}
