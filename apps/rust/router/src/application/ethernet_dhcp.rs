use crate::application::ports::PlatformError;

pub trait EthernetDhcpLifecyclePort: Send + Sync {
    fn ethernet_carrier_up(&self) -> Result<bool, PlatformError>;
    fn management_wifi_ready(&self) -> Result<bool, PlatformError>;
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
        if self.platform.ethernet_carrier_up()? {
            if self.platform.management_wifi_ready()? {
                self.platform.start_ethernet_dhcp()
            } else {
                Ok(())
            }
        } else {
            self.platform.stop_ethernet_dhcp()
        }
    }

    pub fn shutdown(&self) -> Result<(), PlatformError> {
        self.platform.stop_ethernet_dhcp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Fake {
        carrier_up: bool,
        management_wifi_ready: bool,
        ethernet_actions: Mutex<Vec<&'static str>>,
        wifi_process_actions: Mutex<Vec<&'static str>>,
        wifi_ownership_actions: Mutex<Vec<&'static str>>,
    }

    impl EthernetDhcpLifecyclePort for Fake {
        fn ethernet_carrier_up(&self) -> Result<bool, PlatformError> {
            Ok(self.carrier_up)
        }

        fn management_wifi_ready(&self) -> Result<bool, PlatformError> {
            Ok(self.management_wifi_ready)
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

    fn fake(carrier_up: bool, management_wifi_ready: bool) -> Fake {
        Fake {
            carrier_up,
            management_wifi_ready,
            ethernet_actions: Mutex::new(Vec::new()),
            wifi_process_actions: Mutex::new(Vec::new()),
            wifi_ownership_actions: Mutex::new(Vec::new()),
        }
    }

    #[test]
    fn carrier_up_starts_only_ethernet_dhcp_after_management_wifi_is_ready() {
        let fake = fake(true, true);

        EthernetDhcpLifecycleApplication::new(&fake)
            .reconcile()
            .unwrap();

        assert_eq!(*fake.ethernet_actions.lock().unwrap(), vec!["start"]);
        assert!(fake.wifi_process_actions.lock().unwrap().is_empty());
        assert!(fake.wifi_ownership_actions.lock().unwrap().is_empty());
    }

    #[test]
    fn carrier_down_cleans_up_only_ethernet_dhcp() {
        let fake = fake(false, true);

        EthernetDhcpLifecycleApplication::new(&fake)
            .reconcile()
            .unwrap();

        assert_eq!(*fake.ethernet_actions.lock().unwrap(), vec!["stop"]);
        assert!(fake.wifi_process_actions.lock().unwrap().is_empty());
        assert!(fake.wifi_ownership_actions.lock().unwrap().is_empty());
    }

    #[test]
    fn carrier_up_does_not_start_dhcp_during_management_wifi_startup() {
        let fake = fake(true, false);

        EthernetDhcpLifecycleApplication::new(&fake)
            .reconcile()
            .unwrap();

        assert!(fake.ethernet_actions.lock().unwrap().is_empty());
        assert!(fake.wifi_process_actions.lock().unwrap().is_empty());
        assert!(fake.wifi_ownership_actions.lock().unwrap().is_empty());
    }
}
