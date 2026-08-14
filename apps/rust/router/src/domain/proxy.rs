//! Mihomo shared-core and LAN TUN lifecycle model.

use super::{
    device_policy::LanDeviceMac,
    network::{OwnedResource, Probe},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MIHOMO_TUN_INTERFACE: &str = "hyz-mihomo";
pub const MIHOMO_MANGLE_CHAIN: &str = "HYZ_MIHOMO_PRE";
pub const MIHOMO_FILTER_CHAIN: &str = "HYZ_MIHOMO_FWD";
pub const MIHOMO_MARK: &str = "0x1000000/0x1000000";
pub const MIHOMO_RULE_PRIORITY: u32 = 11_000;
pub const MIHOMO_ROUTE_TABLE: u32 = 110;
pub const MIHOMO_MIXED_PORT: u16 = 7_890;
pub const MIHOMO_MIXED_ADDRESS: &str = "127.0.0.1:7890";

pub const CONTROLLED_TUN_ENABLED: &str = "\ntun:\n  enable: true\n  stack: system\n  device: hyz-mihomo\n  auto-route: false\n  auto-redirect: false\n  auto-detect-interface: false\n  strict-route: false\n  dns-hijack: []\n  mtu: 1500\n";
pub const CONTROLLED_TUN_DISABLED: &str = "\ntun:\n  enable: false\n";
pub const CONTROLLED_LOCAL_MIXED: &str =
    "\nmixed-port: 7890\nallow-lan: false\nbind-address: 127.0.0.1\nauthentication: []\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyFeaturesV1 {
    pub version: u8,
    pub lan_tun_enabled: bool,
    pub tailscale_explicit_proxy_enabled: bool,
}

impl ProxyFeaturesV1 {
    pub const VERSION: u8 = 1;

    pub const fn disabled() -> Self {
        Self {
            version: Self::VERSION,
            lan_tun_enabled: false,
            tailscale_explicit_proxy_enabled: false,
        }
    }

    pub const fn new(lan_tun_enabled: bool, tailscale_explicit_proxy_enabled: bool) -> Self {
        Self {
            version: Self::VERSION,
            lan_tun_enabled,
            tailscale_explicit_proxy_enabled,
        }
    }

    pub const fn mihomo_required(self) -> bool {
        self.lan_tun_enabled || self.tailscale_explicit_proxy_enabled
    }

