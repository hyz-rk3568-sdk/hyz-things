use crate::{
    application::{
        ports::{ClockPort, LifecycleLease, PlatformError, RouterPlatformPort, SystemProbePort},
        reconcile::{forwarding_plan, management_plan},
    },
    domain::network::{ForwardingDesired, NetworkAction, NetworkDesired, NetworkObserved, Probe},
};

use std::{
    thread,
    time::{Duration, Instant},
};

const LIFECYCLE_REACQUIRE_WAIT: Duration = Duration::from_secs(5);
const LIFECYCLE_REACQUIRE_RETRY: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterReconcileResult {
    pub observed: NetworkObserved,
    pub actions_applied: usize,
}

pub struct RouterApplication<'a> {
    platform: &'a dyn RouterPlatformPort,
    probe: &'a dyn SystemProbePort,
    clock: &'a dyn ClockPort,
}

impl<'a> RouterApplication<'a> {
    pub fn new(
        platform: &'a dyn RouterPlatformPort,
        probe: &'a dyn SystemProbePort,
        clock: &'a dyn ClockPort,
    ) -> Self {
        Self {
            platform,
            probe,
            clock,
        }
    }

    fn rollback(&self, applied: &[NetworkAction]) {
        for action in applied.iter().rev() {
            let compensation = match action {
                NetworkAction::EnsureOwnedBridge { token } => {
                    Some(NetworkAction::RemoveOwnedBridge {
                        token: token.clone(),
                    })
                }
                NetworkAction::ConfigureBridge => Some(NetworkAction::SetBridgeDown),
                NetworkAction::AssignLanAddress => Some(NetworkAction::RemoveLanAddress),
                NetworkAction::AttachAp => Some(NetworkAction::DetachAp),
                NetworkAction::AttachEthernetLan => Some(NetworkAction::DetachEthernetLan),
                NetworkAction::EnsureManagementServices => {
                    Some(NetworkAction::StopManagementServices)
                }
                NetworkAction::CaptureIpv4Forwarding => Some(NetworkAction::RestoreIpv4Forwarding),
                NetworkAction::EnableIpv4Forwarding => Some(NetworkAction::DisableIpv4Forwarding),
                NetworkAction::InstallRouterFirewall { token, .. } => {
                    Some(NetworkAction::RemoveRouterFirewall {
                        token: token.clone(),
                    })
                }
                NetworkAction::ReconfigureRouterFirewall {
                    token,
                    previous_wan_set,
                    wan_set,
                } => Some(NetworkAction::ReconfigureRouterFirewall {
                    token: token.clone(),
                    previous_wan_set: *wan_set,
                    wan_set: *previous_wan_set,
                }),
                _ => None,
            };
            if let Some(compensation) = compensation {
                let _ = self.platform.apply_network(&compensation);
            }
        }
    }

    fn apply_phase(
        &self,
        actions: &[NetworkAction],
        applied: &mut Vec<NetworkAction>,
    ) -> Result<(), PlatformError> {
        for action in actions {
            self.platform.apply_network(action)?;
            applied.push(action.clone());
        }
        Ok(())
    }

