//! The `hyz-router` root-only Unix control protocol envelope.
//!
//! Versioned typed JSON frames over a root-only Unix domain socket. The server
//! side (daemon ownership, accept loop, peer credentials) lives in the router
//! package; this module defines the wire types, the client transport
//! ([`crate::client`], `client` feature) and the shared semantic validation.

use serde::{Deserialize, Serialize};

use super::{
    admin::SecretString,
    device_policy::{DevicePolicySnapshot, DevicePolicyUpdateRequest},
    dhcp::{DhcpEvent, DhcpTransition},
    network_config::{NetworkConfigSummary, PendingNetworkConfigSummary},
    ota::OtaCommand,
    panel::{
        valid_control_name, DisplayRequest, PanelSnapshot, ProxyDelayRefreshRequest,
        ProxyDelayRequest, ProxyDelayResult, ProxyGroup, ProxySelectionRequest,
    },
    status::{StatusSnapshot, TailscaleStatus},
    subscription::{SubscriptionSummary, SubscriptionUrl},
    tailscale::{TailscaleLoginUrl, TailscaleMode, TailscalePeerSnapshot},
    wifi::{ApPrepareRequest, StaCandidateRequest, WifiScanEntry},
};

pub const PROTOCOL_VERSION: u16 = 13;
pub const CONTROL_SOCKET: &str = "/run/hyz-router/control.sock";
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
pub const IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
pub const CLIENT_OPERATION_WAIT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    pub version: u16,
    pub operation: ControlOperation,
}

impl ControlRequest {
    pub const fn new(operation: ControlOperation) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            operation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlOperation {
    Status {},
    PanelStatus {},
    Display { request: DisplayRequest },
    Router { enabled: bool },
    Proxy { mode: ControlProxyMode },
    ProxyLanTun { enabled: bool },
    ProxyLocalSystem { enabled: bool },
    ProxySelection { request: ProxySelectionRequest },
    ProxyDelay { request: ProxyDelayRequest },
    ProxyDelayRefresh { request: ProxyDelayRefreshRequest },
    SubscriptionGet {},
    SubscriptionSet { url: SecretString },
    SubscriptionRefresh {},
    DevicePoliciesGet {},
    DevicePoliciesSet { request: DevicePolicyUpdateRequest },
    TailscaleGet {},
    TailscalePeersGet {},
    TailscaleMode { mode: TailscaleMode },
    TailscaleLogin {},
    TailscaleLogout {},
    Ota { command: OtaCommand },
    Dhcp { event: DhcpEvent },
    WifiStatus {},
    WifiPending {},
    WifiScan {},
    WifiStaApply { request: StaCandidateRequest },
    WifiApPrepare { request: ApPrepareRequest },
    WifiApApply {},
    WifiApConfirm {},
    WifiApCancel {},
}

impl ControlOperation {
    pub const fn mutates(&self) -> bool {
        !matches!(
            self,
            Self::Status { .. }
                | Self::PanelStatus { .. }
                | Self::WifiStatus { .. }
                | Self::WifiPending { .. }
                | Self::WifiScan { .. }
                | Self::SubscriptionGet { .. }
                | Self::DevicePoliciesGet { .. }
                | Self::TailscaleGet { .. }
                | Self::TailscalePeersGet { .. }
                | Self::Ota {
                    command: OtaCommand::Verify { .. }
                }
        )
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Status { .. }
            | Self::PanelStatus { .. }
            | Self::ProxyDelayRefresh { .. }
            | Self::SubscriptionGet { .. }
            | Self::SubscriptionRefresh { .. }
            | Self::DevicePoliciesGet { .. }
            | Self::TailscaleGet { .. }
            | Self::TailscalePeersGet { .. }
            | Self::TailscaleMode { .. }
            | Self::TailscaleLogin { .. }
            | Self::TailscaleLogout { .. }
            | Self::Router { .. }
            | Self::Proxy { .. }
            | Self::ProxyLanTun { .. }
            | Self::ProxyLocalSystem { .. }
            | Self::WifiStatus { .. }
            | Self::WifiPending { .. }
            | Self::WifiScan { .. }
            | Self::WifiStaApply { .. }
            | Self::WifiApPrepare { .. }
            | Self::WifiApApply { .. }
            | Self::WifiApConfirm { .. }
            | Self::WifiApCancel { .. } => Ok(()),
            Self::DevicePoliciesSet { request } => request
                .clone()
                .candidate()
                .map(|_| ())
                .map_err(|_| "device policy request is invalid"),
            Self::SubscriptionSet { url } => SubscriptionUrl::validate(url.expose())
                .map_err(|_| "subscription URL must be a safe public HTTPS URL"),
            Self::Display { request } => validate_display(request),
            Self::ProxySelection { request } => {
                if valid_control_name(&request.group) && valid_control_name(&request.proxy) {
                    Ok(())
                } else {
                    Err("proxy selection names are invalid")
                }
            }
            Self::ProxyDelay { request } => {
                if valid_control_name(&request.proxy) {
                    Ok(())
                } else {
                    Err("proxy delay name is invalid")
                }
            }
            Self::Ota { command } => validate_ota(command),
            Self::Dhcp { event } => validate_dhcp(event),
        }
    }
}

