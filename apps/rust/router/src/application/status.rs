use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::{
    application::ports::ClockPort,
    domain::status::{
        Component, ProxyStatus, RouterStatus, SnapshotState, StatusSnapshot, SystemStats,
    },
};

// Status needs richer read models than the lifecycle-oriented core ports expose. These two
// status-specific coarse traits are intentional extensions: the main platform adapter can
// implement them alongside application::ports::{RouterPlatformPort, SystemProbePort}. ClockPort
// is reused directly from application::ports.
#[async_trait]
pub trait StatusRouterPlatformPort: Send + Sync {
    async fn read_router_status(&self) -> Component<RouterStatus>;
    async fn read_proxy_status(&self) -> Component<ProxyStatus>;
}

#[async_trait]
pub trait StatusSystemProbePort: Send + Sync {
    async fn read_system_stats(&self) -> Component<SystemStats>;
}

const STATUS_CACHE_TTL: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct CachedSnapshot {
    collected_at: Instant,
    snapshot: StatusSnapshot,
}

#[derive(Clone)]
pub struct ReadStatus {
    router_platform: Arc<dyn StatusRouterPlatformPort>,
    system_probe: Arc<dyn StatusSystemProbePort>,
    clock: Arc<dyn ClockPort>,
    cache: Arc<Mutex<Option<CachedSnapshot>>>,
    cache_ttl: Duration,
}

impl ReadStatus {
    pub fn new(
        router_platform: Arc<dyn StatusRouterPlatformPort>,
        system_probe: Arc<dyn StatusSystemProbePort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self::with_cache_ttl(router_platform, system_probe, clock, STATUS_CACHE_TTL)
    }

    #[cfg(feature = "e2e")]
    pub fn new_uncached(
        router_platform: Arc<dyn StatusRouterPlatformPort>,
        system_probe: Arc<dyn StatusSystemProbePort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self::with_cache_ttl(router_platform, system_probe, clock, Duration::ZERO)
    }

    fn with_cache_ttl(
        router_platform: Arc<dyn StatusRouterPlatformPort>,
        system_probe: Arc<dyn StatusSystemProbePort>,
        clock: Arc<dyn ClockPort>,
        cache_ttl: Duration,
    ) -> Self {
        Self {
            router_platform,
            system_probe,
            clock,
            cache: Arc::new(Mutex::new(None)),
            cache_ttl,
        }
    }

    pub async fn execute(&self) -> StatusSnapshot {
        // Keep the lock while collecting so concurrent unauthenticated LAN requests
        // coalesce into one bounded probe instead of spawning parallel root commands.
        let mut cache = self.cache.lock().await;
        if let Some(cached) = cache.as_ref() {
            if cached.collected_at.elapsed() < self.cache_ttl {
                return cached.snapshot.clone();
            }
        }

        let (router, proxy, system) = tokio::join!(
            self.router_platform.read_router_status(),
            self.router_platform.read_proxy_status(),
            self.system_probe.read_system_stats(),
        );
        let state = if router.is_available() && proxy.is_available() && system.is_available() {
            SnapshotState::Ok
        } else {
            SnapshotState::Degraded
        };

        let snapshot = StatusSnapshot {
            state,
            observed_at_unix_ms: self.clock.unix_time_millis(),
            router,
            proxy,
            system,
        };
        *cache = Some(CachedSnapshot {
            collected_at: Instant::now(),
            snapshot: snapshot.clone(),
        });
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::status::{Issue, RouterStatus, SystemStats};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Fake {
        router_reads: AtomicUsize,
    }

    #[async_trait]
    impl StatusRouterPlatformPort for Fake {
        async fn read_router_status(&self) -> Component<RouterStatus> {
            self.router_reads.fetch_add(1, Ordering::SeqCst);
            Component::available(RouterStatus::default())
        }

        async fn read_proxy_status(&self) -> Component<ProxyStatus> {
            Component::unavailable(Issue::new(
                "proxy_unavailable",
                "Proxy status is unavailable",
            ))
        }
    }

    #[async_trait]
    impl StatusSystemProbePort for Fake {
        async fn read_system_stats(&self) -> Component<SystemStats> {
            Component::available(SystemStats::default())
        }
    }

    impl ClockPort for Fake {
        fn unix_time_millis(&self) -> u64 {
            42
        }
    }

    #[tokio::test]
    async fn preserves_partial_results_and_marks_snapshot_degraded() {
        let fake = Arc::new(Fake::default());
        let snapshot = ReadStatus::new(fake.clone(), fake.clone(), fake)
            .execute()
            .await;

        assert_eq!(snapshot.state, SnapshotState::Degraded);
        assert!(snapshot.router.data.is_some());
        assert!(snapshot.proxy.data.is_none());
        assert_eq!(snapshot.observed_at_unix_ms, 42);
    }

    #[cfg(feature = "e2e")]
    #[tokio::test]
    async fn e2e_uncached_reader_observes_each_harness_state_change() {
        let fake = Arc::new(Fake::default());
        let status = ReadStatus::new_uncached(fake.clone(), fake.clone(), fake.clone());

        status.execute().await;
        status.execute().await;

        assert_eq!(fake.router_reads.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn coalesces_concurrent_and_repeated_status_reads_within_ttl() {
        let fake = Arc::new(Fake::default());
        let status = ReadStatus::new(fake.clone(), fake.clone(), fake.clone());

        let (first, second, third) =
            tokio::join!(status.execute(), status.execute(), status.execute());
        let repeated = status.execute().await;

        assert_eq!(fake.router_reads.load(Ordering::SeqCst), 1);
        assert_eq!(first.observed_at_unix_ms, second.observed_at_unix_ms);
        assert_eq!(second.observed_at_unix_ms, third.observed_at_unix_ms);
        assert_eq!(third.observed_at_unix_ms, repeated.observed_at_unix_ms);
    }
}