    pub const fn supported(self) -> bool {
        self.version == Self::VERSION
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyDesired {
    pub lan_tun_enabled: bool,
    pub tailscale_explicit_proxy_enabled: bool,
    pub direct_macs: BTreeSet<LanDeviceMac>,
}

impl ProxyDesired {
    pub const fn features(&self) -> ProxyFeaturesV1 {
        ProxyFeaturesV1::new(self.lan_tun_enabled, self.tailscale_explicit_proxy_enabled)
    }

    pub const fn mihomo_required(&self) -> bool {
        self.lan_tun_enabled || self.tailscale_explicit_proxy_enabled
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyObserved {
    pub persisted_features: Probe<ProxyFeaturesV1>,
    pub process_identity_valid: Probe<bool>,
    pub watcher_identity_valid: Probe<bool>,
    pub runtime_config_valid: Probe<bool>,
    pub mixed_port_ready: Probe<bool>,
    pub tun_interface: Probe<OwnedResource>,
    pub tun_firewall: Probe<OwnedResource>,
    pub policy_rule_present: Probe<bool>,
    pub policy_route_present: Probe<bool>,
    pub interception_entry_present: Probe<bool>,
    pub ordinary_nat_confirmed: Probe<bool>,
    pub active_direct_macs: Probe<BTreeSet<LanDeviceMac>>,
}

impl ProxyObserved {
    pub fn unknown(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            persisted_features: Probe::Unknown(reason.clone()),
            process_identity_valid: Probe::Unknown(reason.clone()),
            watcher_identity_valid: Probe::Unknown(reason.clone()),
            runtime_config_valid: Probe::Unknown(reason.clone()),
            mixed_port_ready: Probe::Unknown(reason.clone()),
            tun_interface: Probe::Unknown(reason.clone()),
            tun_firewall: Probe::Unknown(reason.clone()),
            policy_rule_present: Probe::Unknown(reason.clone()),
            policy_route_present: Probe::Unknown(reason.clone()),
            interception_entry_present: Probe::Unknown(reason.clone()),
            ordinary_nat_confirmed: Probe::Unknown(reason.clone()),
            active_direct_macs: Probe::Unknown(reason),
        }
    }

    pub fn tun_resources_absent(&self) -> bool {
        self.tun_interface == Probe::Known(OwnedResource::Absent)
            && self.tun_firewall == Probe::Known(OwnedResource::Absent)
            && self.policy_rule_present == Probe::Known(false)
            && self.policy_route_present == Probe::Known(false)
            && self.interception_entry_present == Probe::Known(false)
    }

    pub fn core_ready(&self) -> bool {
        self.process_identity_valid == Probe::Known(true)
            && self.runtime_config_valid == Probe::Known(true)
            && self.mixed_port_ready == Probe::Known(true)
    }

    pub fn lan_tun_ready(&self, direct_macs: &BTreeSet<LanDeviceMac>) -> bool {
        self.core_ready()
            && self.watcher_identity_valid == Probe::Known(true)
            && matches!(
                self.tun_interface,
                Probe::Known(OwnedResource::Owned { .. })
            )
            && matches!(self.tun_firewall, Probe::Known(OwnedResource::Owned { .. }))
            && matches!(
                (&self.tun_interface, &self.tun_firewall),
                (
                    Probe::Known(OwnedResource::Owned { token: interface }),
                    Probe::Known(OwnedResource::Owned { token: firewall }),
                ) if interface == firewall
            )
            && self.policy_rule_present == Probe::Known(true)
            && self.policy_route_present == Probe::Known(true)
            && self.interception_entry_present == Probe::Known(true)
            && self.active_direct_macs == Probe::Known(direct_macs.clone())
    }

    pub fn ready_for_interception(
        &self,
        token: &str,
        direct_macs: &BTreeSet<LanDeviceMac>,
    ) -> bool {
        self.core_ready()
            && self.tun_interface
                == Probe::Known(OwnedResource::Owned {
                    token: token.to_owned(),
                })
            && self.tun_firewall
                == Probe::Known(OwnedResource::Owned {
                    token: token.to_owned(),
                })
            && self.policy_rule_present == Probe::Known(true)
            && self.policy_route_present == Probe::Known(true)
            && self.interception_entry_present == Probe::Known(false)
            && self.active_direct_macs == Probe::Known(direct_macs.clone())
    }

    pub fn runtime_ready_for(&self, desired: &ProxyDesired) -> bool {
        self.runtime_ready_for_with_ordinary_nat(desired, true)
    }

    pub fn runtime_ready_for_without_ordinary_nat(&self, desired: &ProxyDesired) -> bool {
        self.runtime_ready_for_with_ordinary_nat(desired, false)
    }

    fn runtime_ready_for_with_ordinary_nat(
        &self,
        desired: &ProxyDesired,
        require_ordinary_nat: bool,
    ) -> bool {
        if desired.mihomo_required() {
            if !self.core_ready() {
                return false;
            }
        } else if self.process_identity_valid != Probe::Known(false)
            || self.runtime_config_valid != Probe::Known(false)
            || self.mixed_port_ready != Probe::Known(false)
        {
            return false;
        }
        if desired.lan_tun_enabled {
            self.lan_tun_ready(&desired.direct_macs)
        } else {
            self.watcher_identity_valid == Probe::Known(false)
                && self.tun_resources_absent()
                && (!require_ordinary_nat || self.ordinary_nat_confirmed == Probe::Known(true))
        }
    }

    pub fn ready_for(&self, desired: &ProxyDesired) -> bool {
        self.persisted_features == Probe::Known(desired.features())
            && self.runtime_ready_for(desired)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProxyAction {
    RemoveInterceptionEntry {
        token: String,
    },
    RemovePolicyRule,
    RemovePolicyRoute,
    RemoveTunForwardHook {
        token: String,
    },
    RemoveTunChains {
        token: String,
    },
    StopWatcher,
    StopCore,
    RemoveRuntimeState,
    WriteRuntimeConfig {
        lan_tun_enabled: bool,
    },
    ValidateRuntimeConfig,
    StartCore,
    WaitForMixedPort,
    WaitForTunInterface {
        token: String,
    },
    CreateTunChains {
        token: String,
        direct_macs: BTreeSet<LanDeviceMac>,
    },
    InstallTunForwardHook {
        token: String,
    },
    InstallPolicyRoute,
    InstallPolicyRule,
    InstallInterceptionEntry {
        token: String,
    },
    StartWatcher,
    WaitForWatcher,
    CommitFeatures {
        features: ProxyFeaturesV1,
    },
    RestorePersistedFeatures {
        features: ProxyFeaturesV1,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_core_is_derived_from_the_two_public_features() {
        assert!(!ProxyFeaturesV1::disabled().mihomo_required());
        assert!(ProxyFeaturesV1::new(true, false).mihomo_required());
        assert!(ProxyFeaturesV1::new(false, true).mihomo_required());
        assert!(ProxyFeaturesV1::new(true, true).mihomo_required());
    }

    #[test]
    fn versioned_features_reject_unknown_fields_and_versions() {
        assert!(serde_json::from_str::<ProxyFeaturesV1>(
            r#"{"version":1,"lan_tun_enabled":true,"tailscale_explicit_proxy_enabled":false,"port":7890}"#
        )
        .is_err());
        let unsupported: ProxyFeaturesV1 = serde_json::from_str(
            r#"{"version":2,"lan_tun_enabled":false,"tailscale_explicit_proxy_enabled":false}"#,
        )
        .unwrap();
        assert!(!unsupported.supported());
    }
}
