//! Subscription wire DTOs: write-only URL validation and public summaries.

use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::{Host, Url};
use zeroize::Zeroizing;

pub const MAX_SUBSCRIPTION_URL_BYTES: usize = 2_048;

/// A subscription URL whose secret representation cannot be read back as a
/// string. The value is written to sinks via [`SubscriptionUrl::write_secret`].
pub struct SubscriptionUrl(Zeroizing<String>);

impl SubscriptionUrl {
    pub fn parse(value: String) -> Result<Self, SubscriptionError> {
        let value = Zeroizing::new(value);
        Self::validate(&value)?;
        Ok(Self(value))
    }

    pub fn validate(value: &str) -> Result<(), SubscriptionError> {
        if value.is_empty() || value.len() > MAX_SUBSCRIPTION_URL_BYTES {
            return Err(SubscriptionError::InvalidUrl(
                "URL length is outside the allowed range",
            ));
        }
        if value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
        {
            return Err(SubscriptionError::InvalidUrl(
                "URL contains whitespace or control characters",
            ));
        }

        let parsed = Url::parse(value)
            .map_err(|_| SubscriptionError::InvalidUrl("URL syntax is invalid"))?;
        if parsed.scheme() != "https" {
            return Err(SubscriptionError::InvalidUrl("URL must use HTTPS"));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(SubscriptionError::InvalidUrl("URL userinfo is forbidden"));
        }
        if parsed.fragment().is_some() {
            return Err(SubscriptionError::InvalidUrl("URL fragment is forbidden"));
        }
        match parsed.host() {
            Some(Host::Ipv4(address)) if !is_public_ip(IpAddr::V4(address)) => {
                return Err(SubscriptionError::NonPublicAddress)
            }
            Some(Host::Ipv6(address)) if !is_public_ip(IpAddr::V6(address)) => {
                return Err(SubscriptionError::NonPublicAddress)
            }
            Some(_) => {}
            None => return Err(SubscriptionError::InvalidUrl("URL host is required")),
        }

        Ok(())
    }

    /// Writes the URL directly to a sink without exposing a borrow or owned copy.
    pub fn write_secret(&self, writer: &mut impl Write) -> io::Result<()> {
        writer.write_all(self.0.as_bytes())
    }
}

impl fmt::Debug for SubscriptionUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let _secret_length = self.0.len();
        formatter.write_str("SubscriptionUrl([REDACTED])")
    }
}

impl fmt::Display for SubscriptionUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionSummaryState {
    Idle,
    Fetching,
    Active,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionSummary {
    pub configured: bool,
    pub state: SubscriptionSummaryState,
}

pub fn validate_dns_results(addresses: &[IpAddr]) -> Result<(), SubscriptionError> {
    if addresses.is_empty() {
        return Err(SubscriptionError::EmptyDnsResult);
    }
    if addresses.iter().copied().all(is_public_ip) {
        Ok(())
    } else {
        Err(SubscriptionError::NonPublicAddress)
    }
}

pub fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => is_public_ipv6(address),
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let value = u32::from(address);
    let in_prefix = |network: [u8; 4], bits: u32| {
        let mask = if bits == 0 {
            0
        } else {
            u32::MAX << (32 - bits)
        };
        value & mask == u32::from(Ipv4Addr::from(network)) & mask
    };

    !in_prefix([0, 0, 0, 0], 8)
        && !in_prefix([10, 0, 0, 0], 8)
        && !in_prefix([100, 64, 0, 0], 10)
        && !in_prefix([127, 0, 0, 0], 8)
        && !in_prefix([169, 254, 0, 0], 16)
        && !in_prefix([172, 16, 0, 0], 12)
        && !in_prefix([192, 0, 0, 0], 24)
        && !in_prefix([192, 0, 2, 0], 24)
        && !in_prefix([192, 88, 99, 0], 24)
        && !in_prefix([192, 168, 0, 0], 16)
        && !in_prefix([198, 18, 0, 0], 15)
        && !in_prefix([198, 51, 100, 0], 24)
        && !in_prefix([203, 0, 113, 0], 24)
        && !in_prefix([224, 0, 0, 0], 4)
        && !in_prefix([240, 0, 0, 0], 4)
}

