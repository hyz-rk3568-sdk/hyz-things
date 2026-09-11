//! Bounded LAN activity metadata exposed by the router control plane.

use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

use crate::device_policy::{LanDeviceMac, MAX_DEVICE_LABEL_BYTES};

pub const MAX_ACTIVITY_RECORDS: usize = 32;
pub const MAX_ACTIVITY_CONNECTION_ID_BYTES: usize = 96;
pub const MAX_ACTIVITY_TARGET_BYTES: usize = 253;
pub const MAX_ACTIVITY_RULE_BYTES: usize = 96;
pub const MAX_ACTIVITY_CHAIN_ENTRIES: usize = 4;
pub const MAX_ACTIVITY_CHAIN_NAME_BYTES: usize = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityNetwork {
    Tcp,
    Udp,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanActivityRecord {
    pub connection_id: String,
    pub mac: LanDeviceMac,
    pub device_name: String,
    pub source_address: Ipv4Addr,
    pub target: String,
    pub destination_port: u16,
    pub network: ActivityNetwork,
    pub rule: String,
    pub chains: Vec<String>,
    pub upload_bytes: u64,
    pub download_bytes: u64,
    pub first_seen_unix_ms: u64,
    pub last_seen_unix_ms: u64,
    pub active: bool,
}

impl LanActivityRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        connection_id: String,
        mac: LanDeviceMac,
        device_name: String,
        source_address: Ipv4Addr,
        target: String,
        destination_port: u16,
        network: ActivityNetwork,
        rule: String,
        chains: Vec<String>,
        upload_bytes: u64,
        download_bytes: u64,
        first_seen_unix_ms: u64,
        last_seen_unix_ms: u64,
        active: bool,
    ) -> Result<Self, &'static str> {
        validate_text(
            &connection_id,
            MAX_ACTIVITY_CONNECTION_ID_BYTES,
            "connection id is invalid",
        )?;
        validate_text(
            &device_name,
            MAX_DEVICE_LABEL_BYTES.max(17),
            "device name is invalid",
        )?;
        validate_text(&target, MAX_ACTIVITY_TARGET_BYTES, "activity target is invalid")?;
        if destination_port == 0 {
            return Err("activity destination port must be non-zero");
        }
        validate_text(&rule, MAX_ACTIVITY_RULE_BYTES, "activity rule is invalid")?;
        if chains.len() > MAX_ACTIVITY_CHAIN_ENTRIES {
            return Err("activity proxy chain is too long");
        }
        for chain in &chains {
            validate_text(
                chain,
                MAX_ACTIVITY_CHAIN_NAME_BYTES,
                "activity proxy chain name is invalid",
            )?;
        }
        if first_seen_unix_ms > last_seen_unix_ms {
            return Err("activity timestamps are out of order");
        }
        Ok(Self {
            connection_id,
            mac,
            device_name,
            source_address,
            target,
            destination_port,
            network,
            rule,
            chains,
            upload_bytes,
            download_bytes,
            first_seen_unix_ms,
            last_seen_unix_ms,
            active,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanActivitySnapshot {
    pub observed_at_unix_ms: u64,
    pub records: Vec<LanActivityRecord>,
}

impl LanActivitySnapshot {
    pub fn new(
        observed_at_unix_ms: u64,
        records: Vec<LanActivityRecord>,
    ) -> Result<Self, &'static str> {
        if records.len() > MAX_ACTIVITY_RECORDS {
            return Err("activity history exceeds the control-plane bound");
        }
        Ok(Self {
            observed_at_unix_ms,
            records,
        })
    }
}

fn validate_text(value: &str, max_bytes: usize, error: &'static str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > max_bytes
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        Err(error)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(index: usize) -> LanActivityRecord {
        LanActivityRecord::new(
            format!("connection-{index:02}"),
            format!("02:00:00:00:00:{index:02x}").parse().unwrap(),
            format!("device-{index:02}"),
            Ipv4Addr::new(192, 168, 8, index as u8 + 2),
            "x".repeat(MAX_ACTIVITY_TARGET_BYTES),
            443,
            ActivityNetwork::Tcp,
            "MATCH · HYZ-PROXY".to_owned(),
            vec!["node".repeat(20)],
            u64::MAX,
            u64::MAX,
            1,
            2,
            index % 2 == 0,
        )
        .unwrap()
    }

    #[test]
    fn activity_contract_rejects_unbounded_or_secret_like_free_form_fields() {
        let mac = "02:00:00:00:00:01".parse().unwrap();
        assert!(LanActivityRecord::new(
            "id".to_owned(),
            mac,
            "phone".to_owned(),
            Ipv4Addr::new(192, 168, 8, 10),
            "example.com".to_owned(),
            443,
            ActivityNetwork::Tcp,
            "MATCH".to_owned(),
            vec!["x".repeat(MAX_ACTIVITY_CHAIN_NAME_BYTES + 1)],
            0,
            0,
            1,
            1,
            true,
        )
        .is_err());
        assert!(LanActivitySnapshot::new(
            2,
            (0..=MAX_ACTIVITY_RECORDS).map(record).collect(),
        )
        .is_err());
    }

    #[test]
    fn maximum_activity_snapshot_fits_the_router_control_frame() {
        let snapshot = LanActivitySnapshot::new(
            2,
            (0..MAX_ACTIVITY_RECORDS).map(record).collect(),
        )
        .unwrap();
        assert!(serde_json::to_vec(&snapshot).unwrap().len() < 64 * 1024);
    }
}