fn validate_display(request: &DisplayRequest) -> Result<(), &'static str> {
    match (request.enabled, request.brightness) {
        (true, None) | (true, Some(1..=u16::MAX)) | (false, None) => Ok(()),
        (true, Some(0)) => Err("enabled display brightness must be non-zero"),
        (false, Some(_)) => Err("disabled display request may not include brightness"),
    }
}

fn validate_ota(command: &OtaCommand) -> Result<(), &'static str> {
    match command {
        OtaCommand::Verify { firmware, expected }
        | OtaCommand::Install {
            firmware, expected, ..
        }
        | OtaCommand::InstallRecovery {
            firmware, expected, ..
        } => {
            validate_path(firmware)?;
            validate_digest(expected)
        }
        OtaCommand::Download { source, expected }
        | OtaCommand::Apply {
            source, expected, ..
        } => {
            validate_source(source)?;
            validate_digest(expected)
        }
    }
}

fn validate_path(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.len() > 4096
        || !value.starts_with('/')
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        Err("firmware path must be an absolute bounded path without control characters")
    } else {
        Ok(())
    }
}

fn validate_source(value: &str) -> Result<(), &'static str> {
    let remainder = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"));
    let authority = remainder
        .and_then(|remainder| remainder.split(['/', '?', '#']).next())
        .unwrap_or_default();
    if value.len() > 2048
        || authority.is_empty()
        || authority.contains('@')
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        Err("firmware source must be a bounded HTTP(S) URL with a host and no userinfo or whitespace")
    } else {
        Ok(())
    }
}

fn validate_digest(value: &str) -> Result<(), &'static str> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("firmware digest must be exactly 64 hexadecimal characters")
    }
}

fn validate_dhcp(event: &DhcpEvent) -> Result<(), &'static str> {
    let DhcpTransition::Lease { lease } = &event.transition else {
        return Ok(());
    };
    if lease.prefix > 32
        || (lease.routers.is_empty() && lease.static_routes.is_empty())
        || lease.routers.len() > 16
        || lease.static_routes.len() > 64
        || lease.dns.len() > 16
        || lease.search.len() > 16
    {
        return Err("DHCP lease exceeds semantic count or prefix limits");
    }
    for (destination, _) in &lease.static_routes {
        if destination.len() > 64 || !valid_route_destination(destination) {
            return Err("DHCP static route destination is invalid");
        }
    }
    if lease.search.iter().any(|domain| {
        domain.is_empty()
            || domain.len() > 253
            || !domain
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    }) {
        return Err("DHCP search domain is invalid");
    }
    Ok(())
}

