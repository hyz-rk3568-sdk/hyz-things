use crate::application::proxy::ProxyApplication;
use crate::{
    application::ports::{
        ClockPort, DevicePolicyStorePort, LanClientDiscoveryPort, LifecycleLease, PlatformError,
        RouterPlatformPort, SystemProbePort,
    },
    domain::{
        device_policy::{DevicePolicyConfigV1, DevicePolicySnapshot, DevicePolicyUpdateRequest},
        network::Probe,
        proxy::{ProxyDesired, ProxyFeaturesV1},
    },
};
use std::time::{Duration, Instant};

pub struct DevicePolicyApplication<'a> {
    store: &'a dyn DevicePolicyStorePort,
    discovery: &'a dyn LanClientDiscoveryPort,
    platform: &'a dyn RouterPlatformPort,
    probe: &'a dyn SystemProbePort,
    clock: &'a dyn ClockPort,
}

impl<'a> DevicePolicyApplication<'a> {
    pub fn new(
        store: &'a dyn DevicePolicyStorePort,
        discovery: &'a dyn LanClientDiscoveryPort,
        platform: &'a dyn RouterPlatformPort,
        probe: &'a dyn SystemProbePort,
        clock: &'a dyn ClockPort,
    ) -> Self {
        Self {
            store,
            discovery,
            platform,
            probe,
            clock,
        }
    }

    pub fn snapshot(&self) -> Result<DevicePolicySnapshot, PlatformError> {
        let config = self.store.load_device_policy()?;
        let clients = self.discovery.discover_lan_clients(&config)?;
        let effective = self.effective_for(&config)?;
        Ok(DevicePolicySnapshot {
            config,
            clients,
            effective,
        })
    }

    pub fn update(
        &self,
        request: DevicePolicyUpdateRequest,
    ) -> Result<DevicePolicySnapshot, PlatformError> {
        // The lock precedes every read, generation decision, and journal write so two callers
        // cannot both validate against the same committed generation.
        let lease = self.platform.acquire_lifecycle_lock()?;
        let result = (|| {
            let previous = self.store.load_device_policy()?;
            if request.expected_generation != previous.generation {
                return Err(PlatformError::Conflict(
                    "device policy generation is stale; reload before saving".to_owned(),
                ));
            }
            let candidate = request
                .candidate()
                .map_err(|error| PlatformError::InvalidState(error.to_owned()))?;
            // Every fallible response-only read happens before staging. Once the journaled
            // transaction commits, the response is assembled from the reconciled candidate so a
            // transient discovery or probe failure cannot misreport a successful mutation.
            let clients = self.discovery.discover_lan_clients(&candidate)?;
            let features = self.persisted_features()?;
            run_update_transaction(self.store, &previous, &candidate, |config| {
                self.reconcile_locked(features, config)
            })?;
            Ok(DevicePolicySnapshot {
                config: candidate,
                clients,
                effective: features.lan_tun_enabled,
            })
        })();
        let release = self.platform.release_lifecycle_lock(&lease);
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(snapshot), Ok(())) => Ok(snapshot),
        }
    }

    /// Startup recovery always restores the journal's previous state and never promotes candidate.
    pub fn recover(&self) -> Result<(), PlatformError> {
        // Startup must not fail because a transient in-process worker (the DHCP dispatcher
        // applying a lease captured while management startup held the lock) briefly owns the
        // lifecycle lock. Acquire it with the same bounded retry the DHCP worker uses, then still
        // fail closed if the contention never clears.
        let lease = acquire_lifecycle_lock_bounded(self.platform)?;
        let result = (|| {
            let Some((previous, _candidate)) = self.store.load_pending_device_policy()? else {
                return Ok(());
            };
            restore_previous(self.store, &previous, |config| {
                let features = self.persisted_features()?;
                self.reconcile_locked(features, config)
            })
        })();
        let release = self.platform.release_lifecycle_lock(&lease);
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn effective_for(&self, config: &DevicePolicyConfigV1) -> Result<bool, PlatformError> {
        let observed = self.probe.observe_proxy()?;
        let features = self.persisted_features()?;
        if !features.lan_tun_enabled {
            return Ok(false);
        }
        if let Probe::Unknown(reason) = &observed.active_direct_macs {
            return Err(PlatformError::ProbeFailed(format!(
                "active device policy is unknown: {reason}"
            )));
        }
        Ok(observed.ready_for(&ProxyDesired {
            lan_tun_enabled: true,
            local_system_proxy_enabled: features.local_system_proxy_enabled,
            direct_macs: config.direct_macs(),
        }))
    }

    fn persisted_features(&self) -> Result<ProxyFeaturesV1, PlatformError> {
        match self.probe.observe_proxy()?.persisted_features {
            Probe::Known(features) if features.supported() => Ok(features),
            Probe::Known(_) => Err(PlatformError::ProbeFailed(
                "persisted proxy feature version is unsupported".to_owned(),
            )),
            Probe::Unknown(reason) => Err(PlatformError::ProbeFailed(format!(
                "persisted proxy features are unknown: {reason}"
            ))),
        }
    }

    fn reconcile_locked(
        &self,
        features: ProxyFeaturesV1,
        config: &DevicePolicyConfigV1,
    ) -> Result<(), PlatformError> {
        ProxyApplication::new(self.platform, self.probe, self.clock)
            .reconcile_locked(&ProxyDesired {
                lan_tun_enabled: features.lan_tun_enabled,
                local_system_proxy_enabled: features.local_system_proxy_enabled,
                direct_macs: config.direct_macs(),
            })
            .map(|_| ())
    }
}

