use super::{
    process::{LinuxRouterPlatform, Tool},
    storage,
};
use crate::{
    application::ports::{DevicePolicyStorePort, LanClientDiscoveryPort, PlatformError},
    domain::device_policy::{
        validate_hostname, DevicePolicyConfigV1, LanClientObservation, LanDeviceMac,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    net::Ipv4Addr,
};

pub const DEVICE_POLICY_FILE: &str = "/userdata/hyz-router/device-policy-v1.json";
pub const DEVICE_POLICY_PENDING_FILE: &str = "/userdata/hyz-router/device-policy-pending-v1.json";
pub const DNSMASQ_LEASE_FILE: &str = "/run/hyz-router/dnsmasq.leases";
const MAX_POLICY_BYTES: usize = 64 * 1024;
const MAX_LEASE_BYTES: usize = 64 * 1024;
const MAX_STATIONS: usize = 128;
const MAX_DISCOVERED_CLIENTS: usize = 128;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingPolicy {
    previous: DevicePolicyConfigV1,
    candidate: DevicePolicyConfigV1,
}

impl DevicePolicyStorePort for LinuxRouterPlatform {
    fn load_device_policy(&self) -> Result<DevicePolicyConfigV1, PlatformError> {
        load_config(DEVICE_POLICY_FILE)
            .map(|config| config.unwrap_or_else(DevicePolicyConfigV1::empty))
    }

    fn load_pending_device_policy(
        &self,
    ) -> Result<Option<(DevicePolicyConfigV1, DevicePolicyConfigV1)>, PlatformError> {
        let Some(json) =
            storage::read_private_small_optional(DEVICE_POLICY_PENDING_FILE, MAX_POLICY_BYTES)?
        else {
            return Ok(None);
        };
        let pending: PendingPolicy = serde_json::from_str(&json).map_err(|error| {
            PlatformError::InvalidState(format!(
                "pending device policy journal is invalid: {error}"
            ))
        })?;
        Ok(Some((pending.previous, pending.candidate)))
    }

    fn stage_device_policy(
        &self,
        previous: &DevicePolicyConfigV1,
        candidate: &DevicePolicyConfigV1,
    ) -> Result<(), PlatformError> {
        let json = serde_json::to_vec(&PendingPolicy {
            previous: previous.clone(),
            candidate: candidate.clone(),
        })
        .map_err(|error| PlatformError::InvalidState(format!("encode policy journal: {error}")))?;
        storage::atomic_write_private(DEVICE_POLICY_PENDING_FILE, &json)
    }

    fn commit_device_policy(&self, candidate: &DevicePolicyConfigV1) -> Result<(), PlatformError> {
        let json = serde_json::to_vec(candidate).map_err(|error| {
            PlatformError::InvalidState(format!("encode device policy: {error}"))
        })?;
        storage::atomic_write_private(DEVICE_POLICY_FILE, &json)
    }

    fn clear_pending_device_policy(&self) -> Result<(), PlatformError> {
        storage::remove_file_durable(DEVICE_POLICY_PENDING_FILE)
    }
}

fn load_config(path: &str) -> Result<Option<DevicePolicyConfigV1>, PlatformError> {
    let Some(json) = storage::read_private_small_optional(path, MAX_POLICY_BYTES)? else {
        return Ok(None);
    };
    serde_json::from_str(&json).map(Some).map_err(|error| {
        PlatformError::InvalidState(format!("device policy file is invalid: {error}"))
    })
}

impl LanClientDiscoveryPort for LinuxRouterPlatform {
    fn discover_lan_clients(
        &self,
        config: &DevicePolicyConfigV1,
    ) -> Result<Vec<LanClientObservation>, PlatformError> {
        let leases = match storage::read_small_optional(DNSMASQ_LEASE_FILE, MAX_LEASE_BYTES)? {
            Some(contents) => parse_dnsmasq_leases(&contents)?,
            None => BTreeMap::new(),
        };
        let output = self.run(
            Tool::HostapdCli,
            &[
                "-i".to_owned(),
                crate::domain::network::LAN_MEMBER.to_owned(),
                "list_sta".to_owned(),
            ],
        )?;
        let associated = parse_hostapd_stations(&output.stdout)?;
        let mut macs = leases.keys().copied().collect::<BTreeSet<_>>();
        macs.extend(associated.iter().copied());
        macs.extend(config.entries.iter().map(|entry| entry.mac));
        if macs.len() > MAX_DISCOVERED_CLIENTS {
            return Err(PlatformError::InvalidState(
                "discovered LAN client set exceeds control-frame bound".to_owned(),
            ));
        }
        Ok(macs
            .into_iter()
            .map(|mac| {
                let lease = leases.get(&mac);
                LanClientObservation {
                    mac,
                    lease_address: lease.map(|lease| lease.0),
                    hostname: lease.and_then(|lease| lease.1.clone()),
                    associated: associated.contains(&mac),
                    policy: config.policy_for(mac),
                }
            })
            .collect())
    }
}

type LeaseMap = BTreeMap<LanDeviceMac, (Ipv4Addr, Option<String>)>;

pub(crate) fn parse_dnsmasq_leases(contents: &str) -> Result<LeaseMap, PlatformError> {
    if contents.len() > MAX_LEASE_BYTES {
        return Err(PlatformError::InvalidState(
            "dnsmasq lease data exceeds limit".to_owned(),
        ));
    }
    let mut leases = BTreeMap::new();
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() != 5 || fields[0].parse::<u64>().is_err() {
            return Err(PlatformError::InvalidState(
                "dnsmasq lease line is malformed".to_owned(),
            ));
        }
        let mac = fields[1].parse::<LanDeviceMac>().map_err(|error| {
            PlatformError::InvalidState(format!("dnsmasq lease MAC is invalid: {error}"))
        })?;
        let address = fields[2].parse::<Ipv4Addr>().map_err(|_| {
            PlatformError::InvalidState("dnsmasq lease address is invalid".to_owned())
        })?;
        let octets = address.octets();
        if octets[..3] != [192, 168, 8] || matches!(octets[3], 0 | 255) {
            return Err(PlatformError::InvalidState(
                "dnsmasq lease address is outside the LAN subnet".to_owned(),
            ));
        }
        let hostname = match fields[3] {
            "*" => None,
            value if validate_hostname(value) => Some(value.to_owned()),
            _ => {
                return Err(PlatformError::InvalidState(
                    "dnsmasq hostname is invalid".to_owned(),
                ))
            }
        };
        if leases.insert(mac, (address, hostname)).is_some() {
            return Err(PlatformError::InvalidState(
                "dnsmasq lease has duplicate MAC".to_owned(),
            ));
        }
        if leases.len() > MAX_DISCOVERED_CLIENTS {
            return Err(PlatformError::InvalidState(
                "dnsmasq lease count exceeds discovery bound".to_owned(),
            ));
        }
    }
    Ok(leases)
}

