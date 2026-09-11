use std::{
    collections::HashMap,
    net::Ipv4Addr,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use hyz_contract::activity::{
    ActivityNetwork, LanActivityRecord, LanActivitySnapshot, MAX_ACTIVITY_CHAIN_ENTRIES,
    MAX_ACTIVITY_CHAIN_NAME_BYTES, MAX_ACTIVITY_CONNECTION_ID_BYTES, MAX_ACTIVITY_RECORDS,
    MAX_ACTIVITY_RULE_BYTES, MAX_ACTIVITY_TARGET_BYTES,
};
use serde_json::Value;

use super::{
    paths::{MIHOMO_CONTROLLER_ADDRESS, MIHOMO_CONTROLLER_SECRET},
    process::LinuxRouterPlatform,
    storage,
};
use crate::{
    application::ports::{ClockPort, PlatformError},
    domain::device_policy::{DevicePolicyConfigV1, LanClientObservation, LanDeviceMac},
};

const CONTROLLER_RESPONSE_LIMIT: usize = 2 * 1024 * 1024;
const CONTROLLER_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONTROLLER_CONNECTIONS: usize = 4096;
static ACTIVITY_HISTORY: OnceLock<Mutex<Vec<LanActivityRecord>>> = OnceLock::new();

pub(crate) fn read_lan_activity(
    platform: &LinuxRouterPlatform,
    config: &DevicePolicyConfigV1,
    clients: &[LanClientObservation],
) -> Result<LanActivitySnapshot, PlatformError> {
    let now = platform.unix_time_millis();
    let value = controller_connections()?;
    let current = parse_connections(&value, config, clients, now)?;
    let mut history = ACTIVITY_HISTORY
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map_err(|_| PlatformError::InvalidState("activity history lock is poisoned".to_owned()))?;
    merge_history(&mut history, current, config, now);
    LanActivitySnapshot::new(now, history.clone()).map_err(|error| {
        PlatformError::InvalidState(format!("activity snapshot is invalid: {error}"))
    })
}

fn controller_connections() -> Result<Value, PlatformError> {
    let secret =
        storage::read_private_small_optional(MIHOMO_CONTROLLER_SECRET, 128)?.ok_or_else(|| {
            PlatformError::InvalidState("Mihomo controller secret is absent".to_owned())
        })?;
    let secret = secret.trim();
    if secret.len() != 64 || !secret.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PlatformError::InvalidState(
            "Mihomo controller secret has invalid framing".to_owned(),
        ));
    }
    let url = format!("http://{MIHOMO_CONTROLLER_ADDRESS}/connections");
    let authorization = format!("Bearer {secret}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(CONTROLLER_TIMEOUT))
        .max_redirects(0)
        .proxy(None)
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .header("Authorization", &authorization)
        .call()
        .map_err(|_| PlatformError::ProbeFailed("Mihomo connections request failed".to_owned()))?;
    let body = response
        .body_mut()
        .with_config()
        .limit(CONTROLLER_RESPONSE_LIMIT as u64)
        .read_to_string()
        .map_err(|_| PlatformError::ProbeFailed("Mihomo connections body failed".to_owned()))?;
    if body.len() > CONTROLLER_RESPONSE_LIMIT {
        return Err(PlatformError::ProbeFailed(
            "Mihomo connections response exceeds limit".to_owned(),
        ));
    }
    serde_json::from_str(&body)
        .map_err(|_| PlatformError::ProbeFailed("Mihomo connections JSON is invalid".to_owned()))
}

fn parse_connections(
    value: &Value,
    config: &DevicePolicyConfigV1,
    clients: &[LanClientObservation],
    now: u64,
) -> Result<Vec<LanActivityRecord>, PlatformError> {
    let connections = value
        .get("connections")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            PlatformError::ProbeFailed(
                "Mihomo connections response has no connection list".to_owned(),
            )
        })?;
    if connections.len() > MAX_CONTROLLER_CONNECTIONS {
        return Err(PlatformError::ProbeFailed(
            "Mihomo connections response exceeds the inspection bound".to_owned(),
        ));
    }
    let clients_by_ip = clients
        .iter()
        .filter_map(|client| client.lease_address.map(|address| (address, client.mac)))
        .collect::<HashMap<_, _>>();
    let mut records = Vec::new();
    for connection in connections {
        let Some(record) = parse_connection(connection, config, &clients_by_ip, now) else {
            continue;
        };
        records.push(record);
    }
    Ok(records)
}

