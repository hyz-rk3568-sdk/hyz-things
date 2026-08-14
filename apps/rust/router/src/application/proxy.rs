use crate::{
    application::{
        ports::{
            ClockPort, PlatformError, RouterPlatformPort, SystemProbePort, TailscalePlatformPort,
            TailscaleProbePort,
        },
        reconcile::proxy_plan,
    },
    domain::{
        network::{OwnedResource, Probe},
        proxy::{ProxyAction, ProxyDesired, ProxyFeaturesV1, ProxyObserved},
        tailscale::{
            TailscaleAction, TailscaleDesired, TailscaleEnvironment, TailscaleMode,
            TailscaleProcessState,
        },
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
        let restore_mode = applied
            .iter()
            .any(|action| matches!(action, ProxyAction::CommitFeatures { .. }));
        for action in applied.iter().rev() {
            if let Some(compensation) = rollback_compensation(action) {
                let _ = self.platform.apply_proxy(&compensation);
            }
        }
        if restore_mode {
            let _ = self
                .platform
                .apply_proxy(&ProxyAction::RestorePersistedFeatures { features: previous });
        }
    }

    pub fn reconcile(&self, desired: &ProxyDesired) -> Result<ProxyReconcileResult, PlatformError> {
        let lease = self.platform.acquire_lifecycle_lock()?;
        let result = self.reconcile_locked(desired);
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
        let actions = proxy_plan(desired, &observed, &network, &token)?;
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
        if !observed.ready_for(desired) {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyFeatureReconcileResult {
    pub observed: ProxyObserved,
    pub proxy_actions_applied: usize,
    pub tailscale_actions_applied: usize,
}

pub struct ProxyFeatureCoordinator<'a> {
    platform: &'a dyn RouterPlatformPort,
    probe: &'a dyn SystemProbePort,
    tailscale: &'a dyn TailscalePlatformPort,
    tailscale_probe: &'a dyn TailscaleProbePort,
    clock: &'a dyn ClockPort,
}

impl<'a> ProxyFeatureCoordinator<'a> {
    pub fn new(
        platform: &'a dyn RouterPlatformPort,
        probe: &'a dyn SystemProbePort,
        tailscale: &'a dyn TailscalePlatformPort,
        tailscale_probe: &'a dyn TailscaleProbePort,
        clock: &'a dyn ClockPort,
    ) -> Self {
        Self {
            platform,
            probe,
            tailscale,
            tailscale_probe,
            clock,
        }
    }

    pub fn reconcile(
        &self,
        desired: &ProxyDesired,
    ) -> Result<ProxyFeatureReconcileResult, PlatformError> {
        self.reconcile_with_persistence(desired, true)
    }

    pub fn reconcile_runtime_preserving_features(
        &self,
        desired: &ProxyDesired,
    ) -> Result<ProxyFeatureReconcileResult, PlatformError> {
        self.reconcile_with_persistence(desired, false)
    }

    fn reconcile_with_persistence(
        &self,
        desired: &ProxyDesired,
        commit_features: bool,
    ) -> Result<ProxyFeatureReconcileResult, PlatformError> {
        let lease = self.platform.acquire_lifecycle_lock()?;
        let result = self.reconcile_locked(desired, commit_features);
        let release = self.platform.release_lifecycle_lock(&lease);
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(result), Ok(())) => Ok(result),
        }
    }

    fn reconcile_locked(
        &self,
        desired: &ProxyDesired,
        commit_features: bool,
    ) -> Result<ProxyFeatureReconcileResult, PlatformError> {
        let initial_proxy = self.probe.observe_proxy()?;
        let previous = match initial_proxy.persisted_features {
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
        let target_environment = if desired.tailscale_explicit_proxy_enabled {
            TailscaleEnvironment::MihomoExplicit
        } else {
            TailscaleEnvironment::Direct
        };
        let initial_tailscale = self.tailscale_probe.observe_tailscale()?;
        let current_environment = match initial_tailscale.environment {
            Probe::Known(environment) => environment,
            Probe::Unknown(reason) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "tailscaled environment is unknown: {reason}"
                )))
            }
        };
        let network = self.probe.observe_network()?;
        let observed = self.probe.observe_proxy()?;
        let token = self.clock.ownership_token("hyz-mihomo")?;
        let mut actions = proxy_plan(desired, &observed, &network, &token)?;
        let commit = actions
            .pop()
            .filter(|action| matches!(action, ProxyAction::CommitFeatures { .. }));
        let core_restart = actions
            .iter()
            .any(|action| matches!(action, ProxyAction::StopCore));
        let move_tailscale_direct = current_environment == TailscaleEnvironment::MihomoExplicit
            && (target_environment == TailscaleEnvironment::Direct || core_restart);

        let mut tailscale_actions_applied = 0;
        if move_tailscale_direct {
            match self.restart_tailscale(&initial_tailscale, TailscaleEnvironment::Direct) {
                Ok(applied) => tailscale_actions_applied += applied,
                Err(error) => {
                    let _ = self.restore_tailscale_environment(current_environment);
                    return Err(error);
                }
            }
        }

        let mut proxy_actions_applied = 0;
        for action in &actions {
            if let ProxyAction::InstallInterceptionEntry { token } = action {
                let preinterception = match self.probe.observe_proxy() {
                    Ok(observed) => observed,
                    Err(error) => {
                        let _ = self.restore_proxy(previous, &desired.direct_macs);
                        if current_environment != target_environment || move_tailscale_direct {
                            let _ = self.restore_tailscale_environment(current_environment);
                        }
                        return Err(error);
                    }
                };
                if !preinterception.ready_for_interception(token, &desired.direct_macs) {
                    let _ = self.restore_proxy(previous, &desired.direct_macs);
                    if current_environment != target_environment || move_tailscale_direct {
                        let _ = self.restore_tailscale_environment(current_environment);
                    }
                    return Err(PlatformError::UnsafeToCutOver(
                        "Mihomo TUN ownership changed before interception commit".to_owned(),
                    ));
                }
            }
            if let Err(error) = self.platform.apply_proxy(action) {
                let _ = self.restore_proxy(previous, &desired.direct_macs);
                if current_environment != target_environment || move_tailscale_direct {
                    let _ = self.restore_tailscale_environment(current_environment);
                }
                return Err(error);
            }
            proxy_actions_applied += 1;
        }

        if target_environment == TailscaleEnvironment::MihomoExplicit
            && (current_environment == TailscaleEnvironment::Direct || move_tailscale_direct)
        {
            let observed = self.probe.observe_proxy()?;
            if !observed.core_ready() {
                let _ = self.restore_proxy(previous, &desired.direct_macs);
                let _ = self.restore_tailscale_environment(current_environment);
                return Err(PlatformError::UnsafeToCutOver(
                    "Mihomo core and fixed mixed port are not ready for tailscaled".to_owned(),
                ));
            }
            let tailscale_observed = self.tailscale_probe.observe_tailscale()?;
            tailscale_actions_applied += self
                .restart_tailscale(&tailscale_observed, TailscaleEnvironment::MihomoExplicit)
                .inspect_err(|_| {
                    let _ = self.restore_proxy(previous, &desired.direct_macs);
                    let _ = self.restore_tailscale_environment(current_environment);
                })?;
        }

        let final_tailscale = match self.tailscale_probe.observe_tailscale() {
            Ok(observed) => observed,
            Err(error) => {
                let _ = self.restore_tailscale_environment(current_environment);
                let _ = self.restore_proxy(previous, &desired.direct_macs);
                return Err(error);
            }
        };
        if final_tailscale.environment != Probe::Known(target_environment) {
            let _ = self.restore_tailscale_environment(current_environment);
            let _ = self.restore_proxy(previous, &desired.direct_macs);
            return Err(PlatformError::UnsafeToCutOver(
                "tailscaled exact environment did not reach the requested state".to_owned(),
            ));
        }
        if desired.tailscale_explicit_proxy_enabled {
            let mode = match persisted_tailscale_mode(&final_tailscale) {
                Ok(mode) => mode,
                Err(error) => {
                    let _ = self.restore_tailscale_environment(current_environment);
                    let _ = self.restore_proxy(previous, &desired.direct_macs);
                    return Err(error);
                }
            };
            let ordinary_router_ready = match self.probe.observe_network() {
                Ok(network) => {
                    network.ready_for(&crate::domain::network::NetworkDesired::forwarding())
                }
                Err(error) => {
                    let _ = self.restore_tailscale_environment(current_environment);
                    let _ = self.restore_proxy(previous, &desired.direct_macs);
                    return Err(error);
                }
            };
            if mode == TailscaleMode::Disabled
                || !final_tailscale.ready_for(&TailscaleDesired { mode }, ordinary_router_ready)
            {
                let _ = self.restore_tailscale_environment(current_environment);
                let _ = self.restore_proxy(previous, &desired.direct_macs);
                return Err(PlatformError::UnsafeToCutOver(
                    "Tailscale access mode is not ready for explicit proxy cutover".to_owned(),
                ));
            }
            match self.tailscale_probe.probe_explicit_proxy_path() {
                Ok(true) => {}
                Ok(false) => {
                    let _ = self.restore_tailscale_environment(current_environment);
                    let _ = self.restore_proxy(previous, &desired.direct_macs);
                    return Err(PlatformError::UnsafeToCutOver(
                        "fixed Tailscale explicit proxy path is unavailable".to_owned(),
                    ));
                }
                Err(error) => {
                    let _ = self.restore_tailscale_environment(current_environment);
                    let _ = self.restore_proxy(previous, &desired.direct_macs);
                    return Err(error);
                }
            }
        }
        if commit_features {
            if let Some(commit) = commit {
                if let Err(error) = self.platform.apply_proxy(&commit) {
                    let _ = self.restore_tailscale_environment(current_environment);
                    let _ = self.restore_proxy(previous, &desired.direct_macs);
                    return Err(error);
                }
                proxy_actions_applied += 1;
            }
        }
        let observed = match self.probe.observe_proxy() {
            Ok(observed) => observed,
            Err(error) => {
                let _ = self.restore_tailscale_environment(current_environment);
                let _ = self.restore_proxy(previous, &desired.direct_macs);
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
            let _ = self.restore_tailscale_environment(current_environment);
            let _ = self.restore_proxy(previous, &desired.direct_macs);
            return Err(PlatformError::UnsafeToCutOver(
                "proxy feature cutover failed strict final readiness".to_owned(),
            ));
        }
        Ok(ProxyFeatureReconcileResult {
            observed,
            proxy_actions_applied,
            tailscale_actions_applied,
        })
    }

    fn restart_tailscale(
        &self,
        observed: &crate::domain::tailscale::TailscaleObserved,
        environment: TailscaleEnvironment,
    ) -> Result<usize, PlatformError> {
        let new_token = self.clock.ownership_token("hyz-tailscale")?;
        let listener_token = match &observed.management_listener {
            Probe::Known(OwnedResource::Owned { token }) => Some(token.clone()),
            Probe::Known(OwnedResource::Absent) => None,
            Probe::Known(OwnedResource::Foreign) => {
                return Err(PlatformError::Conflict(
                    "refusing to restart tailscaled while its management listener is foreign"
                        .to_owned(),
                ))
            }
            Probe::Unknown(reason) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "Tailscale management listener is unknown: {reason}"
                )))
            }
        };
        let mut applied = 0;
        if let Some(token) = listener_token {
            self.tailscale
                .apply_tailscale(&TailscaleAction::StopManagementListener { token })?;
            applied += 1;
        }
        let actions = tailscale_environment_plan(observed, environment, &new_token)?;
        for action in &actions {
            self.tailscale.apply_tailscale(action)?;
            applied += 1;
        }
        let restarted = self.tailscale_probe.observe_tailscale()?;
        if persisted_tailscale_mode(&restarted)? != TailscaleMode::Disabled {
            let ipv4 = match restarted.ipv4 {
                Probe::Known(Some(ipv4)) => ipv4,
                Probe::Known(None) => {
                    return Err(PlatformError::UnsafeToCutOver(
                        "tailscaled restarted without a management IPv4".to_owned(),
                    ))
                }
                Probe::Unknown(reason) => {
                    return Err(PlatformError::ProbeFailed(format!(
                        "Tailscale IPv4 is unknown after backend restart: {reason}"
                    )))
                }
            };
            self.tailscale
                .apply_tailscale(&TailscaleAction::StartManagementListener {
                    token: new_token,
                    ipv4,
                })?;
            applied += 1;
        }
        Ok(applied)
    }

    fn restore_tailscale_environment(
        &self,
        environment: TailscaleEnvironment,
    ) -> Result<(), PlatformError> {
        let observed = self.tailscale_probe.observe_tailscale()?;
        self.restart_tailscale(&observed, environment).map(|_| ())
    }

    fn restore_proxy(
        &self,
        previous: ProxyFeaturesV1,
        direct_macs: &std::collections::BTreeSet<crate::domain::device_policy::LanDeviceMac>,
    ) -> Result<(), PlatformError> {
        let desired = ProxyDesired {
            lan_tun_enabled: previous.lan_tun_enabled,
            tailscale_explicit_proxy_enabled: previous.tailscale_explicit_proxy_enabled,
            direct_macs: direct_macs.clone(),
        };
        ProxyApplication::new(self.platform, self.probe, self.clock)
            .reconcile_locked(&desired)
            .map(|_| ())
    }
}

