//! Mihomo lifecycle model. Readiness is deliberately stricter than liveness.

use super::{
    device_policy::LanDeviceMac,
    network::{OwnedResource, Probe},
};
use std::collections::BTreeSet;

pub const MIHOMO_TUN_INTERFACE: &str = "hyz-mihomo";
pub const MIHOMO_MANGLE_CHAIN: &str = "HYZ_MIHOMO_PRE";
pub const MIHOMO_FILTER_CHAIN: &str = "HYZ_MIHOMO_FWD";
pub const MIHOMO_MARK: &str = "0x1000000/0x1000000";
pub const MIHOMO_RULE_PRIORITY: u32 = 11_000;
pub const MIHOMO_ROUTE_TABLE: u32 = 110;

pub const CONTROLLED_TUN_ENABLED: &str = "\ntun:\n  enable: true\n  stack: system\n  device: hyz-mihomo\n  auto-route: false\n  auto-redirect: false\n  auto-detect-interface: false\n  strict-route: false\n  dns-hijack: []\n  mtu: 1500\n";
pub const CONTROLLED_TUN_DISABLED: &str = "\ntun:\n  enable: false\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMode {
    Disabled,
    Explicit,
    Tun,
}

impl ProxyMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Explicit => "explicit",
            Self::Tun => "tun",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyDesired {
    pub mode: ProxyMode,
    pub direct_macs: BTreeSet<LanDeviceMac>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyObserved {
    pub persisted_mode: Probe<Option<ProxyMode>>,
    pub process_identity_valid: Probe<bool>,
    pub watcher_identity_valid: Probe<bool>,
    pub runtime_config_valid: Probe<bool>,
    pub tun_interface_present: Probe<bool>,
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
            persisted_mode: Probe::Unknown(reason.clone()),
            process_identity_valid: Probe::Unknown(reason.clone()),
            watcher_identity_valid: Probe::Unknown(reason.clone()),
            runtime_config_valid: Probe::Unknown(reason.clone()),
            tun_interface_present: Probe::Unknown(reason.clone()),
            tun_firewall: Probe::Unknown(reason.clone()),
            policy_rule_present: Probe::Unknown(reason.clone()),
            policy_route_present: Probe::Unknown(reason.clone()),
            interception_entry_present: Probe::Unknown(reason.clone()),
            ordinary_nat_confirmed: Probe::Unknown(reason.clone()),
            active_direct_macs: Probe::Unknown(reason),
        }
    }

    pub fn effective_persisted_mode(&self) -> Probe<ProxyMode> {
        self.persisted_mode
            .clone()
            .map(|mode| mode.unwrap_or(ProxyMode::Explicit))
    }

    pub fn tun_resources_absent(&self) -> bool {
        self.tun_interface_present == Probe::Known(false)
            && self.tun_firewall == Probe::Known(OwnedResource::Absent)
            && self.policy_rule_present == Probe::Known(false)
            && self.policy_route_present == Probe::Known(false)
            && self.interception_entry_present == Probe::Known(false)
    }

    pub fn ready_for(&self, desired: &ProxyDesired) -> bool {
        let persisted_matches = self.persisted_mode == Probe::Known(Some(desired.mode))
            || desired.mode == ProxyMode::Explicit && self.persisted_mode == Probe::Known(None);
        if !persisted_matches {
            return false;
        }
        match desired.mode {
            ProxyMode::Disabled => {
                self.process_identity_valid == Probe::Known(false)
                    && self.watcher_identity_valid == Probe::Known(false)
                    && self.tun_resources_absent()
                    && self.ordinary_nat_confirmed == Probe::Known(true)
            }
            ProxyMode::Explicit => {
                self.process_identity_valid == Probe::Known(true)
                    && self.watcher_identity_valid == Probe::Known(false)
                    && self.runtime_config_valid == Probe::Known(true)
                    && self.tun_resources_absent()
                    && self.ordinary_nat_confirmed == Probe::Known(true)
            }
            ProxyMode::Tun => {
                self.process_identity_valid == Probe::Known(true)
                    && self.watcher_identity_valid == Probe::Known(true)
                    && self.runtime_config_valid == Probe::Known(true)
                    && self.tun_interface_present == Probe::Known(true)
                    && matches!(self.tun_firewall, Probe::Known(OwnedResource::Owned { .. }))
                    && self.policy_rule_present == Probe::Known(true)
                    && self.policy_route_present == Probe::Known(true)
                    && self.interception_entry_present == Probe::Known(true)
                    && self.active_direct_macs == Probe::Known(desired.direct_macs.clone())
            }
        }
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
    WriteRuntimeConfig {
        mode: ProxyMode,
    },
    ValidateRuntimeConfig,
    StartCore,
    WaitForTunInterface,
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
    CommitMode {
        mode: ProxyMode,
    },
    RestorePersistedMode {
        mode: Option<ProxyMode>,
    },
}