/// Upper bound for waiting on a transiently busy lifecycle lock during startup device-policy
/// recovery. Mirrors the DHCP worker's bounded acquisition so startup does not hard-fail on
/// in-process contention that clears within the same budget.
const LIFECYCLE_LOCK_WAIT: Duration = Duration::from_secs(3 * 60);
const LIFECYCLE_LOCK_RETRY: Duration = Duration::from_millis(20);

fn acquire_lifecycle_lock_bounded(
    platform: &dyn RouterPlatformPort,
) -> Result<LifecycleLease, PlatformError> {
    acquire_lifecycle_lock_bounded_until(platform, Instant::now() + LIFECYCLE_LOCK_WAIT, || {
        std::thread::sleep(LIFECYCLE_LOCK_RETRY)
    })
}

fn acquire_lifecycle_lock_bounded_until(
    platform: &dyn RouterPlatformPort,
    deadline: Instant,
    mut wait: impl FnMut(),
) -> Result<LifecycleLease, PlatformError> {
    loop {
        match platform.acquire_lifecycle_lock() {
            Ok(lease) => return Ok(lease),
            Err(PlatformError::Busy(_)) if Instant::now() < deadline => wait(),
            Err(error) => return Err(error),
        }
    }
}

fn run_update_transaction(
    store: &dyn DevicePolicyStorePort,
    previous: &DevicePolicyConfigV1,
    candidate: &DevicePolicyConfigV1,
    mut apply: impl FnMut(&DevicePolicyConfigV1) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    store.stage_device_policy(previous, candidate)?;
    let cutover = apply(candidate)
        .and_then(|()| store.commit_device_policy(candidate))
        .and_then(|()| store.clear_pending_device_policy());
    if let Err(primary) = cutover {
        return match restore_previous(store, previous, apply) {
            Ok(()) => Err(primary),
            Err(rollback) => Err(PlatformError::UnsafeToCutOver(format!(
                "device policy transaction failed: {primary}; rollback also failed: {rollback}"
            ))),
        };
    }
    Ok(())
}

