use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::{
    application::ports::ClockPort,
    domain::{
        network::{OwnedResource, Probe},
        proxy::ProxyFeaturesV1,
        status::{
            Component, Issue, ProxyStatus, RouterStatus, SnapshotState, StatusSnapshot,
            SystemStats, TailscaleConnectionStatus, TailscaleConnectionType,
            TailscaleErrorCategory, TailscaleExplicitProxyPath, TailscaleProxyFallback,
            TailscaleRouteApproval, TailscaleStatus,
        },
        tailscale::{
            TailscaleBackendState, TailscaleConnectionKind, TailscaleDesired, TailscaleEnvironment,
            TailscaleMode, TailscaleObserved, TailscaleReadiness,
        },
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

#[async_trait]
pub trait StatusTailscalePlatformPort: Send + Sync {
    async fn read_tailscale_status(&self) -> Component<TailscaleStatus>;
}

struct UnavailableTailscaleStatus;

#[async_trait]
impl StatusTailscalePlatformPort for UnavailableTailscaleStatus {
    async fn read_tailscale_status(&self) -> Component<TailscaleStatus> {
        Component::unavailable(Issue::new(
            "tailscale_status_unavailable",
            "Tailscale status is not connected to the runtime",
        ))
    }
}

pub fn tailscale_status_from_observed(
    observed: &TailscaleObserved,
    proxy_features: &Probe<ProxyFeaturesV1>,
    ordinary_router_ready: bool,
    explicit_proxy_path_ready: &Probe<bool>,
) -> TailscaleStatus {
    let desired_mode = match &observed.persisted_mode {
        Probe::Known(mode) => *mode,
        Probe::Unknown(_) => None,
    };
    let effective_mode = desired_mode.and_then(|mode| {
        match observed.readiness(&TailscaleDesired { mode }, ordinary_router_ready) {
            TailscaleReadiness::Ready { effective_mode } => Some(effective_mode),
            TailscaleReadiness::NeedsLogin | TailscaleReadiness::NotReady => None,
        }
    });
    let backend_state = match &observed.backend_state {
        Probe::Known(state) => *state,
        Probe::Unknown(_) => TailscaleBackendState::Unknown,
    };
    let authenticated = known_copy(&observed.authenticated);
    let ipv4 = match &observed.ipv4 {
        Probe::Known(address) => *address,
        Probe::Unknown(_) => None,
    };
    let route_advertised = known_copy(&observed.route_advertised);
    let local_firewall_ready = match (
        &observed.router_firewall,
        &observed.subnet_firewall,
        effective_mode,
    ) {
        (Probe::Known(OwnedResource::Owned { .. }), _, Some(mode))
            if mode != TailscaleMode::LanSubnetAccess =>
        {
            Some(true)
        }
        (
            Probe::Known(OwnedResource::Owned { .. }),
            Probe::Known(OwnedResource::Owned { .. }),
            Some(TailscaleMode::LanSubnetAccess),
        ) => Some(true),
        (Probe::Unknown(_), _, _) | (_, Probe::Unknown(_), _) => None,
        _ => Some(false),
    };
    let connection = match &observed.connection {
        Probe::Known(TailscaleConnectionKind::Direct) => TailscaleConnectionStatus {
            kind: TailscaleConnectionType::Direct,
            derp_region: None,
        },
        Probe::Known(TailscaleConnectionKind::PeerRelay) => TailscaleConnectionStatus {
            kind: TailscaleConnectionType::PeerRelay,
            derp_region: None,
        },
        Probe::Known(TailscaleConnectionKind::Derp(region)) => TailscaleConnectionStatus {
            kind: TailscaleConnectionType::Derp,
            derp_region: Some(region.clone()),
        },
        Probe::Known(TailscaleConnectionKind::Unknown) | Probe::Unknown(_) => {
            TailscaleConnectionStatus {
                kind: TailscaleConnectionType::Unknown,
                derp_region: None,
            }
        }
    };
    let explicit_proxy_desired = match proxy_features {
        Probe::Known(features) if features.supported() => {
            Some(features.tailscale_explicit_proxy_enabled)
        }
        Probe::Known(_) | Probe::Unknown(_) => None,
    };
    let environment = match &observed.environment {
        Probe::Known(environment) => Some(*environment),
        Probe::Unknown(_) => None,
    };
    let explicit_proxy_path = match (explicit_proxy_desired, environment) {
        (Some(true), Some(TailscaleEnvironment::MihomoExplicit)) => match explicit_proxy_path_ready
        {
            Probe::Known(true) => TailscaleExplicitProxyPath::Ready,
            Probe::Known(false) => TailscaleExplicitProxyPath::Unavailable,
            Probe::Unknown(_) => TailscaleExplicitProxyPath::Unknown,
        },
        (Some(false), Some(_)) | (Some(true), Some(TailscaleEnvironment::Direct)) => {
            TailscaleExplicitProxyPath::NotRequired
        }
        _ => TailscaleExplicitProxyPath::Unknown,
    };
    let proxy_fallback = match (explicit_proxy_desired, environment, explicit_proxy_path) {
        (Some(false), Some(TailscaleEnvironment::Direct), _)
        | (
            Some(true),
            Some(TailscaleEnvironment::MihomoExplicit),
            TailscaleExplicitProxyPath::Ready,
        ) => TailscaleProxyFallback::NotNeeded,
        (Some(true), Some(TailscaleEnvironment::Direct), _) => {
            TailscaleProxyFallback::DirectRestored
        }
        _ => TailscaleProxyFallback::NotConfirmed,
    };
    let has_unknown = matches!(&observed.persisted_mode, Probe::Unknown(_))
        || matches!(proxy_features, Probe::Unknown(_))
        || matches!(proxy_features, Probe::Known(features) if !features.supported())
        || matches!(&observed.process, Probe::Unknown(_))
        || matches!(&observed.environment, Probe::Unknown(_))
        || matches!(&observed.socket, Probe::Unknown(_))
        || matches!(&observed.interface, Probe::Unknown(_))
        || matches!(&observed.backend_state, Probe::Unknown(_))
        || matches!(&observed.authenticated, Probe::Unknown(_))
        || matches!(&observed.ipv4, Probe::Unknown(_))
        || matches!(&observed.route_advertised, Probe::Unknown(_))
        || matches!(&observed.router_firewall, Probe::Unknown(_))
        || matches!(&observed.subnet_firewall, Probe::Unknown(_))
        || matches!(&observed.management_listener, Probe::Unknown(_))
        || matches!(&observed.management_listener_ipv4, Probe::Unknown(_));
    let degraded_to_router_only = desired_mode == Some(TailscaleMode::LanSubnetAccess)
        && effective_mode == Some(TailscaleMode::RouterOnly);
    TailscaleStatus {
        desired_mode,
        effective_mode,
        backend_state,
        authenticated,
        ipv4,
        route_advertised,
        local_firewall_ready,
        route_approval: TailscaleRouteApproval::UnknownExternalApprovalRequired,
        connection,
        explicit_proxy_desired,
        environment,
        explicit_proxy_path,
        proxy_fallback,
        error_category: if has_unknown || explicit_proxy_path == TailscaleExplicitProxyPath::Unknown
        {
            Some(TailscaleErrorCategory::ProbeFailed)
        } else if explicit_proxy_path == TailscaleExplicitProxyPath::Unavailable
            || degraded_to_router_only
            || proxy_fallback != TailscaleProxyFallback::NotNeeded
            || (desired_mode.is_some() && effective_mode.is_none() && authenticated != Some(false))
        {
            Some(TailscaleErrorCategory::NotReady)
        } else {
            None
        },
    }
}

pub fn tailscale_status_component_from_observed(
    observed: &TailscaleObserved,
    proxy_features: &Probe<ProxyFeaturesV1>,
    ordinary_router_ready: bool,
    explicit_proxy_path_ready: &Probe<bool>,
) -> Component<TailscaleStatus> {
    let status = tailscale_status_from_observed(
        observed,
        proxy_features,
        ordinary_router_ready,
        explicit_proxy_path_ready,
    );
    if status.error_category.is_none() {
        Component::available(status)
    } else {
        let (code, message) =
            if status.explicit_proxy_path == TailscaleExplicitProxyPath::Unavailable {
                (
                    "tailscale_proxy_path_unavailable",
                    "The fixed Tailscale explicit proxy path is unavailable",
                )
            } else {
                (
                    "tailscale_not_ready",
                    "Tailscale state does not satisfy strict readiness",
                )
            };
        Component::degraded(status, Issue::new(code, message))
    }
}

fn known_copy<T: Copy>(probe: &Probe<T>) -> Option<T> {
    match probe {
        Probe::Known(value) => Some(*value),
        Probe::Unknown(_) => None,
    }
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
    tailscale_platform: Arc<dyn StatusTailscalePlatformPort>,
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
        Self::with_cache_ttl(
            router_platform,
            Arc::new(UnavailableTailscaleStatus),
            system_probe,
            clock,
            STATUS_CACHE_TTL,
        )
    }

    pub fn new_with_tailscale(
        router_platform: Arc<dyn StatusRouterPlatformPort>,
        tailscale_platform: Arc<dyn StatusTailscalePlatformPort>,
        system_probe: Arc<dyn StatusSystemProbePort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self::with_cache_ttl(
            router_platform,
            tailscale_platform,
            system_probe,
            clock,
            STATUS_CACHE_TTL,
        )
    }

    #[cfg(feature = "e2e")]
    pub fn new_uncached(
        router_platform: Arc<dyn StatusRouterPlatformPort>,
        system_probe: Arc<dyn StatusSystemProbePort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self::with_cache_ttl(
            router_platform,
            Arc::new(UnavailableTailscaleStatus),
            system_probe,
            clock,
            Duration::ZERO,
        )
    }

    #[cfg(feature = "e2e")]
    pub fn new_uncached_with_tailscale(
        router_platform: Arc<dyn StatusRouterPlatformPort>,
        tailscale_platform: Arc<dyn StatusTailscalePlatformPort>,
        system_probe: Arc<dyn StatusSystemProbePort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self::with_cache_ttl(
            router_platform,
            tailscale_platform,
            system_probe,
            clock,
            Duration::ZERO,
        )
    }

    fn with_cache_ttl(
        router_platform: Arc<dyn StatusRouterPlatformPort>,
        tailscale_platform: Arc<dyn StatusTailscalePlatformPort>,
        system_probe: Arc<dyn StatusSystemProbePort>,
        clock: Arc<dyn ClockPort>,
        cache_ttl: Duration,
    ) -> Self {
        Self {
            router_platform,
            tailscale_platform,
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

        let (router, proxy, tailscale, system) = tokio::join!(
            self.router_platform.read_router_status(),
            self.router_platform.read_proxy_status(),
            self.tailscale_platform.read_tailscale_status(),
            self.system_probe.read_system_stats(),
        );
        let state = if router.is_available()
            && proxy.is_available()
            && tailscale.is_available()
            && system.is_available()
        {
            SnapshotState::Ok
        } else {
            SnapshotState::Degraded
        };

        let snapshot = StatusSnapshot {
            state,
            observed_at_unix_ms: self.clock.unix_time_millis(),
            router,
            proxy,
            tailscale,
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