pub fn tailscale_environment_plan(
    observed: &crate::domain::tailscale::TailscaleObserved,
    environment: TailscaleEnvironment,
    new_token: &str,
) -> Result<Vec<TailscaleAction>, PlatformError> {
    let token = match &observed.process {
        Probe::Known(TailscaleProcessState::OwnedLive { token }) => token.clone(),
        Probe::Known(TailscaleProcessState::Absent)
            if environment == TailscaleEnvironment::Direct =>
        {
            return Ok(Vec::new())
        }
        Probe::Known(_) => {
            return Err(PlatformError::Conflict(
                "tailscaled is not an exact owned live process".to_owned(),
            ))
        }
        Probe::Unknown(reason) => {
            return Err(PlatformError::ProbeFailed(format!(
                "tailscaled process identity is unknown: {reason}"
            )))
        }
    };
    let mut actions = vec![
        TailscaleAction::StopBackend { token },
        TailscaleAction::StartBackend {
            token: new_token.to_owned(),
            environment,
        },
        TailscaleAction::WaitForBackend,
        TailscaleAction::SetFixedPreferences,
    ];
    match persisted_tailscale_mode(observed)? {
        TailscaleMode::LanSubnetAccess => actions.push(TailscaleAction::AdvertiseLanRoute),
        TailscaleMode::RouterOnly | TailscaleMode::Disabled => {
            actions.push(TailscaleAction::ClearAdvertisedRoute)
        }
    }
    Ok(actions)
}

