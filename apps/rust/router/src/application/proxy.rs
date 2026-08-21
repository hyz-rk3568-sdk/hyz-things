use crate::{
    application::{
        ports::{ClockPort, PlatformError, RouterPlatformPort, SystemProbePort},
        reconcile::proxy_plan,
    },
    domain::{
        network::Probe,
        proxy::{ProxyAction, ProxyDesired, ProxyFeaturesV1, ProxyObserved},
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

    fn rollback(&self, applied: &[ProxyAction], previous: ProxyFeaturesV1) {
        let restore_features = applied
            .iter()
            .any(|action| matches!(action, ProxyAction::CommitFeatures { .. }));
        for action in applied.iter().rev() {
            if let Some(compensation) = rollback_compensation(action) {
                let _ = self.platform.apply_proxy(&compensation);
            }
        }
        if restore_features {
            let _ = self
                .platform
                .apply_proxy(&ProxyAction::RestorePersistedFeatures { features: previous });
        }
    }

    pub fn reconcile(&self, desired: &ProxyDesired) -> Result<ProxyReconcileResult, PlatformError> {
        self.reconcile_with_persistence(desired, true)
    }

    pub fn reconcile_runtime_preserving_features(
        &self,
        desired: &ProxyDesired,
    ) -> Result<ProxyReconcileResult, PlatformError> {
        self.reconcile_with_persistence(desired, false)
    }

    fn reconcile_with_persistence(
        &self,
        desired: &ProxyDesired,
        commit_features: bool,
    ) -> Result<ProxyReconcileResult, PlatformError> {
        let lease = self.platform.acquire_lifecycle_lock()?;
        let result = self.reconcile_locked_with_persistence(desired, commit_features);
        let release = self.platform.release_lifecycle_lock(&lease);
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(result), Ok(())) => Ok(result),
        }
    }

    pub(super) fn reconcile_locked(
        &self,
        desired: &ProxyDesired,
    ) -> Result<ProxyReconcileResult, PlatformError> {
        self.reconcile_locked_with_persistence(desired, true)
    }

    fn reconcile_locked_with_persistence(
        &self,
        desired: &ProxyDesired,
        commit_features: bool,
    ) -> Result<ProxyReconcileResult, PlatformError> {
        let network = self.probe.observe_network()?;
        let observed = self.probe.observe_proxy()?;
        let previous = match &observed.persisted_features {
            Probe::Known(features) if features.supported() => *features,
            Probe::Known(_) => {
                return Err(PlatformError::ProbeFailed(
                    "persisted proxy feature version is unsupported".to_owned(),
                ))
            }
            Probe::Unknown(ref reason) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "persisted proxy mode is unknown: {reason}"
                )))
            }
        };
        let token = self.clock.ownership_token("hyz-mihomo")?;
        let mut actions = proxy_plan(desired, &observed, &network, &token)?;
        if !commit_features && matches!(actions.last(), Some(ProxyAction::CommitFeatures { .. })) {
            actions.pop();
        }
        let mut applied = Vec::new();
        for action in &actions {
            if let ProxyAction::InstallInterceptionEntry { token } = action {
                let preinterception = match self.probe.observe_proxy() {
                    Ok(observed) => observed,
                    Err(error) => {
                        self.rollback(&applied, previous);
                        return Err(error);
                    }
                };
                if !preinterception.ready_for_interception(token, &desired.direct_macs) {
                    self.rollback(&applied, previous);
                    return Err(PlatformError::UnsafeToCutOver(
                        "Mihomo TUN ownership changed before interception commit".to_owned(),
                    ));
                }
            }
            if let ProxyAction::CommitFeatures { features } = action {
                let mut precommit = match self.probe.observe_proxy() {
                    Ok(observed) => observed,
                    Err(error) => {
                        self.rollback(&applied, previous);
                        return Err(error);
                    }
                };
                precommit.persisted_features = Probe::Known(*features);
                if !precommit.ready_for(desired) {
                    self.rollback(&applied, previous);
                    return Err(PlatformError::UnsafeToCutOver(
                        "proxy data plane was not ready at persistent-mode commit point".to_owned(),
                    ));
                }
            }
            if let Err(error) = self.platform.apply_proxy(action) {
                self.rollback(&applied, previous);
                return Err(error);
            }
            applied.push(action.clone());
        }
        let observed = match self.probe.observe_proxy() {
            Ok(observed) => observed,
            Err(error) => {
                self.rollback(&applied, previous);
                return Err(error);
            }
        };
        let ready = if commit_features {
            observed.ready_for(desired)
        } else {
            observed.persisted_features == Probe::Known(previous)
                && observed.runtime_ready_for_without_ordinary_nat(desired)
        };
        if !ready {
            self.rollback(&applied, previous);
            return Err(PlatformError::UnsafeToCutOver(
                "proxy actions completed but strict readiness probe failed".to_owned(),
            ));
        }
        Ok(ProxyReconcileResult {
            observed,
            actions_applied: actions.len(),
        })
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
        ProxyAction::CreateTunChains { token, .. } => Some(ProxyAction::RemoveTunChains {
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

    #[test]
    fn proxy_action_ownership_tokens_are_preserved_during_rollback() {
        assert_eq!(
            rollback_compensation(&ProxyAction::InstallPolicyRule),
            Some(ProxyAction::RemovePolicyRule)
        );
        assert_eq!(
            rollback_compensation(&ProxyAction::CreateTunChains {
                token: "tun".to_owned(),
                direct_macs: Default::default(),
                active: crate::domain::network::ActiveUplinkObserved::new(
                    crate::domain::network::UplinkId::Wifi,
                    "192.0.2.1".parse().unwrap(),
                ),
            }),
            Some(ProxyAction::RemoveTunChains {
                token: "tun".to_owned(),
            })
        );
    }
}
