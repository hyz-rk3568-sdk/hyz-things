use crate::{
    application::{
        ports::{
            ClockPort, PlatformError, RouterPlatformPort, SystemProbePort, TailscalePlatformPort,
            TailscaleProbePort,
        },
        reconcile::{proxy_plan, tailscale_plan, tailscale_shutdown_plan},
    },
    domain::{
        network::{OwnedResource, Probe},
        proxy::{ProxyAction, ProxyDesired, ProxyFeaturesV1, ProxyObserved},
        tailscale::{
            TailscaleAction, TailscaleDesired, TailscaleEnvironment, TailscaleMode,
            TailscalePreferences, TailscaleProcessState, TailscaleReadiness,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplicitProxyPathObservation {
    Ready,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplicitProxyFallbackDecision {
    Hold,
    FallbackToDirect,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExplicitProxyFallbackTracker {
    consecutive_failures: u8,
}

impl ExplicitProxyFallbackTracker {
    pub const FAILURE_THRESHOLD: u8 = 3;

    pub const fn consecutive_failures(self) -> u8 {
        self.consecutive_failures
    }

    pub fn observe(
        &mut self,
        observation: ExplicitProxyPathObservation,
    ) -> ExplicitProxyFallbackDecision {
        match observation {
            ExplicitProxyPathObservation::Ready | ExplicitProxyPathObservation::Unknown => {
                self.consecutive_failures = 0;
                ExplicitProxyFallbackDecision::Hold
            }
            ExplicitProxyPathObservation::Unavailable => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                if self.consecutive_failures >= Self::FAILURE_THRESHOLD {
                    self.consecutive_failures = 0;
                    ExplicitProxyFallbackDecision::FallbackToDirect
                } else {
                    ExplicitProxyFallbackDecision::Hold
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailscaleProxyFallbackResult {
    NotNeeded,
    DirectRestored { tailscale_actions_applied: usize },
    ExplicitRestored { tailscale_actions_applied: usize },
}

pub struct TailscaleProxyFallbackApplication<'a> {
    platform: &'a dyn RouterPlatformPort,
    probe: &'a dyn SystemProbePort,
    tailscale: &'a dyn TailscalePlatformPort,
    tailscale_probe: &'a dyn TailscaleProbePort,
    clock: &'a dyn ClockPort,
}

impl<'a> TailscaleProxyFallbackApplication<'a> {
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

    pub fn fallback_to_direct_if_unavailable(
        &self,
    ) -> Result<TailscaleProxyFallbackResult, PlatformError> {
        let lease = self.platform.acquire_lifecycle_lock()?;
        let result = self.fallback_to_direct_locked();
        let release = self.platform.release_lifecycle_lock(&lease);
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(result), Ok(())) => Ok(result),
        }
    }

    pub fn restore_explicit_if_proxy_path_ready(
        &self,
    ) -> Result<TailscaleProxyFallbackResult, PlatformError> {
        let lease = self.platform.acquire_lifecycle_lock()?;
        let result = self.restore_explicit_locked();
        let release = self.platform.release_lifecycle_lock(&lease);
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(result), Ok(())) => Ok(result),
        }
    }

    fn fallback_to_direct_locked(&self) -> Result<TailscaleProxyFallbackResult, PlatformError> {
        let features = self.persisted_features()?;
        if !features.tailscale_explicit_proxy_enabled {
            return Ok(TailscaleProxyFallbackResult::NotNeeded);
        }
        let observed = self.tailscale_probe.observe_tailscale()?;
        let mode = persisted_tailscale_mode(&observed)?;
        if mode == TailscaleMode::Disabled {
            return Ok(TailscaleProxyFallbackResult::NotNeeded);
        }
        match observed.environment {
            Probe::Known(TailscaleEnvironment::Direct) => {
                return Ok(TailscaleProxyFallbackResult::NotNeeded)
            }
            Probe::Known(TailscaleEnvironment::MihomoExplicit) => {}
            Probe::Unknown(reason) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "tailscaled environment is unknown: {reason}"
                )))
            }
        }
        if self.tailscale_probe.probe_explicit_proxy_path()? {
            return Ok(TailscaleProxyFallbackResult::NotNeeded);
        }

        let tailscale_actions_applied =
            self.restart_tailscale(&observed, TailscaleEnvironment::Direct)?;
        let final_observed = self.tailscale_probe.observe_tailscale()?;
        self.require_environment_and_readiness(&final_observed, TailscaleEnvironment::Direct)?;
        Ok(TailscaleProxyFallbackResult::DirectRestored {
            tailscale_actions_applied,
        })
    }

    fn restore_explicit_locked(&self) -> Result<TailscaleProxyFallbackResult, PlatformError> {
        let features = self.persisted_features()?;
        if !features.tailscale_explicit_proxy_enabled {
            return Ok(TailscaleProxyFallbackResult::NotNeeded);
        }
        let observed = self.tailscale_probe.observe_tailscale()?;
        let mode = persisted_tailscale_mode(&observed)?;
        if mode == TailscaleMode::Disabled {
            return Ok(TailscaleProxyFallbackResult::NotNeeded);
        }
        match observed.environment {
            Probe::Known(TailscaleEnvironment::MihomoExplicit) => {
                return Ok(TailscaleProxyFallbackResult::NotNeeded)
            }
            Probe::Known(TailscaleEnvironment::Direct) => {}
            Probe::Unknown(reason) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "tailscaled environment is unknown: {reason}"
                )))
            }
        }
        if !self.tailscale_probe.probe_explicit_proxy_path()? {
            return Ok(TailscaleProxyFallbackResult::NotNeeded);
        }

        let tailscale_actions_applied =
            self.restart_tailscale(&observed, TailscaleEnvironment::MihomoExplicit)?;
        let final_observed = self.tailscale_probe.observe_tailscale()?;
        if final_observed.environment != Probe::Known(TailscaleEnvironment::MihomoExplicit) {
            return Err(PlatformError::UnsafeToCutOver(
                "tailscaled explicit-proxy recovery did not reach the exact fixed environment"
                    .to_owned(),
            ));
        }
        if !self.tailscale_probe.probe_explicit_proxy_path()? {
            let direct_recovery = self
                .restart_tailscale(&final_observed, TailscaleEnvironment::Direct)
                .map(|_| ());
            return match direct_recovery {
                Ok(()) => Err(PlatformError::UnsafeToCutOver(
                    "explicit-proxy recovery lost its path; restored Direct environment".to_owned(),
                )),
                Err(error) => Err(PlatformError::UnsafeToCutOver(format!(
                    "explicit-proxy recovery lost its path and Direct recovery failed: {error}"
                ))),
            };
        }
        Ok(TailscaleProxyFallbackResult::ExplicitRestored {
            tailscale_actions_applied,
        })
    }

    fn persisted_features(&self) -> Result<ProxyFeaturesV1, PlatformError> {
        let observed = self.probe.observe_proxy()?;
        read_supported_proxy_features(&observed)
    }

    fn restart_tailscale(
        &self,
        observed: &crate::domain::tailscale::TailscaleObserved,
        environment: TailscaleEnvironment,
    ) -> Result<usize, PlatformError> {
        ProxyFeatureCoordinator::new(
            self.platform,
            self.probe,
            self.tailscale,
            self.tailscale_probe,
            self.clock,
        )
        .restart_tailscale(observed, environment)
    }

    fn require_environment_and_readiness(
        &self,
        observed: &crate::domain::tailscale::TailscaleObserved,
        environment: TailscaleEnvironment,
    ) -> Result<(), PlatformError> {
        if observed.environment != Probe::Known(environment) {
            return Err(PlatformError::UnsafeToCutOver(format!(
                "tailscaled recovery did not reach the exact {environment:?} environment"
            )));
        }
        let mode = persisted_tailscale_mode(observed)?;
        let ordinary_router_ready = self
            .probe
            .observe_network()?
            .ready_for(&crate::domain::network::NetworkDesired::forwarding());
        let desired = TailscaleDesired { mode };
        if mode != TailscaleMode::Disabled
            && !observed.ready_for(&desired, ordinary_router_ready)
            && !surface_free_login_ready(observed, &desired)
        {
            return Err(PlatformError::UnsafeToCutOver(
                "tailscaled recovery did not restore ready or surface-free login state".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MihomoDirectRecoveryResult {
    NotNeeded,
    AlreadyDirect,
    Restored { tailscale_actions_applied: usize },
}

pub struct MihomoDirectRecoveryApplication<'a> {
    platform: &'a dyn RouterPlatformPort,
    probe: &'a dyn SystemProbePort,
    tailscale: &'a dyn TailscalePlatformPort,
    tailscale_probe: &'a dyn TailscaleProbePort,
    clock: &'a dyn ClockPort,
}

impl<'a> MihomoDirectRecoveryApplication<'a> {
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

    pub fn recover_if_core_unavailable(&self) -> Result<MihomoDirectRecoveryResult, PlatformError> {
        let lease = self.platform.acquire_lifecycle_lock()?;
        let result = self.recover_locked();
        let release = self.platform.release_lifecycle_lock(&lease);
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(result), Ok(())) => Ok(result),
        }
    }

    fn recover_locked(&self) -> Result<MihomoDirectRecoveryResult, PlatformError> {
        let proxy = self.probe.observe_proxy()?;
        let features = read_supported_proxy_features(&proxy)?;
        if !features.tailscale_explicit_proxy_enabled {
            return Ok(MihomoDirectRecoveryResult::NotNeeded);
        }
        if !core_unavailability_is_confirmed(&proxy)? {
            return Ok(MihomoDirectRecoveryResult::NotNeeded);
        }

        let observed = self.tailscale_probe.observe_tailscale()?;
        let mode = persisted_tailscale_mode(&observed)?;
        let ordinary_router_ready = self
            .probe
            .observe_network()?
            .ready_for(&crate::domain::network::NetworkDesired::forwarding());
        let desired = TailscaleDesired { mode };
        let readiness = observed.readiness(&desired, ordinary_router_ready);
        match observed.environment {
            Probe::Known(TailscaleEnvironment::Direct)
                if mode == TailscaleMode::Disabled
                    || !ordinary_router_ready
                    || matches!(readiness, TailscaleReadiness::Ready { .. })
                    || surface_free_login_ready(&observed, &desired) =>
            {
                return Ok(MihomoDirectRecoveryResult::AlreadyDirect);
            }
            Probe::Known(TailscaleEnvironment::Direct | TailscaleEnvironment::MihomoExplicit) => {}
            Probe::Unknown(reason) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "tailscaled environment is unknown: {reason}"
                )));
            }
        }

        let coordinator = ProxyFeatureCoordinator::new(
            self.platform,
            self.probe,
            self.tailscale,
            self.tailscale_probe,
            self.clock,
        );
        let tailscale_actions_applied =
            coordinator.restart_tailscale(&observed, TailscaleEnvironment::Direct)?;
        let final_observed = self.tailscale_probe.observe_tailscale()?;
        if final_observed.environment != Probe::Known(TailscaleEnvironment::Direct) {
            return Err(PlatformError::UnsafeToCutOver(
                "tailscaled Direct recovery did not reach the exact fixed environment".to_owned(),
            ));
        }
        let mode = persisted_tailscale_mode(&final_observed)?;
        let ordinary_router_ready = self
            .probe
            .observe_network()?
            .ready_for(&crate::domain::network::NetworkDesired::forwarding());
        let desired = TailscaleDesired { mode };
        let readiness = final_observed.readiness(&desired, ordinary_router_ready);
        if mode != TailscaleMode::Disabled
            && !matches!(readiness, TailscaleReadiness::Ready { .. })
            && !surface_free_login_ready(&final_observed, &desired)
        {
            return Err(PlatformError::UnsafeToCutOver(
                "tailscaled Direct recovery did not restore ready or surface-free login state"
                    .to_owned(),
            ));
        }
        Ok(MihomoDirectRecoveryResult::Restored {
            tailscale_actions_applied,
        })
    }
}

