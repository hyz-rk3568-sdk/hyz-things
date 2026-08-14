use crate::{
    application::{
        ports::{
            CoreIdentity, CoreRecordState, FailOpenPlatformPort, FailOpenRetryKind, PlatformError,
        },
        reconcile::fail_open_plan,
    },
    domain::network::Probe,
};
use std::time::Duration;

pub const INTERNAL_WATCHER_ROLE: &str = "__hyz_internal_mihomo_watch";
const IDENTITY_WAIT_ATTEMPTS: usize = 50;
const IDENTITY_WAIT: Duration = Duration::from_millis(100);
const CORE_POLL: Duration = Duration::from_secs(2);
const LOCK_RETRY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatcherInvocation {
    pub core: CoreIdentity,
}

impl WatcherInvocation {
    pub fn parse_hidden(args: &[String]) -> Option<Result<Self, PlatformError>> {
        let [role, pid, start] = args else {
            return None;
        };
        if role != INTERNAL_WATCHER_ROLE {
            return None;
        }
        Some((|| {
            let pid = pid.parse::<u32>().map_err(|_| {
                PlatformError::InvalidState("internal watcher PID is invalid".to_owned())
            })?;
            let start_time = start.parse::<u64>().map_err(|_| {
                PlatformError::InvalidState("internal watcher start time is invalid".to_owned())
            })?;
            if pid == 0 || start_time == 0 {
                return Err(PlatformError::InvalidState(
                    "internal watcher identity must be nonzero".to_owned(),
                ));
            }
            Ok(Self {
                core: CoreIdentity { pid, start_time },
            })
        })())
    }