fn parse_connection(
    connection: &Value,
    config: &DevicePolicyConfigV1,
    clients_by_ip: &HashMap<Ipv4Addr, LanDeviceMac>,
    now: u64,
) -> Option<LanActivityRecord> {
    let id = bounded_exact(
        connection.get("id")?.as_str()?,
        MAX_ACTIVITY_CONNECTION_ID_BYTES,
    )?;
    let metadata = connection.get("metadata")?.as_object()?;
    let source_address = metadata
        .get("sourceIP")?
        .as_str()?
        .parse::<Ipv4Addr>()
        .ok()?;
    let mac = *clients_by_ip.get(&source_address)?;
    let destination_address = metadata
        .get("destinationIP")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<Ipv4Addr>().ok());
    let target = ["host", "sniffHost"]
        .into_iter()
        .find_map(|key| {
            metadata
                .get(key)
                .and_then(Value::as_str)
                .and_then(|value| bounded_exact(value, MAX_ACTIVITY_TARGET_BYTES))
        })
        .or_else(|| destination_address.map(|address| address.to_string()))?;
    let destination_port = value_port(metadata.get("destinationPort")?)?;
    let network = match metadata
        .get("network")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "tcp" => ActivityNetwork::Tcp,
        "udp" => ActivityNetwork::Udp,
        _ => ActivityNetwork::Other,
    };
    let rule = connection
        .get("rule")
        .and_then(Value::as_str)
        .and_then(|rule| {
            let payload = connection
                .get("rulePayload")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            let combined = if payload.is_empty() {
                rule.trim().to_owned()
            } else {
                format!("{} · {payload}", rule.trim())
            };
            bounded_prefer(&combined, rule.trim(), MAX_ACTIVITY_RULE_BYTES)
        })
        .unwrap_or_else(|| "UNKNOWN".to_owned());
    let chains = connection
        .get("chains")
        .and_then(Value::as_array)
        .map(|chains| {
            chains
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|chain| bounded_exact(chain, MAX_ACTIVITY_CHAIN_NAME_BYTES))
                .take(MAX_ACTIVITY_CHAIN_ENTRIES)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let upload_bytes = connection
        .get("upload")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let download_bytes = connection
        .get("download")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    LanActivityRecord::new(
        id,
        mac,
        device_name(config, mac),
        source_address,
        target,
        destination_port,
        network,
        rule,
        chains,
        upload_bytes,
        download_bytes,
        now,
        now,
        true,
    )
    .ok()
}

fn value_port(value: &Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .or_else(|| value.as_str()?.parse::<u16>().ok())
        .filter(|port| *port > 0)
}

fn bounded_exact(value: &str, max_bytes: usize) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= max_bytes && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn bounded_prefer(value: &str, fallback: &str, max_bytes: usize) -> Option<String> {
    bounded_exact(value, max_bytes).or_else(|| bounded_exact(fallback, max_bytes))
}

fn device_name(config: &DevicePolicyConfigV1, mac: LanDeviceMac) -> String {
    config
        .entries
        .iter()
        .find(|entry| entry.mac == mac)
        .map(|entry| entry.label.as_str())
        .filter(|label| !label.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| mac.to_string())
}

