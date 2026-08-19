use crate::application::ports::{LifecycleLease, PlatformError};

pub trait EthernetDhcpLifecyclePort: Send + Sync {
    fn acquire_lifecycle_lock(&self) -> Result<LifecycleLease, PlatformError>;
    fn release_lifecycle_lock(&self, lease: &LifecycleLease) -> Result<(), PlatformError>;
    fn ensure_ethernet_link_up(&self) -> Result<(), PlatformError>;
    fn ethernet_carrier_up(&self) -> Result<bool, PlatformError>;
    fn start_ethernet_dhcp(&self) -> Result<(), PlatformError>;
    fn stop_ethernet_dhcp(&self) -> Result<(), PlatformError>;
}

pub struct EthernetDhcpLifecycleApplication<'a> {
    platform: &'a dyn EthernetDhcpLifecyclePort,
}

impl<'a> EthernetDhcpLifecycleApplication<'a> {
    pub fn new(platform: &'a dyn EthernetDhcpLifecyclePort) -> Self {
        Self { platform }
    }

    pub fn reconcile(&self) -> Result<(), PlatformError> {
        let lease = self.platform.acquire_lifecycle_lock()?;
        let result = self.reconcile_locked();
        let release = self.platform.release_lifecycle_lock(&lease);
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    pub fn shutdown(&self) -> Result<(), PlatformError> {
        let lease = self.platform.acquire_lifecycle_lock()?;
        let result = self.platform.stop_ethernet_dhcp();
        let release = self.platform.release_lifecycle_lock(&lease);
        match (result, release) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn reconcile_locked(&self) -> Result<(), PlatformError> {
        self.platform.ensure_ethernet_link_up()?;
        if self.platform.ethernet_carrier_up()? {
            self.platform.start_ethernet_dhcp()
        } else {
            self.platform.stop_ethernet_dhcp()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Fake {
        carrier_up: bool,
        ethernet_actions: Mutex<Vec<&'static str>>,
        lock_actions: Mutex<Vec<&'static str>>,
    }

    impl EthernetDhcpLifecyclePort for Fake {
        fn acquire_lifecycle_lock(&self) -> Result<LifecycleLease, PlatformError> {
            self.lock_actions.lock().unwrap().push("acquire");
            Ok(LifecycleLease {
                path: "test",
                identity: "test".to_owned(),
                directory_device: 1,
                directory_inode: 1,
            })
        }

        fn release_lifecycle_lock(&self, _: &LifecycleLease) -> Result<(), PlatformError> {
            self.lock_actions.lock().unwrap().push("release");
            Ok(())
        }

        fn ensure_ethernet_link_up(&self) -> Result<(), PlatformError> {
            self.ethernet_actions.lock().unwrap().push("ensure");
            Ok(())
        }

        fn ethernet_carrier_up(&self) -> Result<bool, PlatformError> {
            Ok(self.carrier_up)
        }

        fn start_ethernet_dhcp(&self) -> Result<(), PlatformError> {
            self.ethernet_actions.lock().unwrap().push("start");
            Ok(())
        }

        fn stop_ethernet_dhcp(&self) -> Result<(), PlatformError> {
            self.ethernet_actions.lock().unwrap().push("stop");
            Ok(())
        }
    }

    fn fake(carrier_up: bool) -> Fake {
        Fake {
            carrier_up,
            ethernet_actions: Mutex::new(Vec::new()),
            lock_actions: Mutex::new(Vec::new()),
        }
    }

    #[test]
    fn carrier_up_starts_ethernet_dhcp_without_a_wifi_lease_dependency() {
        let fake = fake(true);

        EthernetDhcpLifecycleApplication::new(&fake)
            .reconcile()
            .unwrap();

        assert_eq!(
            *fake.ethernet_actions.lock().unwrap(),
            vec!["ensure", "start"]
        );
        assert_eq!(
            *fake.lock_actions.lock().unwrap(),
            vec!["acquire", "release"]
        );
    }

    #[test]
    fn shutdown_stops_ethernet_dhcp_without_consulting_wifi_state() {
        let fake = fake(true);

        EthernetDhcpLifecycleApplication::new(&fake)
            .shutdown()
            .unwrap();

        assert_eq!(*fake.ethernet_actions.lock().unwrap(), vec!["stop"]);
        assert_eq!(
            *fake.lock_actions.lock().unwrap(),
            vec!["acquire", "release"]
        );
    }

    #[test]
    fn carrier_down_cleans_up_only_ethernet_dhcp() {
        let fake = fake(false);

        EthernetDhcpLifecycleApplication::new(&fake)
            .reconcile()
            .unwrap();

        assert_eq!(
            *fake.ethernet_actions.lock().unwrap(),
            vec!["ensure", "stop"]
        );
    }
}
