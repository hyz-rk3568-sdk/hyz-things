use crate::{
    application::{
        ports::{ClockPort, PlatformError, RouterPlatformPort, SystemProbePort},
        reconcile::proxy_plan,
    },
    domain::{
        network::Probe,
        proxy::{ProxyAction, ProxyDesired, ProxyMode, ProxyObserved},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyReconcileResult {
    pub observed: ProxyObserved,
    pub actions_applied: usize,
}

pub struct ProxyApplication<'a> {
    platform: &'a dyn RouterPlatformPort,
    probe: &'a dyn SystemProbePort,
    clock: &'a dyn ClockPort,
}

impl<'a> ProxyApplication<'a> {
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

    fn rollback(&self, applied: &[ProxyAction], previous_mode: Option<ProxyMode>) {
        let restore_mode = applied
            .iter()
            .any(|action| matches!(action, ProxyAction::CommitMode { .. }));
        for action in applied.iter().rev() {
            if let Some(compensation) = rollback_compensation(action) {
                let _ = self.platform.apply_proxy(&compensation);
            }
        }
        if restore_mode {
            let _ = self
                .platform
                .apply_proxy(&ProxyAction::RestorePersistedMode {
                    mode: previous_mode,
                });
        }
    }

    pub fn reconcile(&self, desired: &ProxyDesired) -> Result<ProxyReconcileResult, PlatformError> {
        let lease = self.platform.acquire_lifecycle_lock()?;
        let result = (|| {
            let network = self.probe.observe_network()?;
            let observed = self.probe.observe_proxy()?;
            let previous_mode = match &observed.persisted_mode {
                Probe::Known(mode) => *mode,
                Probe::Unknown(ref reason) => {
                    return Err(PlatformError::ProbeFailed(format!(
                        "persisted proxy mode is unknown: {reason}"
                    )))
                }
            };
            let token = self.clock.ownership_token("hyz-mihomo")?;
            let actions = proxy_plan(desired, &observed, &network, &token)?;
            let mut applied = Vec::new();
            for action in &actions {
                if let ProxyAction::CommitMode { mode } = action {
                    let mut precommit = match self.probe.observe_proxy() {
                        Ok(observed) => observed,
                        Err(error) => {
                            self.rollback(&applied, previous_mode);
                            return Err(error);
                        }
                    };
                    precommit.persisted_mode = crate::domain::network::Probe::Known(Some(*mode));
                    if !precommit.ready_for(desired) {
                        self.rollback(&applied, previous_mode);
                        return Err(PlatformError::UnsafeToCutOver(
                            "proxy data plane was not ready at persistent-mode commit point"
                                .to_owned(),
                        ));
                    }
                }
                if let Err(error) = self.platform.apply_proxy(action) {
                    self.rollback(&applied, previous_mode);
                    return Err(error);
                }
                applied.push(action.clone());
            }
            let observed = match self.probe.observe_proxy() {
                Ok(observed) => observed,
                Err(error) => {
                    self.rollback(&applied, previous_mode);
                    return Err(error);
                }
            };
            if !observed.ready_for(desired) {
                self.rollback(&applied, previous_mode);
                return Err(PlatformError::UnsafeToCutOver(
                    "proxy actions completed but strict readiness probe failed".to_owned(),
                ));
            }
            Ok(ProxyReconcileResult {
                observed,
                actions_applied: actions.len(),
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

fn rollback_compensation(action: &ProxyAction) -> Option<ProxyAction> {
    match action {
        ProxyAction::InstallInterceptionEntry { token } => {
            Some(ProxyAction::RemoveInterceptionEntry {
                token: token.clone(),
            })
        }
        ProxyAction::InstallPolicyRule => Some(ProxyAction::RemovePolicyRule),
        ProxyAction::InstallPolicyRoute => Some(ProxyAction::RemovePolicyRoute),
        ProxyAction::InstallTunForwardHook { token } => Some(ProxyAction::RemoveTunForwardHook {
            token: token.clone(),
        }),
        ProxyAction::CreateTunChains { token } => Some(ProxyAction::RemoveTunChains {
            token: token.clone(),
        }),
        ProxyAction::StartWatcher => Some(ProxyAction::StopWatcher),
        ProxyAction::StartCore => Some(ProxyAction::StopCore),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_stops_watcher_before_interception_and_core_cleanup() {
        let applied = [
            ProxyAction::StartCore,
            ProxyAction::InstallInterceptionEntry {
                token: "owned".to_owned(),
            },
            ProxyAction::StartWatcher,
        ];
        let compensations = applied
            .iter()
            .rev()
            .filter_map(rollback_compensation)
            .collect::<Vec<_>>();
        assert_eq!(
            compensations,
            vec![
                ProxyAction::StopWatcher,
                ProxyAction::RemoveInterceptionEntry {
                    token: "owned".to_owned()
                },
                ProxyAction::StopCore,
            ]
        );
    }
}