fn restore_previous(
    store: &dyn DevicePolicyStorePort,
    previous: &DevicePolicyConfigV1,
    mut apply: impl FnMut(&DevicePolicyConfigV1) -> Result<(), PlatformError>,
) -> Result<(), PlatformError> {
    apply(previous)?;
    store.commit_device_policy(previous)?;
    store.clear_pending_device_policy()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::ports::LifecycleLease,
        domain::{
            device_policy::LanClientObservation,
            network::{NetworkAction, NetworkObserved},
            proxy::{ProxyAction, ProxyObserved},
        },
    };
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct FakeStore {
        committed: Arc<Mutex<DevicePolicyConfigV1>>,
        pending: Arc<Mutex<Option<(DevicePolicyConfigV1, DevicePolicyConfigV1)>>>,
        events: Arc<Mutex<Vec<String>>>,
        fail_commit_generation: Option<u64>,
        fail_clear: bool,
    }

    impl FakeStore {
        fn new(config: DevicePolicyConfigV1, events: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                committed: Arc::new(Mutex::new(config)),
                pending: Arc::new(Mutex::new(None)),
                events,
                fail_commit_generation: None,
                fail_clear: false,
            }
        }

        fn record(&self, event: impl Into<String>) {
            self.events.lock().unwrap().push(event.into());
        }
    }

    impl DevicePolicyStorePort for FakeStore {
        fn load_device_policy(&self) -> Result<DevicePolicyConfigV1, PlatformError> {
            self.record("load");
            Ok(self.committed.lock().unwrap().clone())
        }

        fn load_pending_device_policy(
            &self,
        ) -> Result<Option<(DevicePolicyConfigV1, DevicePolicyConfigV1)>, PlatformError> {
            self.record("load_pending");
            Ok(self.pending.lock().unwrap().clone())
        }

        fn stage_device_policy(
            &self,
            previous: &DevicePolicyConfigV1,
            candidate: &DevicePolicyConfigV1,
        ) -> Result<(), PlatformError> {
            self.record("stage");
            *self.pending.lock().unwrap() = Some((previous.clone(), candidate.clone()));
            Ok(())
        }

        fn commit_device_policy(
            &self,
            candidate: &DevicePolicyConfigV1,
        ) -> Result<(), PlatformError> {
            self.record(format!("commit:{}", candidate.generation));
            if self.fail_commit_generation == Some(candidate.generation) {
                return Err(PlatformError::Io("injected commit failure".to_owned()));
            }
            *self.committed.lock().unwrap() = candidate.clone();
            Ok(())
        }

        fn clear_pending_device_policy(&self) -> Result<(), PlatformError> {
            self.record("clear");
            if self.fail_clear {
                return Err(PlatformError::Io("injected clear failure".to_owned()));
            }
            *self.pending.lock().unwrap() = None;
            Ok(())
        }
    }

    #[test]
    fn update_orders_stage_candidate_commit_and_clear() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let previous = DevicePolicyConfigV1::empty();
        let candidate = DevicePolicyConfigV1::new(1, Vec::new()).unwrap();
        let store = FakeStore::new(previous.clone(), events.clone());
        run_update_transaction(&store, &previous, &candidate, |config| {
            events
                .lock()
                .unwrap()
                .push(format!("apply:{}", config.generation));
            Ok(())
        })
        .unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            ["stage", "apply:1", "commit:1", "clear"]
        );
    }

    #[test]
    fn candidate_failure_rolls_back_once_and_keeps_both_errors() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let previous = DevicePolicyConfigV1::empty();
        let candidate = DevicePolicyConfigV1::new(1, Vec::new()).unwrap();
        let store = FakeStore::new(previous.clone(), events.clone());
        let error = run_update_transaction(&store, &previous, &candidate, |config| {
            events
                .lock()
                .unwrap()
                .push(format!("apply:{}", config.generation));
            if config.generation == 1 {
                Err(PlatformError::CommandFailed("candidate failed".to_owned()))
            } else {
                Err(PlatformError::CommandFailed("previous failed".to_owned()))
            }
        })
        .unwrap_err();
        assert!(error.to_string().contains("candidate failed"));
        assert!(error.to_string().contains("previous failed"));
        assert_eq!(*events.lock().unwrap(), ["stage", "apply:1", "apply:0"]);
    }

    #[test]
    fn commit_failure_restores_previous_data_plane_and_canonical_state() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let previous = DevicePolicyConfigV1::empty();
        let candidate = DevicePolicyConfigV1::new(1, Vec::new()).unwrap();
        let mut store = FakeStore::new(previous.clone(), events.clone());
        store.fail_commit_generation = Some(1);
        let error = run_update_transaction(&store, &previous, &candidate, |config| {
            events
                .lock()
                .unwrap()
                .push(format!("apply:{}", config.generation));
            Ok(())
        })
        .unwrap_err();
        assert!(error.to_string().contains("injected commit failure"));
        assert_eq!(
            *events.lock().unwrap(),
            ["stage", "apply:1", "commit:1", "apply:0", "commit:0", "clear"]
        );
        assert_eq!(store.committed.lock().unwrap().generation, 0);
    }

    #[test]
    fn recovery_restores_previous_and_never_candidate() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let previous = DevicePolicyConfigV1::empty();
        let candidate = DevicePolicyConfigV1::new(1, Vec::new()).unwrap();
        let store = FakeStore::new(candidate, events.clone());
        *store.pending.lock().unwrap() = Some((
            previous.clone(),
            DevicePolicyConfigV1::new(1, Vec::new()).unwrap(),
        ));
        restore_previous(&store, &previous, |config| {
            events
                .lock()
                .unwrap()
                .push(format!("apply:{}", config.generation));
            Ok(())
        })
        .unwrap();
        assert_eq!(*events.lock().unwrap(), ["apply:0", "commit:0", "clear"]);
        assert_eq!(store.committed.lock().unwrap().generation, 0);
    }

    struct LockPlatform {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl RouterPlatformPort for LockPlatform {
        fn acquire_lifecycle_lock(&self) -> Result<LifecycleLease, PlatformError> {
            self.events.lock().unwrap().push("lock".to_owned());
            Ok(LifecycleLease {
                path: "test",
                identity: "test".to_owned(),
                directory_device: 0,
                directory_inode: 0,
            })
        }
        fn release_lifecycle_lock(&self, _: &LifecycleLease) -> Result<(), PlatformError> {
            self.events.lock().unwrap().push("release".to_owned());
            Ok(())
        }
        fn apply_network(&self, _: &NetworkAction) -> Result<(), PlatformError> {
            unreachable!()
        }
        fn apply_proxy(&self, _: &ProxyAction) -> Result<(), PlatformError> {
            unreachable!()
        }
    }

    struct UnusedProbe;
    impl SystemProbePort for UnusedProbe {
        fn observe_network(&self) -> Result<NetworkObserved, PlatformError> {
            unreachable!()
        }
        fn observe_proxy(&self) -> Result<ProxyObserved, PlatformError> {
            unreachable!()
        }
    }
    impl ClockPort for UnusedProbe {
        fn unix_time_millis(&self) -> u64 {
            0
        }
    }
    impl LanClientDiscoveryPort for UnusedProbe {
        fn discover_lan_clients(
            &self,
            _: &DevicePolicyConfigV1,
        ) -> Result<Vec<LanClientObservation>, PlatformError> {
            unreachable!()
        }
    }

    #[test]
    fn bounded_snapshot_fits_control_frame() {
        let clients = (0..128)
            .map(|index| LanClientObservation {
                mac: format!("02:00:00:00:{:02x}:{:02x}", index / 256, index % 256)
                    .parse()
                    .unwrap(),
                lease_address: Some(format!("192.168.8.{}", index % 254 + 1).parse().unwrap()),
                hostname: Some("h".repeat(crate::domain::device_policy::MAX_LAN_HOSTNAME_BYTES)),
                associated: true,
                policy: crate::domain::device_policy::DeviceRoutePolicy::Proxy,
            })
            .collect();
        let config = DevicePolicyConfigV1::new(
            u64::MAX,
            (0..crate::domain::device_policy::MAX_DEVICE_POLICIES)
                .map(|index| crate::domain::device_policy::DevicePolicyEntry {
                    mac: format!("02:00:00:00:00:{index:02x}").parse().unwrap(),
                    label: crate::domain::device_policy::LanDeviceLabel::new(
                        "\"".repeat(crate::domain::device_policy::MAX_DEVICE_LABEL_BYTES),
                    )
                    .unwrap(),
                    policy: crate::domain::device_policy::DeviceRoutePolicy::Direct,
                })
                .collect(),
        )
        .unwrap();
        let snapshot = DevicePolicySnapshot {
            config,
            clients,
            effective: false,
        };
        assert!(serde_json::to_vec(&snapshot).unwrap().len() < 64 * 1024);
    }

    #[test]
    fn successful_update_has_no_fallible_response_probe_after_commit() {
        let production = include_str!("device_policy.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        let update = production
            .split("pub fn update(")
            .nth(1)
            .unwrap()
            .split("/// Startup recovery")
            .next()
            .unwrap();
        let discovery = update.find("discover_lan_clients(&candidate)").unwrap();
        let transaction = update.find("run_update_transaction").unwrap();
        let response = update.find("Ok(DevicePolicySnapshot").unwrap();
        assert!(discovery < transaction && transaction < response);
        assert!(!update.contains("self.snapshot()"));
    }

    #[test]
    fn stale_generation_still_locks_before_load_and_has_no_platform_side_effects() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let store = FakeStore::new(
            DevicePolicyConfigV1::new(2, Vec::new()).unwrap(),
            events.clone(),
        );
        let platform = LockPlatform {
            events: events.clone(),
        };
        let unused = UnusedProbe;
        let app = DevicePolicyApplication::new(&store, &unused, &platform, &unused, &unused);
        let error = app
            .update(DevicePolicyUpdateRequest {
                expected_generation: 1,
                entries: Vec::new(),
            })
            .unwrap_err();
        assert!(matches!(error, PlatformError::Conflict(_)));
        assert_eq!(*events.lock().unwrap(), ["lock", "load", "release"]);
    }

    struct BusyThenOkLockPlatform {
        remaining_busy: Arc<Mutex<usize>>,
    }

    impl RouterPlatformPort for BusyThenOkLockPlatform {
        fn acquire_lifecycle_lock(&self) -> Result<LifecycleLease, PlatformError> {
            let mut remaining = self.remaining_busy.lock().unwrap();
            if *remaining > 0 {
                *remaining -= 1;
                return Err(PlatformError::Busy("test transient busy".to_owned()));
            }
            Ok(LifecycleLease {
                path: "test",
                identity: "test".to_owned(),
                directory_device: 0,
                directory_inode: 0,
            })
        }
        fn release_lifecycle_lock(&self, _: &LifecycleLease) -> Result<(), PlatformError> {
            Ok(())
        }
        fn apply_network(&self, _: &NetworkAction) -> Result<(), PlatformError> {
            unreachable!()
        }
        fn apply_proxy(&self, _: &ProxyAction) -> Result<(), PlatformError> {
            unreachable!()
        }
    }

    #[test]
    fn bounded_lock_acquisition_retries_a_transient_busy_holder() {
        let platform = BusyThenOkLockPlatform {
            remaining_busy: Arc::new(Mutex::new(2)),
        };
        let attempts = Arc::new(Mutex::new(0usize));
        let lease = acquire_lifecycle_lock_bounded_until(
            &platform,
            Instant::now() + Duration::from_secs(60),
            || {
                *attempts.lock().unwrap() += 1;
            },
        )
        .expect("a transient Busy must be retried and then acquired");
        assert_eq!(lease.path, "test");
        assert_eq!(*attempts.lock().unwrap(), 2);
    }

    #[test]
    fn bounded_lock_acquisition_fails_closed_when_the_holder_never_clears() {
        let platform = BusyThenOkLockPlatform {
            remaining_busy: Arc::new(Mutex::new(usize::MAX)),
        };
        let error = acquire_lifecycle_lock_bounded_until(&platform, Instant::now(), || {})
            .expect_err("a lock that stays Busy must fail closed at the deadline");
        assert!(matches!(error, PlatformError::Busy(_)));
    }

    #[test]
    fn recovery_retries_a_transient_busy_lifecycle_lock_instead_of_failing_startup() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let store = FakeStore::new(DevicePolicyConfigV1::empty(), events.clone());
        let platform = BusyThenOkLockPlatform {
            remaining_busy: Arc::new(Mutex::new(2)),
        };
        let unused = UnusedProbe;
        let app = DevicePolicyApplication::new(&store, &unused, &platform, &unused, &unused);
        // The DHCP dispatcher can briefly hold the lifecycle lock while applying a lease that
        // management startup captured; recovery must wait it out instead of aborting the boot.
        app.recover()
            .expect("transient lock contention must not fail startup recovery");
    }
}