fn persisted_tailscale_mode(
    observed: &crate::domain::tailscale::TailscaleObserved,
) -> Result<TailscaleMode, PlatformError> {
    match observed.persisted_mode {
        Probe::Known(Some(mode)) => Ok(mode),
        Probe::Known(None) => Ok(TailscaleMode::Disabled),
        Probe::Unknown(ref reason) => Err(PlatformError::ProbeFailed(format!(
            "persisted Tailscale mode is unknown: {reason}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::ports::LifecycleLease,
        domain::{
            network::{NetworkObserved, OwnedResource},
            tailscale::{TailscaleBackendState, TailscalePreferences},
        },
    };
    use std::sync::Mutex;

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

    fn tailscale_observed(mode: TailscaleMode) -> crate::domain::tailscale::TailscaleObserved {
        use crate::domain::{
            network::OwnedResource,
            tailscale::{
                TailscaleBackendState, TailscaleConnectionKind, TailscaleObserved,
                TailscalePreferences,
            },
        };
        TailscaleObserved {
            persisted_mode: Probe::Known(Some(mode)),
            backend_state: Probe::Known(TailscaleBackendState::Running),
            process: Probe::Known(TailscaleProcessState::OwnedLive {
                token: "old".to_owned(),
            }),
            environment: Probe::Known(TailscaleEnvironment::Direct),
            socket: Probe::Known(OwnedResource::Owned {
                token: "old".to_owned(),
            }),
            interface: Probe::Known(OwnedResource::Owned {
                token: "old".to_owned(),
            }),
            authenticated: Probe::Known(true),
            ipv4: Probe::Known(Some("100.64.0.1".parse().unwrap())),
            preferences: Probe::Known(TailscalePreferences::FIXED),
            route_advertised: Probe::Known(mode == TailscaleMode::LanSubnetAccess),
            router_firewall: Probe::Known(OwnedResource::Owned {
                token: "firewall".to_owned(),
            }),
            subnet_firewall: Probe::Known(if mode == TailscaleMode::LanSubnetAccess {
                OwnedResource::Owned {
                    token: "subnet".to_owned(),
                }
            } else {
                OwnedResource::Absent
            }),
            management_listener: Probe::Known(OwnedResource::Owned {
                token: "listener".to_owned(),
            }),
            management_listener_ipv4: Probe::Known(Some("100.64.0.1".parse().unwrap())),
            connection: Probe::Known(TailscaleConnectionKind::Direct),
        }
    }

    #[test]
    fn environment_plan_restarts_exact_backend_and_restores_route_preferences() {
        let actions = tailscale_environment_plan(
            &tailscale_observed(TailscaleMode::LanSubnetAccess),
            TailscaleEnvironment::MihomoExplicit,
            "new",
        )
        .unwrap();
        assert_eq!(
            actions,
            vec![
                TailscaleAction::StopBackend {
                    token: "old".to_owned(),
                },
                TailscaleAction::StartBackend {
                    token: "new".to_owned(),
                    environment: TailscaleEnvironment::MihomoExplicit,
                },
                TailscaleAction::WaitForBackend,
                TailscaleAction::SetFixedPreferences,
                TailscaleAction::AdvertiseLanRoute,
            ]
        );
    }

    #[test]
    fn environment_plan_rejects_unknown_or_foreign_process_identity() {
        let mut observed = tailscale_observed(TailscaleMode::RouterOnly);
        observed.process = Probe::Known(TailscaleProcessState::Foreign);
        assert!(matches!(
            tailscale_environment_plan(&observed, TailscaleEnvironment::MihomoExplicit, "new"),
            Err(PlatformError::Conflict(_))
        ));
        observed.process = Probe::Unknown("proc unreadable".to_owned());
        assert!(matches!(
            tailscale_environment_plan(&observed, TailscaleEnvironment::Direct, "new"),
            Err(PlatformError::ProbeFailed(_))
        ));
    }

    fn coordinator_proxy(features: ProxyFeaturesV1) -> ProxyObserved {
        let core = features.mihomo_required();
        let tun = features.lan_tun_enabled;
        ProxyObserved {
            persisted_features: Probe::Known(features),
            process_identity_valid: Probe::Known(core),
            watcher_identity_valid: Probe::Known(tun),
            runtime_config_valid: Probe::Known(core),
            mixed_port_ready: Probe::Known(core),
            tun_interface: Probe::Known(if tun {
                OwnedResource::Owned {
                    token: "proxy-old".to_owned(),
                }
            } else {
                OwnedResource::Absent
            }),
            tun_firewall: Probe::Known(if tun {
                OwnedResource::Owned {
                    token: "proxy-old".to_owned(),
                }
            } else {
                OwnedResource::Absent
            }),
            policy_rule_present: Probe::Known(tun),
            policy_route_present: Probe::Known(tun),
            interception_entry_present: Probe::Known(tun),
            ordinary_nat_confirmed: Probe::Known(!tun),
            active_direct_macs: Probe::Known(Default::default()),
        }
    }

    struct CoordinatorFake {
        proxy: Mutex<ProxyObserved>,
        tailscale: Mutex<crate::domain::tailscale::TailscaleObserved>,
        proxy_path_ready: Mutex<bool>,
        events: Mutex<Vec<String>>,
        replace_tun_before_interception: bool,
    }

    impl CoordinatorFake {
        fn new(features: ProxyFeaturesV1, environment: TailscaleEnvironment) -> Self {
            let mut tailscale = tailscale_observed(TailscaleMode::RouterOnly);
            tailscale.environment = Probe::Known(environment);
            Self {
                proxy: Mutex::new(coordinator_proxy(features)),
                tailscale: Mutex::new(tailscale),
                proxy_path_ready: Mutex::new(true),
                events: Mutex::new(Vec::new()),
                replace_tun_before_interception: false,
            }
        }

        fn with_tun_replacement(mut self) -> Self {
            self.replace_tun_before_interception = true;
            self
        }

        fn lease(path: &'static str) -> LifecycleLease {
            LifecycleLease {
                path,
                identity: "fake".to_owned(),
                directory_device: 1,
                directory_inode: 1,
            }
        }

        fn events(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }
    }

    impl RouterPlatformPort for CoordinatorFake {
        fn acquire_lifecycle_lock(&self) -> Result<LifecycleLease, PlatformError> {
            self.events.lock().unwrap().push("router:lock".to_owned());
            Ok(Self::lease("/run/fake-router.lock"))
        }

        fn release_lifecycle_lock(&self, _: &LifecycleLease) -> Result<(), PlatformError> {
            self.events
                .lock()
                .unwrap()
                .push("router:release".to_owned());
            Ok(())
        }

        fn apply_network(
            &self,
            _: &crate::domain::network::NetworkAction,
        ) -> Result<(), PlatformError> {
            unreachable!("coordinator does not apply network actions")
        }

        fn apply_proxy(&self, action: &ProxyAction) -> Result<(), PlatformError> {
            self.events
                .lock()
                .unwrap()
                .push(format!("proxy:{action:?}"));
            let mut observed = self.proxy.lock().unwrap();
            match action {
                ProxyAction::WriteRuntimeConfig { .. } => {
                    observed.runtime_config_valid = Probe::Known(true)
                }
                ProxyAction::StartCore => observed.process_identity_valid = Probe::Known(true),
                ProxyAction::WaitForMixedPort => observed.mixed_port_ready = Probe::Known(true),
                ProxyAction::WaitForTunInterface { token } => {
                    observed.tun_interface = Probe::Known(OwnedResource::Owned {
                        token: token.clone(),
                    })
                }
                ProxyAction::CreateTunChains { token, direct_macs } => {
                    observed.tun_firewall = Probe::Known(OwnedResource::Owned {
                        token: token.clone(),
                    });
                    observed.active_direct_macs = Probe::Known(direct_macs.clone());
                }
                ProxyAction::InstallPolicyRoute => {
                    observed.policy_route_present = Probe::Known(true)
                }
                ProxyAction::InstallPolicyRule => observed.policy_rule_present = Probe::Known(true),
                ProxyAction::InstallInterceptionEntry { .. } => {
                    observed.interception_entry_present = Probe::Known(true)
                }
                ProxyAction::StartWatcher => observed.watcher_identity_valid = Probe::Known(true),
                ProxyAction::StopCore => {
                    observed.process_identity_valid = Probe::Known(false);
                    observed.mixed_port_ready = Probe::Known(false);
                }
                ProxyAction::RemoveRuntimeState => {
                    observed.runtime_config_valid = Probe::Known(false)
                }
                ProxyAction::CommitFeatures { features }
                | ProxyAction::RestorePersistedFeatures { features } => {
                    observed.persisted_features = Probe::Known(*features)
                }
                _ => {}
            }
            Ok(())
        }
    }

    impl SystemProbePort for CoordinatorFake {
        fn observe_network(&self) -> Result<NetworkObserved, PlatformError> {
            Ok(NetworkObserved {
                bridge: Probe::Known(OwnedResource::Owned {
                    token: "bridge".to_owned(),
                }),
                bridge_up: Probe::Known(true),
                lan_address_present: Probe::Known(true),
                ap_attached: Probe::Known(true),
                management_services_healthy: Probe::Known(true),
                wan_default_route_present: Probe::Known(true),
                ipv4_forwarding: Probe::Known(true),
                previous_ipv4_forwarding: Probe::Known(Some(false)),
                router_firewall: Probe::Known(OwnedResource::Owned {
                    token: "router".to_owned(),
                }),
            })
        }

        fn observe_proxy(&self) -> Result<ProxyObserved, PlatformError> {
            let mut observed = self.proxy.lock().unwrap();
            if self.replace_tun_before_interception
                && observed.policy_rule_present == Probe::Known(true)
                && observed.interception_entry_present == Probe::Known(false)
            {
                observed.tun_interface = Probe::Known(OwnedResource::Foreign);
            }
            Ok(observed.clone())
        }
    }

    impl TailscalePlatformPort for CoordinatorFake {
        fn acquire_tailscale_lock(&self) -> Result<LifecycleLease, PlatformError> {
            self.events
                .lock()
                .unwrap()
                .push("tailscale:lock".to_owned());
            Ok(Self::lease("/run/fake-tailscale.lock"))
        }

        fn release_tailscale_lock(&self, _: &LifecycleLease) -> Result<(), PlatformError> {
            self.events
                .lock()
                .unwrap()
                .push("tailscale:release".to_owned());
            Ok(())
        }

        fn apply_tailscale(&self, action: &TailscaleAction) -> Result<(), PlatformError> {
            self.events
                .lock()
                .unwrap()
                .push(format!("tailscale:{action:?}"));
            let mut observed = self.tailscale.lock().unwrap();
            match action {
                TailscaleAction::StopBackend { .. } => {
                    observed.process = Probe::Known(TailscaleProcessState::Absent);
                    observed.backend_state = Probe::Known(TailscaleBackendState::Stopped);
                }
                TailscaleAction::StartBackend { token, environment } => {
                    observed.process = Probe::Known(TailscaleProcessState::OwnedLive {
                        token: token.clone(),
                    });
                    observed.socket = Probe::Known(OwnedResource::Owned {
                        token: token.clone(),
                    });
                    observed.interface = Probe::Known(OwnedResource::Owned {
                        token: token.clone(),
                    });
                    observed.environment = Probe::Known(*environment);
                    observed.backend_state = Probe::Known(TailscaleBackendState::Running);
                }
                TailscaleAction::SetFixedPreferences => {
                    observed.preferences = Probe::Known(TailscalePreferences::FIXED)
                }
                TailscaleAction::ClearAdvertisedRoute => {
                    observed.route_advertised = Probe::Known(false)
                }
                TailscaleAction::StopManagementListener { .. } => {
                    observed.management_listener = Probe::Known(OwnedResource::Absent);
                    observed.management_listener_ipv4 = Probe::Known(None);
                }
                TailscaleAction::StartManagementListener { token, ipv4 } => {
                    observed.management_listener = Probe::Known(OwnedResource::Owned {
                        token: token.clone(),
                    });
                    observed.management_listener_ipv4 = Probe::Known(Some(*ipv4));
                }
                _ => {}
            }
            Ok(())
        }

        fn request_login(
            &self,
        ) -> Result<crate::domain::tailscale::TailscaleLoginUrl, PlatformError> {
            Err(PlatformError::InvalidState(
                "coordinator test does not request login".to_owned(),
            ))
        }

        fn logout(&self) -> Result<(), PlatformError> {
            unreachable!("coordinator test does not log out")
        }
    }

    impl TailscaleProbePort for CoordinatorFake {
        fn observe_tailscale(
            &self,
        ) -> Result<crate::domain::tailscale::TailscaleObserved, PlatformError> {
            Ok(self.tailscale.lock().unwrap().clone())
        }

        fn probe_explicit_proxy_path(&self) -> Result<bool, PlatformError> {
            self.events
                .lock()
                .unwrap()
                .push("tailscale:path-probe".to_owned());
            Ok(*self.proxy_path_ready.lock().unwrap())
        }
    }

    impl ClockPort for CoordinatorFake {
        fn unix_time_millis(&self) -> u64 {
            42
        }
    }

    #[test]
    fn coordinator_reprobes_exact_tun_ownership_before_interception_commit() {
        let fake = CoordinatorFake::new(ProxyFeaturesV1::disabled(), TailscaleEnvironment::Direct)
            .with_tun_replacement();
        let error = ProxyFeatureCoordinator::new(&fake, &fake, &fake, &fake, &fake)
            .reconcile(&ProxyDesired {
                lan_tun_enabled: true,
                tailscale_explicit_proxy_enabled: false,
                direct_macs: Default::default(),
            })
            .expect_err("foreign replacement must block interception");

        assert!(matches!(error, PlatformError::UnsafeToCutOver(_)));
        assert!(!fake
            .events()
            .iter()
            .any(|event| event.contains("InstallInterceptionEntry")));
    }

    #[test]
    fn coordinator_enables_core_before_proxy_environment_and_commits_last() {
        let fake = CoordinatorFake::new(ProxyFeaturesV1::disabled(), TailscaleEnvironment::Direct);
        ProxyFeatureCoordinator::new(&fake, &fake, &fake, &fake, &fake)
            .reconcile(&ProxyDesired {
                lan_tun_enabled: false,
                tailscale_explicit_proxy_enabled: true,
                direct_macs: Default::default(),
            })
            .unwrap();
        let events = fake.events();
        let mixed = events
            .iter()
            .position(|event| event == "proxy:WaitForMixedPort")
            .unwrap();
        let stop_listener = events
            .iter()
            .position(|event| event.contains("tailscale:StopManagementListener"))
            .unwrap();
        let stop_backend = events
            .iter()
            .position(|event| event.contains("tailscale:StopBackend"))
            .unwrap();
        let proxied = events
            .iter()
            .position(|event| event.contains("environment: MihomoExplicit"))
            .unwrap();
        let start_listener = events
            .iter()
            .position(|event| event.contains("tailscale:StartManagementListener"))
            .unwrap();
        let path_probe = events
            .iter()
            .position(|event| event == "tailscale:path-probe")
            .unwrap();
        let commit = events
            .iter()
            .position(|event| event.contains("proxy:CommitFeatures"))
            .unwrap();
        assert!(mixed < stop_listener && stop_listener < stop_backend);
        assert!(stop_backend < proxied && proxied < start_listener);
        assert!(start_listener < path_probe && path_probe < commit);
        assert!(!events
            .iter()
            .any(|event| event.starts_with("tailscale:lock")));
        assert!(!events
            .iter()
            .any(|event| event.starts_with("tailscale:release")));
    }

    #[test]
    fn unavailable_explicit_proxy_path_blocks_commit_and_preserves_ready_lan_tun() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(true, false),
            TailscaleEnvironment::Direct,
        );
        *fake.proxy_path_ready.lock().unwrap() = false;

        assert!(matches!(
            ProxyFeatureCoordinator::new(&fake, &fake, &fake, &fake, &fake).reconcile(
                &ProxyDesired {
                    lan_tun_enabled: true,
                    tailscale_explicit_proxy_enabled: true,
                    direct_macs: Default::default(),
                }
            ),
            Err(PlatformError::UnsafeToCutOver(_))
        ));

        let proxy = fake.proxy.lock().unwrap().clone();
        assert_eq!(
            proxy.persisted_features,
            Probe::Known(ProxyFeaturesV1::new(true, false))
        );
        assert!(proxy.lan_tun_ready(&Default::default()));
        assert_eq!(
            fake.tailscale.lock().unwrap().environment,
            Probe::Known(TailscaleEnvironment::Direct)
        );
        let events = fake.events();
        assert!(events.iter().any(|event| event == "tailscale:path-probe"));
        assert!(!events.iter().any(|event| {
            event.contains("CommitFeatures")
                && event.contains("tailscale_explicit_proxy_enabled: true")
        }));
    }

    #[test]
    fn coordinator_moves_tailscale_direct_around_shared_core_restart() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(false, true),
            TailscaleEnvironment::MihomoExplicit,
        );
        ProxyFeatureCoordinator::new(&fake, &fake, &fake, &fake, &fake)
            .reconcile(&ProxyDesired {
                lan_tun_enabled: true,
                tailscale_explicit_proxy_enabled: true,
                direct_macs: Default::default(),
            })
            .unwrap();
        let events = fake.events();
        let direct = events
            .iter()
            .position(|event| event.contains("environment: Direct"))
            .unwrap();
        let stop_core = events
            .iter()
            .position(|event| event == "proxy:StopCore")
            .unwrap();
        let start_core = events
            .iter()
            .position(|event| event == "proxy:StartCore")
            .unwrap();
        let proxied = events
            .iter()
            .rposition(|event| event.contains("environment: MihomoExplicit"))
            .unwrap();
        let commit = events
            .iter()
            .position(|event| event.contains("proxy:CommitFeatures"))
            .unwrap();
        assert!(direct < stop_core && stop_core < start_core);
        assert!(start_core < proxied && proxied < commit);
    }

    #[test]
    fn coordinator_restores_direct_before_stopping_last_core_dependency() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(false, true),
            TailscaleEnvironment::MihomoExplicit,
        );
        ProxyFeatureCoordinator::new(&fake, &fake, &fake, &fake, &fake)
            .reconcile(&ProxyDesired {
                lan_tun_enabled: false,
                tailscale_explicit_proxy_enabled: false,
                direct_macs: Default::default(),
            })
            .unwrap();
        let events = fake.events();
        let direct = events
            .iter()
            .position(|event| event.contains("environment: Direct"))
            .unwrap();
        let stop_core = events
            .iter()
            .position(|event| event == "proxy:StopCore")
            .unwrap();
        let commit = events
            .iter()
            .position(|event| event.contains("proxy:CommitFeatures"))
            .unwrap();
        assert!(direct < stop_core && stop_core < commit);
    }
}
