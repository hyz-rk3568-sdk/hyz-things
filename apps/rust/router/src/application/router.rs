use crate::{
    application::{
        ports::{ClockPort, PlatformError, RouterPlatformPort, SystemProbePort},
        reconcile::{forwarding_plan, management_plan},
    },
    domain::network::{ForwardingDesired, NetworkAction, NetworkDesired, NetworkObserved, Probe},
};

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
                NetworkAction::EnsureManagementServices => {
                    Some(NetworkAction::StopManagementServices)
                }
                NetworkAction::CaptureIpv4Forwarding => Some(NetworkAction::RestoreIpv4Forwarding),
                NetworkAction::EnableIpv4Forwarding => Some(NetworkAction::DisableIpv4Forwarding),
                NetworkAction::InstallRouterFirewall { token } => {
                    Some(NetworkAction::RemoveRouterFirewall {
                        token: token.clone(),
                    })
                }
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
        let lease = self.platform.acquire_lifecycle_lock()?;
        let result = (|| {
            let initial = self.probe.observe_network()?;
            let token = self.clock.ownership_token("hyz-router")?;
            let management_actions = management_plan(&initial, &token)?;
            let mut applied = Vec::new();
            if let Err(error) = self.apply_phase(&management_actions, &mut applied) {
                self.rollback(&applied);
                return Err(error);
            }

            // LAN/AP health is independent of STA association and DHCP. This probe must succeed
            // before either a management-only startup can complete or forwarding can be gated.
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

            // Only forwarding waits for an upstream lease. The wait is bounded by the outbound
            // adapter and followed by an exact ownership reprobe before any firewall commit.
            if desired.forwarding == ForwardingDesired::Enabled {
                match &managed.wan_default_route_present {
                    Probe::Known(true) => {}
                    Probe::Known(false) => {
                        if let Err(error) =
                            self.apply_phase(&[NetworkAction::WaitForWanRoute], &mut applied)
                        {
                            self.rollback(&applied);
                            return Err(error);
                        }
                        managed = match self.probe.observe_network() {
                            Ok(observed) => observed,
                            Err(error) => {
                                self.rollback(&applied);
                                return Err(error);
                            }
                        };
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
                actions_applied: applied.len(),
            })
        })();
        let release = self.platform.release_lifecycle_lock(&lease);
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(result), Ok(())) => Ok(result),
        }
    }
}