    pub fn reconcile(
        &self,
        desired: &NetworkDesired,
    ) -> Result<RouterReconcileResult, PlatformError> {
        let mut lease = Some(self.platform.acquire_lifecycle_lock()?);
        let result = self.reconcile_with_lease(desired, &mut lease);
        match (result, release_optional(self.platform, lease)) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(result), Ok(())) => Ok(result),
        }
    }

    fn reconcile_with_lease(
        &self,
        desired: &NetworkDesired,
        lease: &mut Option<LifecycleLease>,
    ) -> Result<RouterReconcileResult, PlatformError> {
        let initial = self.probe.observe_network()?;
        let token = self.clock.ownership_token("hyz-router")?;
        let management_actions = management_plan(&initial, &token)?;
        let mut applied = Vec::new();
        if let Err(error) = self.apply_phase(&management_actions, &mut applied) {
            self.rollback(&applied);
            return Err(error);
        }

        let mut managed = match self.probe.observe_network() {
            Ok(observed) => observed,
            Err(error) => {
                self.rollback(&applied);
                return Err(error);
            }
        };
        if !managed.management_ready() {
            self.rollback(&applied);
            return Err(PlatformError::UnsafeToCutOver(
                "management phase completed but strict management probe failed".to_owned(),
            ));
        }
        let management_actions_applied = applied.len();
        // Strictly confirmed management is the fail-open commit point. Later WAN-gate or
        // forwarding failures must retain it and may only compensate post-gate actions.
        applied.clear();

        if desired.forwarding == ForwardingDesired::Enabled {
            match managed.active_uplink_probe() {
                Probe::Known(Some(_)) => {}
                Probe::Known(None) => {
                    let held = lease.take().ok_or_else(|| {
                        PlatformError::InvalidState("router lifecycle lease is absent".to_owned())
                    })?;
                    self.platform.release_lifecycle_lock(&held)?;

                    let wait = self.platform.apply_network(&NetworkAction::WaitForWanRoute);
                    let reacquired = self.reacquire_lifecycle_lock_bounded()?;
                    *lease = Some(reacquired);
                    if let Err(error) = wait {
                        self.rollback(&applied);
                        return Err(error);
                    }
                    applied.push(NetworkAction::WaitForWanRoute);

                    managed = match self.probe.observe_network() {
                        Ok(observed) => observed,
                        Err(error) => {
                            self.rollback(&applied);
                            return Err(error);
                        }
                    };
                    let management_unchanged = match management_plan(&managed, &token) {
                        Ok(actions) => actions.is_empty(),
                        Err(error) => {
                            self.rollback(&applied);
                            return Err(error);
                        }
                    };
                    if !managed.management_ready()
                        || !matches!(managed.router_wan_set(), Probe::Known(Some(_)))
                        || !management_unchanged
                    {
                        self.rollback(&applied);
                        return Err(PlatformError::UnsafeToCutOver(
                            "network state changed while the WAN route wait released the lifecycle lock"
                                .to_owned(),
                        ));
                    }
                }
                Probe::Unknown(reason) => {
                    self.rollback(&applied);
                    return Err(PlatformError::ProbeFailed(format!(
                        "WAN route readiness is unknown: {reason}"
                    )));
                }
            }
        }

        let forwarding_actions = match forwarding_plan(desired, &managed, &token) {
            Ok(actions) => actions,
            Err(error) => {
                self.rollback(&applied);
                return Err(error);
            }
        };
        if let Err(error) = self.apply_phase(&forwarding_actions, &mut applied) {
            self.rollback(&applied);
            return Err(error);
        }

        let observed = match self.probe.observe_network() {
            Ok(observed) => observed,
            Err(error) => {
                self.rollback(&applied);
                return Err(error);
            }
        };
        if !observed.ready_for(desired) {
            self.rollback(&applied);
            return Err(PlatformError::UnsafeToCutOver(
                "network actions completed but strict readiness probe failed".to_owned(),
            ));
        }
        Ok(RouterReconcileResult {
            observed,
            actions_applied: management_actions_applied + applied.len(),
        })
    }

    fn reacquire_lifecycle_lock_bounded(&self) -> Result<LifecycleLease, PlatformError> {
        let deadline = Instant::now() + LIFECYCLE_REACQUIRE_WAIT;
        loop {
            match self.platform.acquire_lifecycle_lock() {
                Ok(lease) => return Ok(lease),
                Err(PlatformError::Busy(_)) if Instant::now() < deadline => {
                    thread::sleep(LIFECYCLE_REACQUIRE_RETRY);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

fn release_optional(
    platform: &dyn RouterPlatformPort,
    lease: Option<LifecycleLease>,
) -> Result<(), PlatformError> {
    match lease {
        Some(lease) => platform.release_lifecycle_lock(&lease),
        None => Ok(()),
    }
}
