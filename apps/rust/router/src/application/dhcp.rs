use crate::application::ports::{LifecycleLease, PlatformError, RouterPlatformPort};
use std::{
    thread,
    time::{Duration, Instant},
};

pub use hyz_contract::dhcp::{DhcpEvent, DhcpGeneration, DhcpLease, DhcpTransition};

pub const DHCP_HOOK_ROLE_ENV: &str = "HYZ_ROUTER_INTERNAL_DHCP_HOOK";
pub const DHCP_GENERATION_ENV: &str = "HYZ_ROUTER_DHCP_GENERATION";
const LIFECYCLE_LOCK_WAIT: Duration = Duration::from_secs(3 * 60);
const LIFECYCLE_LOCK_RETRY: Duration = Duration::from_millis(20);

pub trait DhcpPlatformPort: Send + Sync {
    fn active_dhcp_generation(&self) -> Result<Option<DhcpGeneration>, PlatformError>;
    fn apply_dhcp_event(&self, event: &DhcpEvent) -> Result<(), PlatformError>;
}

pub struct DhcpApplication<'a> {
    lifecycle: &'a dyn RouterPlatformPort,
    platform: &'a dyn DhcpPlatformPort,
}

impl<'a> DhcpApplication<'a> {
    pub fn new(lifecycle: &'a dyn RouterPlatformPort, platform: &'a dyn DhcpPlatformPort) -> Self {
        Self {
            lifecycle,
            platform,
        }
    }

    pub fn execute(&self, event: &DhcpEvent) -> Result<(), PlatformError> {
        let lease = self.acquire_lifecycle_lock_bounded()?;
        let result = (|| {
            let active = self.platform.active_dhcp_generation()?;
            if active.as_ref() != Some(&event.generation) {
                return Err(PlatformError::Conflict(
                    "DHCP callback generation is stale".to_owned(),
                ));
            }
            self.platform.apply_dhcp_event(event)
        })();
        release_lifecycle(self.lifecycle, &lease, result)
    }

    fn acquire_lifecycle_lock_bounded(&self) -> Result<LifecycleLease, PlatformError> {
        let deadline = Instant::now() + LIFECYCLE_LOCK_WAIT;
        loop {
            match self.lifecycle.acquire_lifecycle_lock() {
                Ok(lease) => return Ok(lease),
                Err(PlatformError::Busy(_)) if Instant::now() < deadline => {
                    thread::sleep(LIFECYCLE_LOCK_RETRY);
                }
                Err(error) => return Err(error),
            }
        }
    }
}

fn release_lifecycle<T>(
    platform: &dyn RouterPlatformPort,
    lease: &LifecycleLease,
    result: Result<T, PlatformError>,
) -> Result<T, PlatformError> {
    let release = platform.release_lifecycle_lock(lease);
    match (result, release) {
        (Err(primary), Err(release)) => Err(PlatformError::InvalidState(format!(
            "DHCP operation failed: {primary}; lifecycle lock release also failed: {release}"
        ))),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::ports::RouterPlatformPort,
        domain::{network::NetworkAction, proxy::ProxyAction},
    };
    use std::sync::Mutex;

    struct Fake {
        active: Option<DhcpGeneration>,
        events: Mutex<Vec<&'static str>>,
    }

    impl RouterPlatformPort for Fake {
        fn acquire_lifecycle_lock(&self) -> Result<LifecycleLease, PlatformError> {
            self.events.lock().unwrap().push("lock");
            Ok(LifecycleLease {
                path: "test",
                identity: "test".to_owned(),
                directory_device: 1,
                directory_inode: 1,
            })
        }

        fn release_lifecycle_lock(&self, _: &LifecycleLease) -> Result<(), PlatformError> {
            self.events.lock().unwrap().push("release");
            Ok(())
        }

        fn apply_network(&self, _: &NetworkAction) -> Result<(), PlatformError> {
            unreachable!()
        }

        fn apply_proxy(&self, _: &ProxyAction) -> Result<(), PlatformError> {
            unreachable!()
        }
    }

    impl DhcpPlatformPort for Fake {
        fn active_dhcp_generation(&self) -> Result<Option<DhcpGeneration>, PlatformError> {
            self.events.lock().unwrap().push("generation");
            Ok(self.active.clone())
        }

        fn apply_dhcp_event(&self, _: &DhcpEvent) -> Result<(), PlatformError> {
            self.events.lock().unwrap().push("apply");
            Ok(())
        }
    }

    #[test]
    fn stale_generation_is_rejected_under_the_shared_lifecycle_lock() {
        let active = DhcpGeneration::new("active".to_owned()).unwrap();
        let stale = DhcpGeneration::new("stale".to_owned()).unwrap();
        let fake = Fake {
            active: Some(active),
            events: Mutex::new(Vec::new()),
        };
        let error = DhcpApplication::new(&fake, &fake)
            .execute(&DhcpEvent::new(stale, DhcpTransition::NoChange))
            .unwrap_err();
        assert!(matches!(error, PlatformError::Conflict(_)));
        assert_eq!(
            *fake.events.lock().unwrap(),
            vec!["lock", "generation", "release"]
        );
    }

    #[test]
    fn current_generation_applies_before_releasing_the_shared_lock() {
        let generation = DhcpGeneration::new("active".to_owned()).unwrap();
        let fake = Fake {
            active: Some(generation.clone()),
            events: Mutex::new(Vec::new()),
        };
        DhcpApplication::new(&fake, &fake)
            .execute(&DhcpEvent::new(generation, DhcpTransition::NoChange))
            .unwrap();
        assert_eq!(
            *fake.events.lock().unwrap(),
            vec!["lock", "generation", "apply", "release"]
        );
    }
}