fn read_supported_proxy_features(
    observed: &ProxyObserved,
) -> Result<ProxyFeaturesV1, PlatformError> {
    match &observed.persisted_features {
        Probe::Known(features) if features.supported() => Ok(*features),
        Probe::Known(_) => Err(PlatformError::ProbeFailed(
            "persisted proxy feature version is unsupported".to_owned(),
        )),
        Probe::Unknown(reason) => Err(PlatformError::ProbeFailed(format!(
            "persisted proxy features are unknown: {reason}"
        ))),
    }
}

fn core_unavailability_is_confirmed(proxy: &ProxyObserved) -> Result<bool, PlatformError> {
    let mut unavailable = false;
    for (surface, probe) in [
        ("process identity", &proxy.process_identity_valid),
        ("runtime config", &proxy.runtime_config_valid),
        ("mixed port", &proxy.mixed_port_ready),
    ] {
        match probe {
            Probe::Known(true) => {}
            Probe::Known(false) => unavailable = true,
            Probe::Unknown(reason) => {
                return Err(PlatformError::ProbeFailed(format!(
                    "Mihomo Core {surface} is unknown: {reason}"
                )));
            }
        }
    }
    Ok(unavailable)
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
        let tailscale_transition =
            current_environment != target_environment || move_tailscale_direct;

        let mut tailscale_actions_applied = 0;
        if move_tailscale_direct {
            match self.restart_tailscale(&initial_tailscale, TailscaleEnvironment::Direct) {
                Ok(applied) => tailscale_actions_applied += applied,
                Err(error) => {
                    return Err(self.recover_after_failure(
                        error,
                        previous,
                        &desired.direct_macs,
                        current_environment,
                        true,
                    ));
                }
            }
        }

        let mut proxy_actions_applied = 0;
        for action in &actions {
            if let ProxyAction::InstallInterceptionEntry { token } = action {
                let preinterception = match self.probe.observe_proxy() {
                    Ok(observed) => observed,
                    Err(error) => {
                        return Err(self.recover_after_failure(
                            error,
                            previous,
                            &desired.direct_macs,
                            current_environment,
                            tailscale_transition,
                        ));
                    }
                };
                if !preinterception.ready_for_interception(token, &desired.direct_macs) {
                    return Err(self.recover_after_failure(
                        PlatformError::UnsafeToCutOver(
                            "Mihomo TUN ownership changed before interception commit".to_owned(),
                        ),
                        previous,
                        &desired.direct_macs,
                        current_environment,
                        tailscale_transition,
                    ));
                }
            }
            if let Err(error) = self.platform.apply_proxy(action) {
                return Err(self.recover_after_failure(
                    error,
                    previous,
                    &desired.direct_macs,
                    current_environment,
                    tailscale_transition,
                ));
            }
            proxy_actions_applied += 1;
        }

        if target_environment == TailscaleEnvironment::MihomoExplicit
            && (current_environment == TailscaleEnvironment::Direct || move_tailscale_direct)
        {
            let observed = match self.probe.observe_proxy() {
                Ok(observed) => observed,
                Err(error) => {
                    return Err(self.recover_after_failure(
                        error,
                        previous,
                        &desired.direct_macs,
                        current_environment,
                        true,
                    ));
                }
            };
            if !observed.core_ready() {
                return Err(self.recover_after_failure(
                    PlatformError::UnsafeToCutOver(
                        "Mihomo core and fixed mixed port are not ready for tailscaled".to_owned(),
                    ),
                    previous,
                    &desired.direct_macs,
                    current_environment,
                    true,
                ));
            }
            let tailscale_observed = match self.tailscale_probe.observe_tailscale() {
                Ok(observed) => observed,
                Err(error) => {
                    return Err(self.recover_after_failure(
                        error,
                        previous,
                        &desired.direct_macs,
                        current_environment,
                        true,
                    ));
                }
            };
            match self.restart_tailscale(&tailscale_observed, TailscaleEnvironment::MihomoExplicit)
            {
                Ok(applied) => tailscale_actions_applied += applied,
                Err(error) => {
                    return Err(self.recover_after_failure(
                        error,
                        previous,
                        &desired.direct_macs,
                        current_environment,
                        true,
                    ));
                }
            }
        }

        let final_tailscale = match self.tailscale_probe.observe_tailscale() {
            Ok(observed) => observed,
            Err(error) => {
                return Err(self.recover_after_failure(
                    error,
                    previous,
                    &desired.direct_macs,
                    current_environment,
                    tailscale_transition,
                ));
            }
        };
        if final_tailscale.environment != Probe::Known(target_environment) {
            return Err(self.recover_after_failure(
                PlatformError::UnsafeToCutOver(
                    "tailscaled exact environment did not reach the requested state".to_owned(),
                ),
                previous,
                &desired.direct_macs,
                current_environment,
                tailscale_transition,
            ));
        }
        if desired.tailscale_explicit_proxy_enabled {
            let mode = match persisted_tailscale_mode(&final_tailscale) {
                Ok(mode) => mode,
                Err(error) => {
                    return Err(self.recover_after_failure(
                        error,
                        previous,
                        &desired.direct_macs,
                        current_environment,
                        tailscale_transition,
                    ));
                }
            };
            let ordinary_router_ready = match self.probe.observe_network() {
                Ok(network) => {
                    network.ready_for(&crate::domain::network::NetworkDesired::forwarding())
                }
                Err(error) => {
                    return Err(self.recover_after_failure(
                        error,
                        previous,
                        &desired.direct_macs,
                        current_environment,
                        tailscale_transition,
                    ));
                }
            };
            if mode == TailscaleMode::Disabled
                || !final_tailscale.ready_for(&TailscaleDesired { mode }, ordinary_router_ready)
            {
                return Err(self.recover_after_failure(
                    PlatformError::UnsafeToCutOver(
                        "Tailscale access mode is not ready for explicit proxy cutover".to_owned(),
                    ),
                    previous,
                    &desired.direct_macs,
                    current_environment,
                    tailscale_transition,
                ));
            }
            match self.tailscale_probe.probe_explicit_proxy_path() {
                Ok(true) => {}
                Ok(false) => {
                    return Err(self.recover_after_failure(
                        PlatformError::UnsafeToCutOver(
                            "fixed Tailscale explicit proxy path is unavailable".to_owned(),
                        ),
                        previous,
                        &desired.direct_macs,
                        current_environment,
                        tailscale_transition,
                    ));
                }
                Err(error) => {
                    return Err(self.recover_after_failure(
                        error,
                        previous,
                        &desired.direct_macs,
                        current_environment,
                        tailscale_transition,
                    ));
                }
            }
        }
        if commit_features {
            if let Some(commit) = commit {
                if let Err(error) = self.platform.apply_proxy(&commit) {
                    return Err(self.recover_after_failure(
                        error,
                        previous,
                        &desired.direct_macs,
                        current_environment,
                        tailscale_transition,
                    ));
                }
                proxy_actions_applied += 1;
            }
        }
        let observed = match self.probe.observe_proxy() {
            Ok(observed) => observed,
            Err(error) => {
                return Err(self.recover_after_failure(
                    error,
                    previous,
                    &desired.direct_macs,
                    current_environment,
                    tailscale_transition,
                ));
            }
        };
        let ready = if commit_features {
            observed.ready_for(desired)
        } else {
            observed.persisted_features == Probe::Known(previous)
                && observed.runtime_ready_for_without_ordinary_nat(desired)
        };
        if !ready {
            return Err(self.recover_after_failure(
                PlatformError::UnsafeToCutOver(
                    "proxy feature cutover failed strict final readiness".to_owned(),
                ),
                previous,
                &desired.direct_macs,
                current_environment,
                tailscale_transition,
            ));
        }
        Ok(ProxyFeatureReconcileResult {
            observed,
            proxy_actions_applied,
            tailscale_actions_applied,
        })
    }

    fn recover_after_failure(
        &self,
        primary: PlatformError,
        previous: ProxyFeaturesV1,
        direct_macs: &std::collections::BTreeSet<crate::domain::device_policy::LanDeviceMac>,
        environment: TailscaleEnvironment,
        restore_tailscale: bool,
    ) -> PlatformError {
        let mut recovery_failures = Vec::new();
        if let Err(error) = self.restore_proxy(previous, direct_macs) {
            recovery_failures.push(format!("proxy runtime: {error}"));
        }
        if restore_tailscale {
            if let Err(error) = self.restore_tailscale_environment(environment) {
                recovery_failures.push(format!("Tailscale environment: {error}"));
            }
        }
        if recovery_failures.is_empty() {
            primary
        } else {
            PlatformError::UnsafeToCutOver(format!(
                "cutover failed ({primary}); recovery could not be confirmed ({})",
                recovery_failures.join("; ")
            ))
        }
    }

    fn restart_tailscale(
        &self,
        observed: &crate::domain::tailscale::TailscaleObserved,
        environment: TailscaleEnvironment,
    ) -> Result<usize, PlatformError> {
        let new_token = self.clock.ownership_token("hyz-tailscale")?;
        let mode = persisted_tailscale_mode(observed)?;
        let actions = tailscale_environment_plan(observed, environment, &new_token)?;
        let mut applied = 0;
        for action in &actions {
            self.tailscale.apply_tailscale(action)?;
            applied += 1;
        }

        let restarted = self.tailscale_probe.observe_tailscale()?;
        if mode == TailscaleMode::Disabled {
            return Ok(applied);
        }
        if restarted.environment != Probe::Known(environment) {
            return Err(PlatformError::UnsafeToCutOver(
                "tailscaled restart did not reach the exact requested environment".to_owned(),
            ));
        }
        let desired = TailscaleDesired { mode };
        if surface_free_login_ready(&restarted, &desired) {
            return Ok(applied);
        }

        let network = self.probe.observe_network()?;
        let mut restore = tailscale_plan(&desired, &restarted, &network, &new_token)?;
        restore.retain(|action| !matches!(action, TailscaleAction::CommitDesiredMode { .. }));
        for action in &restore {
            self.tailscale.apply_tailscale(action)?;
            applied += 1;
        }
        let final_observed = self.tailscale_probe.observe_tailscale()?;
        if final_observed.environment != Probe::Known(environment)
            || !final_observed.ready_for(
                &desired,
                network.ready_for(&crate::domain::network::NetworkDesired::forwarding()),
            )
        {
            return Err(PlatformError::UnsafeToCutOver(
                "tailscaled restart did not restore strict persisted-mode readiness".to_owned(),
            ));
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
    let mode = persisted_tailscale_mode(observed)?;
    let mut actions = tailscale_shutdown_plan(observed)?;
    if mode != TailscaleMode::Disabled {
        actions.extend([
            TailscaleAction::StartBackend {
                token: new_token.to_owned(),
                environment,
            },
            TailscaleAction::WaitForBackend,
            TailscaleAction::SetFixedPreferences,
        ]);
    }
    Ok(actions)
}

fn surface_free_login_ready(
    observed: &crate::domain::tailscale::TailscaleObserved,
    desired: &TailscaleDesired,
) -> bool {
    observed.readiness(desired, false) == TailscaleReadiness::NeedsLogin
        && matches!(
            (&observed.process, &observed.socket, &observed.interface),
            (
                Probe::Known(TailscaleProcessState::OwnedLive { token: process }),
                Probe::Known(OwnedResource::Owned { token: socket }),
                Probe::Known(OwnedResource::Owned { token: interface }),
            ) if process == socket && process == interface
        )
        && observed.preferences == Probe::Known(TailscalePreferences::FIXED)
        && observed.ipv4 == Probe::Known(None)
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
            network::{NetworkObserved, OwnedResource, UplinkObserved},
            tailscale::{TailscaleBackendState, TailscalePreferences, TailscaleProcessState},
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
            connection: Probe::Known(TailscaleConnectionKind::Direct),
        }
    }

    #[test]
    fn environment_plan_removes_remote_surfaces_before_restarting_exact_backend() {
        let actions = tailscale_environment_plan(
            &tailscale_observed(TailscaleMode::LanSubnetAccess),
            TailscaleEnvironment::MihomoExplicit,
            "new",
        )
        .unwrap();
        assert_eq!(
            actions,
            vec![
                TailscaleAction::RemoveSubnetFirewall {
                    token: "subnet".to_owned(),
                },
                TailscaleAction::ClearAdvertisedRoute,
                TailscaleAction::RemoveRouterFirewall {
                    token: "firewall".to_owned(),
                },
                TailscaleAction::StopBackend {
                    token: "old".to_owned(),
                },
                TailscaleAction::StartBackend {
                    token: "new".to_owned(),
                    environment: TailscaleEnvironment::MihomoExplicit,
                },
                TailscaleAction::WaitForBackend,
                TailscaleAction::SetFixedPreferences,
            ]
        );
    }

    #[test]
    fn environment_plan_recovers_an_absent_required_backend_in_direct_mode() {
        let mut observed = tailscale_observed(TailscaleMode::RouterOnly);
        observed.process = Probe::Known(TailscaleProcessState::Absent);
        observed.backend_state = Probe::Known(TailscaleBackendState::Stopped);
        observed.socket = Probe::Known(OwnedResource::Absent);
        observed.interface = Probe::Known(OwnedResource::Absent);
        observed.authenticated = Probe::Known(false);
        observed.ipv4 = Probe::Known(None);

        assert_eq!(
            tailscale_environment_plan(&observed, TailscaleEnvironment::Direct, "replacement")
                .unwrap(),
            vec![
                TailscaleAction::RemoveRouterFirewall {
                    token: "firewall".to_owned(),
                },
                TailscaleAction::StartBackend {
                    token: "replacement".to_owned(),
                    environment: TailscaleEnvironment::Direct,
                },
                TailscaleAction::WaitForBackend,
                TailscaleAction::SetFixedPreferences,
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

    #[test]
    fn surface_free_login_requires_exact_owned_runtime_nodes() {
        let desired = TailscaleDesired {
            mode: TailscaleMode::RouterOnly,
        };
        let mut observed = tailscale_observed(TailscaleMode::RouterOnly);
        observed.backend_state = Probe::Known(TailscaleBackendState::NeedsLogin);
        observed.authenticated = Probe::Known(false);
        observed.ipv4 = Probe::Known(None);
        observed.route_advertised = Probe::Known(false);
        observed.router_firewall = Probe::Known(OwnedResource::Absent);
        observed.subnet_firewall = Probe::Known(OwnedResource::Absent);
        assert!(surface_free_login_ready(&observed, &desired));

        observed.socket = Probe::Known(OwnedResource::Foreign);
        assert!(!surface_free_login_ready(&observed, &desired));
        observed.socket = Probe::Known(OwnedResource::Owned {
            token: "replacement".to_owned(),
        });
        assert!(!surface_free_login_ready(&observed, &desired));
    }

    fn coordinator_proxy(features: ProxyFeaturesV1) -> ProxyObserved {
        let core = features.mihomo_required();
        let lan = features.lan_tun_enabled;
        ProxyObserved {
            persisted_features: Probe::Known(features),
            process_identity_valid: Probe::Known(core),
            watcher_identity_valid: Probe::Known(lan),
            runtime_config_valid: Probe::Known(core),
            mixed_port_ready: Probe::Known(core),
            tun_interface: Probe::Known(if lan {
                OwnedResource::Owned {
                    token: "tun".to_owned(),
                }
            } else {
                OwnedResource::Absent
            }),
            tun_firewall: Probe::Known(if lan {
                OwnedResource::Owned {
                    token: "tun".to_owned(),
                }
            } else {
                OwnedResource::Absent
            }),
            tun_active_uplink: Probe::Known(None),
            policy_rule_present: Probe::Known(lan),
            policy_route_present: Probe::Known(lan),
            interception_entry_present: Probe::Known(lan),
            ordinary_nat_confirmed: Probe::Known(true),
            active_direct_macs: Probe::Known(Default::default()),
        }
    }

    struct CoordinatorFake {
        proxy: Mutex<ProxyObserved>,
        tailscale: Mutex<crate::domain::tailscale::TailscaleObserved>,
        proxy_path_ready: Mutex<bool>,
        events: Mutex<Vec<String>>,
        replace_tun_before_interception: bool,
        start_requires_login: bool,
        fail_start_environment: Mutex<Option<TailscaleEnvironment>>,
        ordinary_router_ready: bool,
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
                start_requires_login: false,
                fail_start_environment: Mutex::new(None),
                ordinary_router_ready: true,
            }
        }

        fn with_router_not_ready(mut self) -> Self {
            self.ordinary_router_ready = false;
            self
        }

        fn with_tun_replacement(mut self) -> Self {
            self.replace_tun_before_interception = true;
            self
        }

        fn with_login_required_after_restart(mut self) -> Self {
            self.start_requires_login = true;
            self
        }

        fn fail_start_once(&self, environment: TailscaleEnvironment) {
            *self.fail_start_environment.lock().unwrap() = Some(environment);
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
                ProxyAction::CreateTunChains {
                    token,
                    direct_macs,
                    active,
                } => {
                    observed.tun_firewall = Probe::Known(OwnedResource::Owned {
                        token: token.clone(),
                    });
                    observed.tun_active_uplink = Probe::Known(Some(active.clone()));
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
                ProxyAction::StopWatcher => observed.watcher_identity_valid = Probe::Known(false),
                ProxyAction::RemoveInterceptionEntry { .. } => {
                    observed.interception_entry_present = Probe::Known(false)
                }
                ProxyAction::RemovePolicyRule => observed.policy_rule_present = Probe::Known(false),
                ProxyAction::RemovePolicyRoute => {
                    observed.policy_route_present = Probe::Known(false)
                }
                ProxyAction::RemoveTunChains { .. } => {
                    observed.tun_firewall = Probe::Known(OwnedResource::Absent);
                    observed.tun_active_uplink = Probe::Known(None);
                    observed.active_direct_macs = Probe::Known(Default::default());
                }
                ProxyAction::StopCore => {
                    observed.process_identity_valid = Probe::Known(false);
                    observed.mixed_port_ready = Probe::Known(false);
                    observed.tun_interface = Probe::Known(OwnedResource::Absent);
                    observed.tun_active_uplink = Probe::Known(None);
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
                ethernet_lan_attached: Probe::Known(true),
                management_services_healthy: Probe::Known(true),
                ethernet_uplink: UplinkObserved::unavailable(),
                wifi_uplink: UplinkObserved::wifi_only_route(Probe::Known(
                    self.ordinary_router_ready,
                )),
                ipv4_forwarding: Probe::Known(true),
                previous_ipv4_forwarding: Probe::Known(Some(false)),
                router_firewall: Probe::Known(OwnedResource::Owned {
                    token: "router".to_owned(),
                }),
                firewall_wan_set: Probe::Known(Some(crate::domain::network::RouterWanSet::Wifi)),
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
            if let TailscaleAction::StartBackend { environment, .. } = action {
                let mut failure = self.fail_start_environment.lock().unwrap();
                if *failure == Some(*environment) {
                    *failure = None;
                    return Err(PlatformError::CommandFailed(format!(
                        "injected tailscaled start failure for {environment:?}"
                    )));
                }
            }
            let mut observed = self.tailscale.lock().unwrap();
            match action {
                TailscaleAction::StopBackend { .. } => {
                    observed.process = Probe::Known(TailscaleProcessState::Absent);
                    observed.socket = Probe::Known(OwnedResource::Absent);
                    observed.interface = Probe::Known(OwnedResource::Absent);
                    observed.backend_state = Probe::Known(TailscaleBackendState::Stopped);
                    observed.authenticated = Probe::Known(false);
                    observed.ipv4 = Probe::Known(None);
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
                    if self.start_requires_login {
                        observed.backend_state = Probe::Known(TailscaleBackendState::NeedsLogin);
                        observed.authenticated = Probe::Known(false);
                        observed.ipv4 = Probe::Known(None);
                    } else {
                        observed.backend_state = Probe::Known(TailscaleBackendState::Running);
                        observed.authenticated = Probe::Known(true);
                        observed.ipv4 = Probe::Known(Some("100.64.0.1".parse().unwrap()));
                    }
                }
                TailscaleAction::SetFixedPreferences => {
                    observed.preferences = Probe::Known(TailscalePreferences::FIXED)
                }
                TailscaleAction::AdvertiseLanRoute => {
                    observed.route_advertised = Probe::Known(true)
                }
                TailscaleAction::ClearAdvertisedRoute => {
                    observed.route_advertised = Probe::Known(false)
                }
                TailscaleAction::InstallRouterFirewall { token } => {
                    observed.router_firewall = Probe::Known(OwnedResource::Owned {
                        token: token.clone(),
                    })
                }
                TailscaleAction::RemoveRouterFirewall { .. } => {
                    observed.router_firewall = Probe::Known(OwnedResource::Absent)
                }
                TailscaleAction::InstallSubnetFirewall { token } => {
                    observed.subnet_firewall = Probe::Known(OwnedResource::Owned {
                        token: token.clone(),
                    })
                }
                TailscaleAction::RemoveSubnetFirewall { .. } => {
                    observed.subnet_firewall = Probe::Known(OwnedResource::Absent)
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
        let stop_backend = events
            .iter()
            .position(|event| event.contains("tailscale:StopBackend"))
            .unwrap();
        let proxied = events
            .iter()
            .position(|event| event.contains("environment: MihomoExplicit"))
            .unwrap();
        let path_probe = events
            .iter()
            .position(|event| event == "tailscale:path-probe")
            .unwrap();
        let commit = events
            .iter()
            .position(|event| event.contains("proxy:CommitFeatures"))
            .unwrap();
        assert!(mixed < stop_backend);
        assert!(stop_backend < proxied && proxied < path_probe);
        assert!(path_probe < commit);
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
    fn failed_proxied_backend_start_restores_direct_backend_and_listener() {
        let fake = CoordinatorFake::new(ProxyFeaturesV1::disabled(), TailscaleEnvironment::Direct);
        fake.fail_start_once(TailscaleEnvironment::MihomoExplicit);

        let error = ProxyFeatureCoordinator::new(&fake, &fake, &fake, &fake, &fake)
            .reconcile(&ProxyDesired {
                lan_tun_enabled: false,
                tailscale_explicit_proxy_enabled: true,
                direct_macs: Default::default(),
            })
            .unwrap_err();

        assert!(matches!(error, PlatformError::CommandFailed(_)));
        let tailscale = fake.tailscale.lock().unwrap().clone();
        assert_eq!(
            tailscale.environment,
            Probe::Known(TailscaleEnvironment::Direct)
        );
        assert!(matches!(
            tailscale.process,
            Probe::Known(TailscaleProcessState::OwnedLive { .. })
        ));
        let events = fake.events();
        let failed_proxied = events
            .iter()
            .position(|event| event.contains("environment: MihomoExplicit"))
            .unwrap();
        let restored_direct = events
            .iter()
            .rposition(|event| event.contains("environment: Direct"))
            .unwrap();
        assert!(failed_proxied < restored_direct);
    }

    #[test]
    fn login_required_during_proxy_cutover_restores_surface_free_direct_backend() {
        let fake = CoordinatorFake::new(ProxyFeaturesV1::disabled(), TailscaleEnvironment::Direct)
            .with_login_required_after_restart();

        let error = ProxyFeatureCoordinator::new(&fake, &fake, &fake, &fake, &fake)
            .reconcile(&ProxyDesired {
                lan_tun_enabled: false,
                tailscale_explicit_proxy_enabled: true,
                direct_macs: Default::default(),
            })
            .expect_err("explicit proxy cutover must not commit while login is required");

        assert!(matches!(error, PlatformError::UnsafeToCutOver(_)));
        assert_eq!(
            fake.proxy.lock().unwrap().persisted_features,
            Probe::Known(ProxyFeaturesV1::disabled())
        );
        let tailscale = fake.tailscale.lock().unwrap().clone();
        assert_eq!(
            tailscale.environment,
            Probe::Known(TailscaleEnvironment::Direct)
        );
        assert_eq!(
            tailscale.backend_state,
            Probe::Known(TailscaleBackendState::NeedsLogin)
        );
        assert_eq!(tailscale.authenticated, Probe::Known(false));
        assert_eq!(tailscale.ipv4, Probe::Known(None));
        assert_eq!(tailscale.route_advertised, Probe::Known(false));
        assert_eq!(
            tailscale.router_firewall,
            Probe::Known(OwnedResource::Absent)
        );
        assert_eq!(
            tailscale.subnet_firewall,
            Probe::Known(OwnedResource::Absent)
        );
        assert!(!fake.events().iter().any(|event| {
            event.contains("InstallRouterFirewall") || event.contains("InstallSubnetFirewall")
        }));
    }

    #[test]
    fn daemon_direct_recovery_does_not_mutate_tailscale_when_core_probe_is_unknown() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(false, true),
            TailscaleEnvironment::MihomoExplicit,
        );
        fake.proxy.lock().unwrap().mixed_port_ready =
            Probe::Unknown("transient fdinfo read failure".to_owned());

        let error = MihomoDirectRecoveryApplication::new(&fake, &fake, &fake, &fake, &fake)
            .recover_if_core_unavailable()
            .expect_err("unknown Core readiness must not trigger fail-open mutation");

        assert!(matches!(error, PlatformError::ProbeFailed(_)));
        assert_eq!(
            fake.tailscale.lock().unwrap().environment,
            Probe::Known(TailscaleEnvironment::MihomoExplicit)
        );
        assert_eq!(fake.events(), vec!["router:lock", "router:release"]);
    }

    #[test]
    fn daemon_direct_recovery_is_not_needed_when_core_is_confirmed_ready() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(false, true),
            TailscaleEnvironment::MihomoExplicit,
        );

        assert_eq!(
            MihomoDirectRecoveryApplication::new(&fake, &fake, &fake, &fake, &fake)
                .recover_if_core_unavailable(),
            Ok(MihomoDirectRecoveryResult::NotNeeded)
        );
        assert_eq!(
            fake.tailscale.lock().unwrap().environment,
            Probe::Known(TailscaleEnvironment::MihomoExplicit)
        );
        assert_eq!(fake.events(), vec!["router:lock", "router:release"]);
    }

    #[test]
    fn daemon_direct_recovery_handles_01_without_a_lan_tun_watcher() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(false, true),
            TailscaleEnvironment::MihomoExplicit,
        );
        {
            let mut proxy = fake.proxy.lock().unwrap();
            proxy.process_identity_valid = Probe::Known(false);
            proxy.mixed_port_ready = Probe::Known(false);
            assert_eq!(proxy.watcher_identity_valid, Probe::Known(false));
        }

        assert_eq!(
            MihomoDirectRecoveryApplication::new(&fake, &fake, &fake, &fake, &fake)
                .recover_if_core_unavailable(),
            Ok(MihomoDirectRecoveryResult::Restored {
                tailscale_actions_applied: 6,
            })
        );
        assert_eq!(
            fake.proxy.lock().unwrap().persisted_features,
            Probe::Known(ProxyFeaturesV1::new(false, true))
        );
        assert_eq!(
            fake.tailscale.lock().unwrap().environment,
            Probe::Known(TailscaleEnvironment::Direct)
        );
    }

    #[test]
    fn daemon_direct_recovery_accepts_surface_free_login_state() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(false, true),
            TailscaleEnvironment::MihomoExplicit,
        )
        .with_login_required_after_restart();
        {
            let mut proxy = fake.proxy.lock().unwrap();
            proxy.process_identity_valid = Probe::Known(false);
            proxy.mixed_port_ready = Probe::Known(false);
        }

        assert!(matches!(
            MihomoDirectRecoveryApplication::new(&fake, &fake, &fake, &fake, &fake)
                .recover_if_core_unavailable(),
            Ok(MihomoDirectRecoveryResult::Restored { .. })
        ));
        let tailscale = fake.tailscale.lock().unwrap().clone();
        assert_eq!(
            tailscale.environment,
            Probe::Known(TailscaleEnvironment::Direct)
        );
        assert_eq!(
            tailscale.readiness(
                &TailscaleDesired {
                    mode: TailscaleMode::RouterOnly
                },
                true
            ),
            TailscaleReadiness::NeedsLogin
        );
        assert_eq!(
            MihomoDirectRecoveryApplication::new(&fake, &fake, &fake, &fake, &fake)
                .recover_if_core_unavailable(),
            Ok(MihomoDirectRecoveryResult::AlreadyDirect)
        );
    }

    #[test]
    fn daemon_direct_recovery_is_a_bounded_noop_after_direct_is_restored() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(false, true),
            TailscaleEnvironment::Direct,
        );
        {
            let mut proxy = fake.proxy.lock().unwrap();
            proxy.process_identity_valid = Probe::Known(false);
            proxy.mixed_port_ready = Probe::Known(false);
        }

        assert_eq!(
            MihomoDirectRecoveryApplication::new(&fake, &fake, &fake, &fake, &fake)
                .recover_if_core_unavailable(),
            Ok(MihomoDirectRecoveryResult::AlreadyDirect)
        );
        assert_eq!(fake.events(), vec!["router:lock", "router:release"]);
    }

    #[test]
    fn daemon_direct_recovery_restarts_an_absent_required_backend() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(false, true),
            TailscaleEnvironment::Direct,
        );
        {
            let mut proxy = fake.proxy.lock().unwrap();
            proxy.process_identity_valid = Probe::Known(false);
            proxy.mixed_port_ready = Probe::Known(false);
        }
        {
            let mut tailscale = fake.tailscale.lock().unwrap();
            tailscale.process = Probe::Known(TailscaleProcessState::Absent);
            tailscale.socket = Probe::Known(OwnedResource::Absent);
            tailscale.interface = Probe::Known(OwnedResource::Absent);
            tailscale.backend_state = Probe::Known(TailscaleBackendState::Stopped);
            tailscale.authenticated = Probe::Known(false);
            tailscale.ipv4 = Probe::Known(None);
        }

        assert!(matches!(
            MihomoDirectRecoveryApplication::new(&fake, &fake, &fake, &fake, &fake)
                .recover_if_core_unavailable(),
            Ok(MihomoDirectRecoveryResult::Restored { .. })
        ));
        assert!(matches!(
            fake.tailscale.lock().unwrap().process,
            Probe::Known(TailscaleProcessState::OwnedLive { .. })
        ));
    }

    #[test]
    fn daemon_direct_recovery_does_not_start_direct_backend_without_router_readiness() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(false, true),
            TailscaleEnvironment::Direct,
        )
        .with_router_not_ready();
        {
            let mut proxy = fake.proxy.lock().unwrap();
            proxy.process_identity_valid = Probe::Known(false);
            proxy.mixed_port_ready = Probe::Known(false);
        }
        {
            let mut tailscale = fake.tailscale.lock().unwrap();
            tailscale.process = Probe::Known(TailscaleProcessState::Absent);
            tailscale.socket = Probe::Known(OwnedResource::Absent);
            tailscale.interface = Probe::Known(OwnedResource::Absent);
            tailscale.backend_state = Probe::Known(TailscaleBackendState::Stopped);
            tailscale.authenticated = Probe::Known(false);
            tailscale.ipv4 = Probe::Known(None);
        }

        assert_eq!(
            MihomoDirectRecoveryApplication::new(&fake, &fake, &fake, &fake, &fake)
                .recover_if_core_unavailable(),
            Ok(MihomoDirectRecoveryResult::AlreadyDirect)
        );
        assert_eq!(
            fake.tailscale.lock().unwrap().process,
            Probe::Known(TailscaleProcessState::Absent)
        );
        assert_eq!(fake.events(), vec!["router:lock", "router:release"]);
    }

    #[test]
    fn explicit_proxy_path_fallback_requires_three_confirmed_failures() {
        let mut tracker = ExplicitProxyFallbackTracker::default();

        assert_eq!(
            tracker.observe(ExplicitProxyPathObservation::Unavailable),
            ExplicitProxyFallbackDecision::Hold
        );
        assert_eq!(
            tracker.observe(ExplicitProxyPathObservation::Unavailable),
            ExplicitProxyFallbackDecision::Hold
        );
        assert_eq!(
            tracker.observe(ExplicitProxyPathObservation::Unavailable),
            ExplicitProxyFallbackDecision::FallbackToDirect
        );
        assert_eq!(tracker.consecutive_failures(), 0);

        assert_eq!(
            tracker.observe(ExplicitProxyPathObservation::Ready),
            ExplicitProxyFallbackDecision::Hold
        );
        assert_eq!(tracker.consecutive_failures(), 0);
    }

    #[test]
    fn explicit_proxy_path_unknown_does_not_trigger_fallback() {
        let mut tracker = ExplicitProxyFallbackTracker::default();

        for _ in 0..10 {
            assert_eq!(
                tracker.observe(ExplicitProxyPathObservation::Unknown),
                ExplicitProxyFallbackDecision::Hold
            );
        }
        assert_eq!(tracker.consecutive_failures(), 0);
    }

    #[test]
    fn explicit_proxy_path_fallback_restores_direct_without_committing_proxy_features() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(false, true),
            TailscaleEnvironment::MihomoExplicit,
        );
        *fake.proxy_path_ready.lock().unwrap() = false;

        let result = TailscaleProxyFallbackApplication::new(&fake, &fake, &fake, &fake, &fake)
            .fallback_to_direct_if_unavailable()
            .unwrap();

        assert!(matches!(
            result,
            TailscaleProxyFallbackResult::DirectRestored { .. }
        ));
        assert_eq!(
            fake.proxy.lock().unwrap().persisted_features,
            Probe::Known(ProxyFeaturesV1::new(false, true))
        );
        assert_eq!(
            fake.tailscale.lock().unwrap().environment,
            Probe::Known(TailscaleEnvironment::Direct)
        );
        assert!(!fake
            .events()
            .iter()
            .any(|event| event.contains("CommitFeatures")));
    }

    #[test]
    fn explicit_proxy_path_recovers_to_mihomo_without_committing_proxy_features() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(false, true),
            TailscaleEnvironment::Direct,
        );

        let result = TailscaleProxyFallbackApplication::new(&fake, &fake, &fake, &fake, &fake)
            .restore_explicit_if_proxy_path_ready()
            .unwrap();

        assert!(matches!(
            result,
            TailscaleProxyFallbackResult::ExplicitRestored { .. }
        ));
        assert_eq!(
            fake.proxy.lock().unwrap().persisted_features,
            Probe::Known(ProxyFeaturesV1::new(false, true))
        );
        assert_eq!(
            fake.tailscale.lock().unwrap().environment,
            Probe::Known(TailscaleEnvironment::MihomoExplicit)
        );
        assert!(!fake
            .events()
            .iter()
            .any(|event| event.contains("CommitFeatures")));
    }

    #[test]
    fn explicit_proxy_path_recovery_waits_when_path_is_still_unavailable() {
        let fake = CoordinatorFake::new(
            ProxyFeaturesV1::new(false, true),
            TailscaleEnvironment::Direct,
        );
        *fake.proxy_path_ready.lock().unwrap() = false;

        assert_eq!(
            TailscaleProxyFallbackApplication::new(&fake, &fake, &fake, &fake, &fake)
                .restore_explicit_if_proxy_path_ready()
                .unwrap(),
            TailscaleProxyFallbackResult::NotNeeded
        );
        assert_eq!(
            fake.tailscale.lock().unwrap().environment,
            Probe::Known(TailscaleEnvironment::Direct)
        );
    }

    #[test]
    fn runtime_degradation_preserves_both_persisted_feature_flags() {
        let features = ProxyFeaturesV1::new(true, true);
        let fake = CoordinatorFake::new(features, TailscaleEnvironment::MihomoExplicit);

        let result = ProxyFeatureCoordinator::new(&fake, &fake, &fake, &fake, &fake)
            .reconcile_runtime_preserving_features(&ProxyDesired {
                lan_tun_enabled: false,
                tailscale_explicit_proxy_enabled: false,
                direct_macs: Default::default(),
            })
            .unwrap();

        assert_eq!(result.observed.persisted_features, Probe::Known(features));
        assert!(!fake
            .events()
            .iter()
            .any(|event| event.contains("proxy:CommitFeatures")));
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