fn valid_route_destination(value: &str) -> bool {
    if value == "default" {
        return true;
    }
    let Some((address, prefix)) = value.split_once('/') else {
        return false;
    };
    address.parse::<std::net::Ipv4Addr>().is_ok()
        && prefix.parse::<u8>().ok().is_some_and(|prefix| prefix <= 32)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlProxyMode {
    Explicit,
    Tun,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlResponse {
    pub version: u16,
    pub result: Result<ControlResult, ControlError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ControlResult {
    Status {
        snapshot: Box<StatusSnapshot>,
    },
    PanelStatus {
        snapshot: Box<PanelSnapshot>,
    },
    ProxyDelay {
        result: ProxyDelayResult,
    },
    ProxyDelays {
        groups: Vec<ProxyGroup>,
    },
    WifiConfig {
        config: NetworkConfigSummary,
    },
    WifiPending {
        pending: PendingNetworkConfigSummary,
    },
    WifiPendingStatus {
        pending: Option<PendingNetworkConfigSummary>,
        applied: bool,
        remaining_seconds: Option<u64>,
    },
    WifiScan {
        entries: Vec<WifiScanEntry>,
    },
    DevicePolicies {
        snapshot: DevicePolicySnapshot,
    },
    Subscription {
        summary: SubscriptionSummary,
    },
    Tailscale {
        status: TailscaleStatus,
    },
    TailscalePeers {
        snapshot: TailscalePeerSnapshot,
    },
    TailscaleMutation {
        status: TailscaleStatus,
        #[serde(skip_serializing_if = "Option::is_none")]
        login_url: Option<TailscaleLoginUrl>,
    },
    Completed {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlError {
    pub code: String,
    pub message: String,
}

impl ControlResponse {
    pub fn success(result: ControlResult) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            result: Ok(result),
        }
    }

    pub fn error(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            result: Err(ControlError {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        device_policy::DevicePolicyUpdateRequest,
        tailscale::{TailscalePeer, TailscalePeerSnapshot, MAX_TAILSCALE_PEERS},
    };

    #[test]
    fn protocol_rejects_unknown_fields_and_is_bounded_by_version() {
        let unknown = br#"{"version":1,"operation":{"op":"status","extra":true}}"#;
        assert!(serde_json::from_slice::<ControlRequest>(unknown).is_err());
        let request = ControlRequest::new(ControlOperation::Status {});
        let encoded = serde_json::to_vec(&request).unwrap();
        assert!(encoded.len() < MAX_FRAME_BYTES);
        assert_eq!(PROTOCOL_VERSION, 13);
        let policy = ControlOperation::DevicePoliciesSet {
            request: DevicePolicyUpdateRequest {
                expected_generation: 0,
                entries: Vec::new(),
            },
        };
        assert!(policy.mutates());
        assert!(policy.validate().is_ok());
        let tailscale_get = ControlOperation::TailscaleGet {};
        assert!(!tailscale_get.mutates());
        assert!(tailscale_get.validate().is_ok());
        let tailscale_peers_get = ControlOperation::TailscalePeersGet {};
        assert!(!tailscale_peers_get.mutates());
        assert!(tailscale_peers_get.validate().is_ok());
        let peers = (0..MAX_TAILSCALE_PEERS)
            .map(|index| {
                TailscalePeer::new(
                    format!("peer-{index:03}"),
                    std::net::Ipv4Addr::new(100, 64 + (index / 256) as u8, 0, index as u8),
                    index % 2 == 0,
                    Some("linux".to_owned()),
                )
                .unwrap()
            })
            .collect();
        let snapshot = TailscalePeerSnapshot::new(peers).unwrap();
        let encoded_peers =
            serde_json::to_vec(&ControlResponse::success(ControlResult::TailscalePeers {
                snapshot,
            }))
            .unwrap();
        assert!(encoded_peers.len() < MAX_FRAME_BYTES);
        let encoded_peers = String::from_utf8(encoded_peers).unwrap();
        for secret in [
            "node_key",
            "machine_key",
            "public_key",
            "endpoints",
            "auth_key",
        ] {
            assert!(!encoded_peers.contains(secret));
        }
        for operation in [
            ControlOperation::TailscaleMode {
                mode: TailscaleMode::Disabled,
            },
            ControlOperation::TailscaleLogin {},
            ControlOperation::TailscaleLogout {},
        ] {
            assert!(operation.mutates());
            assert!(operation.validate().is_ok());
            let json = serde_json::to_string(&ControlRequest::new(operation)).unwrap();
            assert!(!json.contains("url"));
            assert!(!json.contains("auth_key"));
            assert!(!json.contains("subnet"));
            assert!(!json.contains("port"));
        }
        assert_eq!(
            serde_json::from_slice::<ControlRequest>(&encoded).unwrap(),
            request
        );
    }

    #[test]
    fn local_system_proxy_operation_is_typed_and_validated() {
        let operation = serde_json::from_str::<ControlOperation>(
            r#"{"op":"proxy_local_system","enabled":true}"#,
        )
        .expect("local system proxy operation must decode");
        assert!(operation.mutates());
        assert!(operation.validate().is_ok());
    }

    #[test]
    fn only_verify_is_a_read_only_ota_operation() {
        assert!(!ControlOperation::Status {}.mutates());
        assert!(!ControlOperation::Ota {
            command: OtaCommand::Verify {
                firmware: "/tmp/x".to_owned(),
                expected: "0".repeat(64),
            }
        }
        .mutates());
        assert!(ControlOperation::Router { enabled: true }.mutates());
    }

    #[test]
    fn semantic_validation_bounds_ota_and_dhcp_payloads() {
        assert!(ControlOperation::Ota {
            command: OtaCommand::Verify {
                firmware: "relative.fw".to_owned(),
                expected: "0".repeat(64),
            }
        }
        .validate()
        .is_err());
        assert!(ControlOperation::Ota {
            command: OtaCommand::Download {
                source: "file:///tmp/update.fw".to_owned(),
                expected: "not-a-digest".to_owned(),
            }
        }
        .validate()
        .is_err());

        let lease = crate::dhcp::DhcpLease {
            address: "192.0.2.10".parse().unwrap(),
            prefix: 24,
            broadcast: None,
            routers: vec!["192.0.2.1".parse().unwrap(); 17],
            static_routes: Vec::new(),
            dns: Vec::new(),
            search: Vec::new(),
        };
        assert!(ControlOperation::Dhcp {
            event: crate::dhcp::DhcpEvent::new(
                crate::dhcp::DhcpUplink::Wifi,
                crate::dhcp::DhcpGeneration::new("test-generation".to_owned()).unwrap(),
                DhcpTransition::Lease { lease },
            )
        }
        .validate()
        .is_err());
    }
}