    pub fn exact_args(self) -> [String; 3] {
        [
            INTERNAL_WATCHER_ROLE.to_owned(),
            self.core.pid.to_string(),
            self.core.start_time.to_string(),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailOpenResult {
    Cleaned,
    ReplacementObserved,
    AlreadyClean,
}

pub struct MihomoFailOpenApplication<'a> {
    platform: &'a dyn FailOpenPlatformPort,
}

impl<'a> MihomoFailOpenApplication<'a> {
    pub fn new(platform: &'a dyn FailOpenPlatformPort) -> Self {
        Self { platform }
    }

    pub fn execute(&self, invocation: WatcherInvocation) -> Result<FailOpenResult, PlatformError> {
        self.repair_until_safe(invocation)
    }

    /// Ongoing watcher primitive: transient probe, lock, cleanup, and readiness failures are
    /// retried until ordinary NAT is confirmed or an exact replacement core is observed.
    pub fn repair_until_safe(
        &self,
        invocation: WatcherInvocation,
    ) -> Result<FailOpenResult, PlatformError> {
        let mut role_ready = false;
        let mut identity_mismatches = 0;
        while identity_mismatches < IDENTITY_WAIT_ATTEMPTS {
            match self.platform.watcher_role_matches(invocation.core) {
                Ok(true) => {
                    role_ready = true;
                    break;
                }
                Ok(false) => identity_mismatches += 1,
                Err(error) => self
                    .platform
                    .record_fail_open_retry(FailOpenRetryKind::CoreProbe, &error),
            }
            self.platform.sleep_fail_open_retry(IDENTITY_WAIT);
        }
        if !role_ready {
            return Err(PlatformError::Conflict(
                "internal watcher argv/process/record identity did not match".to_owned(),
            ));
        }

        loop {
            let state = match self.platform.core_record_state(invocation.core) {
                Ok(state) => state,
                Err(error) => {
                    self.retry(FailOpenRetryKind::CoreProbe, &error, CORE_POLL);
                    continue;
                }
            };
            match state {
                CoreRecordState::ExpectedLive => {
                    self.platform.sleep_fail_open_retry(CORE_POLL);
                    continue;
                }
                CoreRecordState::Replaced => return Ok(FailOpenResult::ReplacementObserved),
                CoreRecordState::Absent => match self.confirm_already_clean() {
                    Ok(result) => return Ok(result),
                    Err(error) => {
                        self.retry(FailOpenRetryKind::Cleanup, &error, LOCK_RETRY);
                        continue;
                    }
                },
                CoreRecordState::ExpectedExited => {}
            }

            let lease = match self.platform.acquire_fail_open_lock() {
                Ok(lease) => lease,
                Err(error) => {
                    self.retry(FailOpenRetryKind::Lock, &error, LOCK_RETRY);
                    continue;
                }
            };
            let result = self.cleanup_under_lock(invocation.core);
            let release = self.platform.release_fail_open_lock(&lease);
            if let Err(error) = release {
                self.retry(FailOpenRetryKind::LockRelease, &error, LOCK_RETRY);
                continue;
            }
            match result {
                Ok(result) => return Ok(result),
                Err(error) => {
                    self.retry(FailOpenRetryKind::Cleanup, &error, LOCK_RETRY);
                }
            }
        }
    }

    fn retry(&self, kind: FailOpenRetryKind, error: &PlatformError, delay: Duration) {
        self.platform.record_fail_open_retry(kind, error);
        self.platform.sleep_fail_open_retry(delay);
    }

    fn cleanup_under_lock(&self, expected: CoreIdentity) -> Result<FailOpenResult, PlatformError> {
        match self.platform.core_record_state(expected)? {
            CoreRecordState::ExpectedLive => {
                return Err(PlatformError::Busy(
                    "expected core became live again before fail-open cleanup".to_owned(),
                ))
            }
            CoreRecordState::Replaced => return Ok(FailOpenResult::ReplacementObserved),
            CoreRecordState::Absent => return self.confirm_already_clean(),
            CoreRecordState::ExpectedExited => {}
        }
        let observed = self.platform.observe_fail_open_proxy()?;
        if !matches!(observed.persisted_features, Probe::Known(features) if features.lan_tun_enabled)
        {
            return Err(PlatformError::Conflict(
                "watcher fail-open requires persisted TUN mode".to_owned(),
            ));
        }
        for action in fail_open_plan(&observed)? {
            self.platform.apply_fail_open_proxy(&action)?;
        }
        self.platform.remove_fail_open_stale_state(expected)?;
        let final_state = self.platform.observe_fail_open_proxy()?;
        if final_state.interception_entry_present != Probe::Known(false)
            || final_state.policy_rule_present != Probe::Known(false)
            || final_state.policy_route_present != Probe::Known(false)
            || !matches!(
                final_state.persisted_features,
                Probe::Known(features) if features.lan_tun_enabled
            )
            || final_state.ordinary_nat_confirmed != Probe::Known(true)
        {
            return Err(PlatformError::UnsafeToCutOver(
                "fail-open cleanup did not confirm ordinary NAT while preserving TUN mode"
                    .to_owned(),
            ));
        }
        Ok(FailOpenResult::Cleaned)
    }

    fn confirm_already_clean(&self) -> Result<FailOpenResult, PlatformError> {
        let observed = self.platform.observe_fail_open_proxy()?;
        if observed.interception_entry_present == Probe::Known(false)
            && observed.policy_rule_present == Probe::Known(false)
            && observed.policy_route_present == Probe::Known(false)
            && observed.ordinary_nat_confirmed == Probe::Known(true)
        {
            Ok(FailOpenResult::AlreadyClean)
        } else {
            Err(PlatformError::Conflict(
                "core record disappeared without a confirmed ordinary-NAT state".to_owned(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::ports::{FailOpenPlatformPort, LifecycleLease},
        domain::{
            network::{OwnedResource, Probe},
            proxy::{ProxyAction, ProxyObserved},
        },
    };
    use std::{collections::VecDeque, sync::Mutex};

    struct Fake {
        states: Mutex<VecDeque<CoreRecordState>>,
        role_failures: Mutex<usize>,
        state_failures: Mutex<usize>,
        locks: Mutex<VecDeque<bool>>,
        observations: Mutex<VecDeque<ProxyObserved>>,
        actions: Mutex<Vec<ProxyAction>>,
        sleeps: Mutex<usize>,
        removals: Mutex<usize>,
        releases: Mutex<usize>,
    }

    impl Fake {
        fn new(
            states: Vec<CoreRecordState>,
            locks: Vec<bool>,
            observations: Vec<ProxyObserved>,
        ) -> Self {
            Self {
                states: Mutex::new(states.into()),
                role_failures: Mutex::new(0),
                state_failures: Mutex::new(0),
                locks: Mutex::new(locks.into()),
                observations: Mutex::new(observations.into()),
                actions: Mutex::new(Vec::new()),
                sleeps: Mutex::new(0),
                removals: Mutex::new(0),
                releases: Mutex::new(0),
            }
        }
    }

    impl FailOpenPlatformPort for Fake {
        fn watcher_role_matches(&self, _: CoreIdentity) -> Result<bool, PlatformError> {
            let mut failures = self.role_failures.lock().unwrap();
            if *failures > 0 {
                *failures -= 1;
                Err(PlatformError::ProbeFailed(
                    "transient watcher identity probe".to_owned(),
                ))
            } else {
                Ok(true)
            }
        }
        fn core_record_state(&self, _: CoreIdentity) -> Result<CoreRecordState, PlatformError> {
            let mut failures = self.state_failures.lock().unwrap();
            if *failures > 0 {
                *failures -= 1;
                return Err(PlatformError::ProbeFailed(
                    "transient state probe".to_owned(),
                ));
            }
            self.states
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| PlatformError::ProbeFailed("no state".to_owned()))
        }
        fn acquire_fail_open_lock(&self) -> Result<LifecycleLease, PlatformError> {
            if self.locks.lock().unwrap().pop_front().unwrap_or(false) {
                Err(PlatformError::Busy("held".to_owned()))
            } else {
                Ok(LifecycleLease {
                    path: "/run/fake.lock",
                    identity: "fake".to_owned(),
                    directory_device: 0,
                    directory_inode: 0,
                })
            }
        }
        fn release_fail_open_lock(&self, _: &LifecycleLease) -> Result<(), PlatformError> {
            *self.releases.lock().unwrap() += 1;
            Ok(())
        }
        fn observe_fail_open_proxy(&self) -> Result<ProxyObserved, PlatformError> {
            self.observations
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| PlatformError::ProbeFailed("no observation".to_owned()))
        }
        fn apply_fail_open_proxy(&self, action: &ProxyAction) -> Result<(), PlatformError> {
            self.actions.lock().unwrap().push(action.clone());
            Ok(())
        }
        fn remove_fail_open_stale_state(&self, _: CoreIdentity) -> Result<(), PlatformError> {
            *self.removals.lock().unwrap() += 1;
            Ok(())
        }
        fn sleep_fail_open_retry(&self, _: Duration) {
            *self.sleeps.lock().unwrap() += 1;
        }
    }

    fn observed(owned: bool) -> ProxyObserved {
        ProxyObserved {
            persisted_features: Probe::Known(crate::domain::proxy::ProxyFeaturesV1::new(
                true, false,
            )),
            process_identity_valid: Probe::Known(false),
            watcher_identity_valid: Probe::Known(owned),
            runtime_config_valid: Probe::Known(owned),
            mixed_port_ready: Probe::Known(owned),
            tun_interface: Probe::Known(if owned {
                OwnedResource::Owned {
                    token: "old".to_owned(),
                }
            } else {
                OwnedResource::Absent
            }),
            tun_firewall: Probe::Known(if owned {
                OwnedResource::Owned {
                    token: "hyz-mihomo-owned".to_owned(),
                }
            } else {
                OwnedResource::Absent
            }),
            policy_rule_present: Probe::Known(owned),
            policy_route_present: Probe::Known(owned),
            interception_entry_present: Probe::Known(owned),
            ordinary_nat_confirmed: Probe::Known(!owned),
            active_direct_macs: Probe::Known(Default::default()),
        }
    }

    fn invocation() -> WatcherInvocation {
        WatcherInvocation {
            core: CoreIdentity {
                pid: 40,
                start_time: 90,
            },
        }
    }

    #[test]
    fn hidden_role_parser_requires_exact_role_pid_and_start_argv() {
        let args = vec![
            INTERNAL_WATCHER_ROLE.to_owned(),
            "40".to_owned(),
            "90".to_owned(),
        ];
        assert_eq!(
            WatcherInvocation::parse_hidden(&args),
            Some(Ok(invocation()))
        );
        assert!(WatcherInvocation::parse_hidden(&["status".to_owned()]).is_none());
        assert!(WatcherInvocation::parse_hidden(&[
            INTERNAL_WATCHER_ROLE.to_owned(),
            "40".to_owned(),
        ])
        .is_none());
    }

    #[test]
    fn lock_busy_retries_then_cleans_interception_first() {
        let fake = Fake::new(
            vec![
                CoreRecordState::ExpectedExited,
                CoreRecordState::ExpectedExited,
                CoreRecordState::ExpectedExited,
            ],
            vec![true, false],
            vec![observed(true), observed(false)],
        );
        assert_eq!(
            MihomoFailOpenApplication::new(&fake).execute(invocation()),
            Ok(FailOpenResult::Cleaned)
        );
        assert!(matches!(
            fake.actions.lock().unwrap().first(),
            Some(ProxyAction::RemoveInterceptionEntry { .. })
        ));
        assert_eq!(*fake.sleeps.lock().unwrap(), 1);
        assert_eq!(*fake.removals.lock().unwrap(), 1);
        assert_eq!(*fake.releases.lock().unwrap(), 1);
    }

    #[test]
    fn transient_identity_and_core_probe_failures_retry_instead_of_exiting() {
        let fake = Fake::new(vec![CoreRecordState::Replaced], vec![], vec![]);
        *fake.role_failures.lock().unwrap() = 1;
        *fake.state_failures.lock().unwrap() = 1;
        assert_eq!(
            MihomoFailOpenApplication::new(&fake).repair_until_safe(invocation()),
            Ok(FailOpenResult::ReplacementObserved)
        );
        assert_eq!(*fake.sleeps.lock().unwrap(), 2);
    }

    #[test]
    fn final_readiness_failure_retries_until_ordinary_nat_is_confirmed() {
        let fake = Fake::new(
            vec![
                CoreRecordState::ExpectedExited,
                CoreRecordState::ExpectedExited,
                CoreRecordState::ExpectedExited,
                CoreRecordState::ExpectedExited,
            ],
            vec![false, false],
            vec![
                observed(true),
                observed(true),
                observed(false),
                observed(false),
            ],
        );
        assert_eq!(
            MihomoFailOpenApplication::new(&fake).repair_until_safe(invocation()),
            Ok(FailOpenResult::Cleaned)
        );
        assert_eq!(*fake.removals.lock().unwrap(), 2);
        assert_eq!(*fake.releases.lock().unwrap(), 2);
        assert_eq!(*fake.sleeps.lock().unwrap(), 1);
    }

    #[test]
    fn replacement_core_after_lock_causes_no_cleanup() {
        let fake = Fake::new(
            vec![CoreRecordState::ExpectedExited, CoreRecordState::Replaced],
            vec![false],
            vec![],
        );
        assert_eq!(
            MihomoFailOpenApplication::new(&fake).execute(invocation()),
            Ok(FailOpenResult::ReplacementObserved)
        );
        assert!(fake.actions.lock().unwrap().is_empty());
        assert_eq!(*fake.removals.lock().unwrap(), 0);
        assert_eq!(*fake.releases.lock().unwrap(), 1);
    }
}