fn merge_history(
    history: &mut Vec<LanActivityRecord>,
    current: Vec<LanActivityRecord>,
    config: &DevicePolicyConfigV1,
    now: u64,
) {
    for record in history.iter_mut() {
        record.active = false;
        record.device_name = device_name(config, record.mac);
    }
    for mut fresh in current {
        if let Some(existing) = history
            .iter_mut()
            .find(|existing| existing.connection_id == fresh.connection_id)
        {
            fresh.first_seen_unix_ms = existing.first_seen_unix_ms;
            *existing = fresh;
        } else {
            history.push(fresh);
        }
    }
    for record in history.iter_mut().filter(|record| !record.active) {
        record.last_seen_unix_ms = record.last_seen_unix_ms.min(now);
    }
    history.sort_by(|left, right| {
        right
            .active
            .cmp(&left.active)
            .then_with(|| right.last_seen_unix_ms.cmp(&left.last_seen_unix_ms))
            .then_with(|| left.connection_id.cmp(&right.connection_id))
    });
    history.truncate(MAX_ACTIVITY_RECORDS);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device_policy::{DevicePolicyEntry, DeviceRoutePolicy, LanDeviceLabel};

    fn config(label: &str) -> DevicePolicyConfigV1 {
        DevicePolicyConfigV1::new(
            1,
            vec![DevicePolicyEntry {
                mac: "02:00:00:00:00:10".parse().unwrap(),
                label: LanDeviceLabel::new(label).unwrap(),
                policy: DeviceRoutePolicy::Proxy,
            }],
        )
        .unwrap()
    }

    fn clients() -> Vec<LanClientObservation> {
        vec![LanClientObservation {
            mac: "02:00:00:00:00:10".parse().unwrap(),
            lease_address: Some("192.168.8.10".parse().unwrap()),
            hostname: Some("must-not-be-display-name".to_owned()),
            associated: true,
            policy: DeviceRoutePolicy::Proxy,
        }]
    }

    fn controller_fixture(id: &str, host: &str, upload: u64) -> Value {
        serde_json::json!({
            "connections": [{
                "id": id,
                "metadata": {
                    "network": "tcp",
                    "sourceIP": "192.168.8.10",
                    "destinationIP": "203.0.113.8",
                    "destinationPort": "443",
                    "host": host
                },
                "upload": upload,
                "download": 44,
                "chains": ["新加坡", "HYZ-PROXY"],
                "rule": "MATCH",
                "rulePayload": "HYZ-PROXY"
            }, {
                "id": "foreign",
                "metadata": {
                    "network": "udp",
                    "sourceIP": "192.168.8.99",
                    "destinationIP": "198.51.100.9",
                    "destinationPort": 443,
                    "host": "foreign.example"
                }
            }]
        })
    }

    fn sniff_host_fixture() -> Value {
        serde_json::json!({
            "connections": [{
                "id": "c-sniff",
                "metadata": {
                    "network": "tcp",
                    "sourceIP": "192.168.8.10",
                    "destinationIP": "203.0.113.8",
                    "destinationPort": "443",
                    "host": "",
                    "sniffHost": "sniffed.example"
                },
                "upload": 1,
                "download": 2,
                "chains": ["HYZ-PROXY"],
                "rule": "MATCH"
            }]
        })
    }

    #[test]
    fn parser_uses_sniff_host_when_host_is_empty() {
        let records =
            parse_connections(&sniff_host_fixture(), &config(""), &clients(), 100).unwrap();
        assert_eq!(records[0].target, "sniffed.example");
    }

    #[test]
    fn parser_maps_source_ip_to_mac_and_uses_custom_label_without_hostname_fallback() {
        let records = parse_connections(
            &controller_fixture("c-1", "example.com", 12),
            &config("我的手机"),
            &clients(),
            100,
        )
        .unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].mac.to_string(), "02:00:00:00:00:10");
        assert_eq!(records[0].device_name, "我的手机");
        assert_eq!(records[0].target, "example.com");
        assert_eq!(records[0].rule, "MATCH · HYZ-PROXY");
        assert_eq!(records[0].chains, ["新加坡", "HYZ-PROXY"]);

        let records = parse_connections(
            &controller_fixture("c-1", "example.com", 12),
            &config(""),
            &clients(),
            100,
        )
        .unwrap();
        assert_eq!(records[0].device_name, "02:00:00:00:00:10");
        assert_ne!(records[0].device_name, "must-not-be-display-name");
    }

    #[test]
    fn bounded_history_preserves_first_seen_marks_closed_and_refreshes_labels() {
        let mut history = Vec::new();
        let first = parse_connections(
            &controller_fixture("c-1", "example.com", 1),
            &config("旧名字"),
            &clients(),
            100,
        )
        .unwrap();
        merge_history(&mut history, first, &config("旧名字"), 100);
        let second = parse_connections(
            &controller_fixture("c-1", "example.com", 9),
            &config("新名字"),
            &clients(),
            200,
        )
        .unwrap();
        merge_history(&mut history, second, &config("新名字"), 200);
        assert_eq!(history[0].first_seen_unix_ms, 100);
        assert_eq!(history[0].last_seen_unix_ms, 200);
        assert_eq!(history[0].upload_bytes, 9);
        assert_eq!(history[0].device_name, "新名字");
        assert!(history[0].active);

        merge_history(&mut history, Vec::new(), &config("最终名字"), 300);
        assert!(!history[0].active);
        assert_eq!(history[0].device_name, "最终名字");
        assert_eq!(history[0].last_seen_unix_ms, 200);
    }
}