pub(crate) fn parse_hostapd_stations(
    contents: &str,
) -> Result<BTreeSet<LanDeviceMac>, PlatformError> {
    if contents.len() > MAX_LEASE_BYTES {
        return Err(PlatformError::InvalidState(
            "hostapd station data exceeds limit".to_owned(),
        ));
    }
    let mut stations = BTreeSet::new();
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
        let mac = line.parse::<LanDeviceMac>().map_err(|_| {
            PlatformError::InvalidState("hostapd station list is malformed".to_owned())
        })?;
        if !stations.insert(mac) || stations.len() > MAX_STATIONS {
            return Err(PlatformError::InvalidState(
                "hostapd station list is duplicate or excessive".to_owned(),
            ));
        }
    }
    Ok(stations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_discovery_parsers_are_strict() {
        let leases =
            parse_dnsmasq_leases("1786406400 02:00:00:00:00:01 192.168.8.10 phone *\n").unwrap();
        assert_eq!(leases.len(), 1);
        assert!(parse_dnsmasq_leases("bad 02:00:00:00:00:01 192.168.8.10 phone *\n").is_err());
        assert!(parse_dnsmasq_leases("1 02:00:00:00:00:01 192.168.8.10 bad/name *\n").is_err());
        assert!(parse_dnsmasq_leases("1 02:00:00:00:00:01 192.168.9.10 phone *\n").is_err());
        assert!(parse_dnsmasq_leases("1 02:00:00:00:00:01 192.168.8.255 phone *\n").is_err());
        assert!(parse_hostapd_stations("02:00:00:00:00:01\n02:00:00:00:00:01\n").is_err());
    }

    #[test]
    fn persisted_dtos_reject_unknown_fields() {
        assert!(serde_json::from_str::<PendingPolicy>(r#"{"previous":{"version":1,"generation":0,"entries":[]},"candidate":{"version":1,"generation":1,"entries":[]},"command":"x"}"#).is_err());
    }
}
