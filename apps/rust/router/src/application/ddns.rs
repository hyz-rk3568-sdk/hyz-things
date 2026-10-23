use std::net::Ipv6Addr;

use crate::{application::ports::PlatformError, domain::ddns::AliyunDdnsConfig};

pub trait DdnsConfigPort: Send + Sync {
    fn load_aliyun_ddns_config(&self) -> Result<Option<AliyunDdnsConfig>, PlatformError>;
}

pub trait DdnsAddressPort: Send + Sync {
    fn stable_public_ipv6(&self) -> Result<Option<Ipv6Addr>, PlatformError>;
}

pub trait DdnsProviderPort: Send + Sync {
    fn reconcile_aaaa(
        &self,
        config: &AliyunDdnsConfig,
        address: Ipv6Addr,
    ) -> Result<DdnsProviderResult, PlatformError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DdnsProviderResult {
    Updated,
    Unchanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DdnsRunResult {
    Disabled,
    WaitingForStableAddress,
    Updated,
    Unchanged,
}

pub struct DdnsApplication<'a> {
    config: &'a dyn DdnsConfigPort,
    address: &'a dyn DdnsAddressPort,
    provider: &'a dyn DdnsProviderPort,
}

impl<'a> DdnsApplication<'a> {
    pub fn new(
        config: &'a dyn DdnsConfigPort,
        address: &'a dyn DdnsAddressPort,
        provider: &'a dyn DdnsProviderPort,
    ) -> Self {
        Self {
            config,
            address,
            provider,
        }
    }

    pub fn reconcile_once(&self) -> Result<DdnsRunResult, PlatformError> {
        let Some(config) = self.config.load_aliyun_ddns_config()? else {
            return Ok(DdnsRunResult::Disabled);
        };
        let Some(address) = self.address.stable_public_ipv6()? else {
            return Ok(DdnsRunResult::WaitingForStableAddress);
        };
        match self.provider.reconcile_aaaa(&config, address)? {
            DdnsProviderResult::Updated => Ok(DdnsRunResult::Updated),
            DdnsProviderResult::Unchanged => Ok(DdnsRunResult::Unchanged),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };

    struct ConfigFake {
        enabled: bool,
    }

    impl DdnsConfigPort for ConfigFake {
        fn load_aliyun_ddns_config(&self) -> Result<Option<AliyunDdnsConfig>, PlatformError> {
            self.enabled
                .then(|| {
                    AliyunDdnsConfig::new(
                        "LTAI1234567890".to_owned(),
                        "test-secret".to_owned(),
                        "example.invalid".to_owned(),
                        "things".to_owned(),
                    )
                    .map_err(|error| PlatformError::InvalidState(error.to_string()))
                })
                .transpose()
        }
    }

    struct AddressFake(Option<Ipv6Addr>, AtomicUsize);

    impl DdnsAddressPort for AddressFake {
        fn stable_public_ipv6(&self) -> Result<Option<Ipv6Addr>, PlatformError> {
            self.1.fetch_add(1, Ordering::SeqCst);
            Ok(self.0)
        }
    }

    #[derive(Default)]
    struct ProviderFake(Mutex<Vec<Ipv6Addr>>);

    impl DdnsProviderPort for ProviderFake {
        fn reconcile_aaaa(
            &self,
            _config: &AliyunDdnsConfig,
            address: Ipv6Addr,
        ) -> Result<DdnsProviderResult, PlatformError> {
            self.0.lock().unwrap().push(address);
            Ok(DdnsProviderResult::Updated)
        }
    }

    #[test]
    fn missing_credentials_disables_ddns_without_probing_or_sending_requests() {
        let config = ConfigFake { enabled: false };
        let address = AddressFake(
            Some("2606:4700:4700::2222".parse().unwrap()),
            AtomicUsize::new(0),
        );
        let provider = ProviderFake::default();

        let result = DdnsApplication::new(&config, &address, &provider)
            .reconcile_once()
            .unwrap();

        assert_eq!(result, DdnsRunResult::Disabled);
        assert_eq!(address.1.load(Ordering::SeqCst), 0);
        assert!(provider.0.lock().unwrap().is_empty());
    }

    #[test]
    fn waits_for_a_stable_public_ipv6_without_sending_a_dns_request() {
        let config = ConfigFake { enabled: true };
        let address = AddressFake(None, AtomicUsize::new(0));
        let provider = ProviderFake::default();

        let result = DdnsApplication::new(&config, &address, &provider)
            .reconcile_once()
            .unwrap();

        assert_eq!(result, DdnsRunResult::WaitingForStableAddress);
        assert_eq!(address.1.load(Ordering::SeqCst), 1);
        assert!(provider.0.lock().unwrap().is_empty());
    }

    #[test]
    fn updates_the_configured_aaaa_record_with_the_stable_ipv6_address() {
        let expected: Ipv6Addr = "2606:4700:4700::2222".parse().unwrap();
        let config = ConfigFake { enabled: true };
        let address = AddressFake(Some(expected), AtomicUsize::new(0));
        let provider = ProviderFake::default();

        let result = DdnsApplication::new(&config, &address, &provider)
            .reconcile_once()
            .unwrap();

        assert_eq!(result, DdnsRunResult::Updated);
        assert_eq!(*provider.0.lock().unwrap(), vec![expected]);
    }
}
