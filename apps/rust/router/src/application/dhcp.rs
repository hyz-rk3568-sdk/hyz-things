use crate::application::ports::{LifecycleLease, PlatformError, RouterPlatformPort};
use serde::{Deserialize, Serialize};
use std::{
    net::Ipv4Addr,
    thread,
    time::{Duration, Instant},
};

pub const DHCP_HOOK_ROLE_ENV: &str = "HYZ_ROUTER_INTERNAL_DHCP_HOOK";
pub const DHCP_GENERATION_ENV: &str = "HYZ_ROUTER_DHCP_GENERATION";
const LIFECYCLE_LOCK_WAIT: Duration = Duration::from_secs(3 * 60);
const LIFECYCLE_LOCK_RETRY: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct DhcpGeneration(String);

impl<'de> Deserialize<'de> for DhcpGeneration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl DhcpGeneration {
    pub fn new(value: String) -> Result<Self, PlatformError> {
        if value.is_empty()
            || value.len() > 96
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
        {
            return Err(PlatformError::InvalidState(
                "DHCP generation has invalid characters or length".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DhcpEvent {
    pub generation: DhcpGeneration,
    pub transition: DhcpTransition,
}

impl DhcpEvent {
    pub fn new(generation: DhcpGeneration, transition: DhcpTransition) -> Self {
        Self {
            generation,
            transition,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DhcpTransition {
    Deconfig,
    Lease { lease: DhcpLease },
    NoChange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DhcpLease {
    pub address: Ipv4Addr,
    pub prefix: u8,
    pub broadcast: Option<Ipv4Addr>,
    pub routers: Vec<Ipv4Addr>,
    pub static_routes: Vec<(String, Ipv4Addr)>,
    pub dns: Vec<Ipv4Addr>,
    pub search: Vec<String>,
}

impl DhcpLease {
    pub fn resolver_lines(&self, interface: &str) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.search.is_empty() {
            lines.push(format!("search {} # {interface}", self.search.join(" ")));
        }
        lines.extend(
            self.dns
                .iter()
                .map(|address| format!("nameserver {address} # {interface}")),
        );
        lines
    }
}

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