fn is_public_ipv6(address: Ipv6Addr) -> bool {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_public_ipv4(mapped);
    }
    let value = u128::from(address);
    let in_prefix = |network: Ipv6Addr, bits: u32| {
        let mask = if bits == 0 {
            0
        } else {
            u128::MAX << (128 - bits)
        };
        value & mask == u128::from(network) & mask
    };

    // Only global-unicast space is eligible, with special-use ranges removed.
    in_prefix(Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0), 3)
        && !in_prefix(Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0), 23)
        && !in_prefix(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0), 32)
        && !in_prefix(Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0), 20)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubscriptionError {
    InvalidUrl(&'static str),
    NonPublicAddress,
    EmptyDnsResult,
    DocumentSize,
    InvalidYaml(String),
    YamlReference,
    NodeLimit,
    DepthLimit,
    ScalarLimit,
    ProxyCount,
    InvalidShape(&'static str),
    DuplicateProxyName,
    InvalidGeneration,
    InvalidStatus,
}

impl fmt::Display for SubscriptionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl(reason) => write!(formatter, "invalid subscription URL: {reason}"),
            Self::NonPublicAddress => formatter.write_str("address is not globally routable"),
            Self::EmptyDnsResult => formatter.write_str("DNS result is empty"),
            Self::DocumentSize => formatter.write_str("subscription exceeds document size limit"),
            Self::InvalidYaml(reason) => write!(formatter, "invalid subscription YAML: {reason}"),
            Self::YamlReference => {
                formatter.write_str("YAML aliases, anchors, and tags are forbidden")
            }
            Self::NodeLimit => formatter.write_str("subscription exceeds YAML node limit"),
            Self::DepthLimit => formatter.write_str("subscription exceeds YAML depth limit"),
            Self::ScalarLimit => formatter.write_str("subscription exceeds YAML scalar limit"),
            Self::ProxyCount => {
                formatter.write_str("subscription proxy count is outside the allowed range")
            }
            Self::InvalidShape(reason) => write!(formatter, "invalid subscription shape: {reason}"),
            Self::DuplicateProxyName => {
                formatter.write_str("proxy names must be non-empty and unique")
            }
            Self::InvalidGeneration => formatter.write_str("invalid subscription generation ID"),
            Self::InvalidStatus => formatter.write_str("invalid subscription status"),
        }
    }
}

impl Error for SubscriptionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_is_https_write_only_and_redacted() {
        let url =
            SubscriptionUrl::parse("https://example.com/sub?token=secret".to_owned()).unwrap();
        assert_eq!(url.to_string(), "[REDACTED]");
        assert_eq!(format!("{url:?}"), "SubscriptionUrl([REDACTED])");
        let mut bytes = Vec::new();
        url.write_secret(&mut bytes).unwrap();
        assert_eq!(bytes, b"https://example.com/sub?token=secret");
    }

    #[test]
    fn url_rejects_unsafe_components_and_addresses() {
        for value in [
            "http://example.com/sub",
            "https://user@example.com/sub",
            "https://example.com/sub#fragment",
            "https://example.com/a b",
            "https://127.0.0.1/sub",
            "https://100.64.0.1/sub",
            "https://[::1]/sub",
            "https://[2001:db8::1]/sub",
        ] {
            assert!(SubscriptionUrl::parse(value.to_owned()).is_err(), "{value}");
        }
    }

    #[test]
    fn dns_requires_all_results_to_be_public() {
        assert!(validate_dns_results(&[]).is_err());
        assert!(validate_dns_results(&[
            "1.1.1.1".parse().unwrap(),
            "2606:4700:4700::1111".parse().unwrap(),
        ])
        .is_ok());
        assert!(validate_dns_results(&[
            "1.1.1.1".parse().unwrap(),
            "169.254.1.1".parse().unwrap(),
        ])
        .is_err());
    }
}
