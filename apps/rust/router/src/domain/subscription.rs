use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::collections::HashSet;
use std::error::Error;
use std::fmt;
#[cfg(feature = "native")]
use std::io::{self, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::{Host, Url};
use zeroize::Zeroizing;

pub const MAX_SUBSCRIPTION_URL_BYTES: usize = 2_048;
pub const MAX_SUBSCRIPTION_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PROXY_NODES: usize = 1_024;
pub const MAX_YAML_NODES: usize = 32_768;
pub const MAX_YAML_DEPTH: usize = 64;
pub const MAX_YAML_SCALAR_BYTES: usize = 256 * 1024;
pub const MAX_GENERATION_ID_BYTES: usize = 64;

/// A subscription URL whose secret representation cannot be read back as a string.
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
    #[cfg(feature = "native")]
    pub(crate) fn write_secret(&self, writer: &mut impl Write) -> io::Result<()> {
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct GenerationId(String);

impl GenerationId {
    pub fn parse(value: impl Into<String>) -> Result<Self, SubscriptionError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_GENERATION_ID_BYTES
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(SubscriptionError::InvalidGeneration);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubscriptionStatus {
    Idle,
    Fetching,
    Active(GenerationId),
    Failed(String),
}

impl SubscriptionStatus {
    pub fn failed(message: impl Into<String>) -> Result<Self, SubscriptionError> {
        let message = message.into();
        if message.is_empty() || message.len() > 1_024 || message.chars().any(char::is_control) {
            return Err(SubscriptionError::InvalidStatus);
        }
        Ok(Self::Failed(message))
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

impl SubscriptionSummary {
    pub fn from_status(configured: bool, status: Option<&SubscriptionStatus>) -> Self {
        let state = match status {
            None | Some(SubscriptionStatus::Idle) => SubscriptionSummaryState::Idle,
            Some(SubscriptionStatus::Fetching) => SubscriptionSummaryState::Fetching,
            Some(SubscriptionStatus::Active(_)) => SubscriptionSummaryState::Active,
            Some(SubscriptionStatus::Failed(_)) => SubscriptionSummaryState::Failed,
        };
        Self { configured, state }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedSubscription {
    yaml: Vec<u8>,
    proxy_count: usize,
}

impl ValidatedSubscription {
    pub fn as_bytes(&self) -> &[u8] {
        &self.yaml
    }

    pub const fn proxy_count(&self) -> usize {
        self.proxy_count
    }
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

pub fn parse_mihomo_subscription(input: &[u8]) -> Result<ValidatedSubscription, SubscriptionError> {
    if input.is_empty() || input.len() > MAX_SUBSCRIPTION_BYTES {
        return Err(SubscriptionError::DocumentSize);
    }
    reject_yaml_references(input)?;

    let document: Value = serde_yaml::from_slice(input)
        .map_err(|error| SubscriptionError::InvalidYaml(error.to_string()))?;
    let mut node_count = 0;
    validate_value_limits(&document, 1, &mut node_count)?;

    let top = document
        .as_mapping()
        .ok_or(SubscriptionError::InvalidShape(
            "top level must be a mapping",
        ))?;
    let proxies = top
        .get(Value::String("proxies".to_owned()))
        .and_then(Value::as_sequence)
        .ok_or(SubscriptionError::InvalidShape(
            "proxies must be a non-empty array",
        ))?;
    if proxies.is_empty() || proxies.len() > MAX_PROXY_NODES {
        return Err(SubscriptionError::ProxyCount);
    }

    let mut names = HashSet::with_capacity(proxies.len());
    for proxy in proxies {
        let mapping = proxy.as_mapping().ok_or(SubscriptionError::InvalidShape(
            "each proxy must be a mapping",
        ))?;
        let name = mapping
            .get(Value::String("name".to_owned()))
            .and_then(Value::as_str)
            .ok_or(SubscriptionError::InvalidShape(
                "each proxy must have a string name",
            ))?;
        if name.is_empty() || !names.insert(name.to_owned()) {
            return Err(SubscriptionError::DuplicateProxyName);
        }
    }

    let mut output = Mapping::new();
    output.insert(
        Value::String("proxies".to_owned()),
        Value::Sequence(proxies.clone()),
    );
    let yaml = serde_yaml::to_string(&output)
        .map_err(|error| SubscriptionError::InvalidYaml(error.to_string()))?
        .into_bytes();
    Ok(ValidatedSubscription {
        yaml,
        proxy_count: proxies.len(),
    })
}

fn validate_value_limits(
    value: &Value,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), SubscriptionError> {
    *nodes += 1;
    if *nodes > MAX_YAML_NODES {
        return Err(SubscriptionError::NodeLimit);
    }
    if depth > MAX_YAML_DEPTH {
        return Err(SubscriptionError::DepthLimit);
    }
    match value {
        Value::String(value) if value.len() > MAX_YAML_SCALAR_BYTES => {
            Err(SubscriptionError::ScalarLimit)
        }
        Value::Sequence(values) => {
            for value in values {
                validate_value_limits(value, depth + 1, nodes)?;
            }
            Ok(())
        }
        Value::Mapping(values) => {
            for (key, value) in values {
                validate_value_limits(key, depth + 1, nodes)?;
                validate_value_limits(value, depth + 1, nodes)?;
            }
            Ok(())
        }
        Value::Tagged(_) => Err(SubscriptionError::YamlReference),
        _ => Ok(()),
    }
}

fn reject_yaml_references(input: &[u8]) -> Result<(), SubscriptionError> {
    let text = std::str::from_utf8(input)
        .map_err(|_| SubscriptionError::InvalidYaml("document is not UTF-8".to_owned()))?;
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut escaped = false;
    let mut comment = false;
    let mut token_start = true;

    for character in text.chars() {
        if comment {
            if matches!(character, '\n' | '\r') {
                comment = false;
                token_start = true;
            }
            continue;
        }
        if double_quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                double_quoted = false;
            }
            continue;
        }
        if single_quoted {
            if character == '\'' {
                single_quoted = false;
            }
            continue;
        }
        match character {
            '#' if token_start => {
                comment = true;
            }
            '\n' | '\r' => token_start = true,
            '\'' => {
                single_quoted = true;
                token_start = false;
            }
            '"' => {
                double_quoted = true;
                token_start = false;
            }
            '!' | '&' | '*' if token_start => return Err(SubscriptionError::YamlReference),
            character if character.is_whitespace() => token_start = true,
            '[' | ']' | '{' | '}' | ',' => token_start = true,
            '-' | '?' | ':' if token_start => token_start = true,
            _ => token_start = false,
        }
    }
    Ok(())
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

    #[test]
    fn parser_extracts_only_proxies_from_full_config() {
        let parsed = parse_mihomo_subscription(
            b"mixed-port: 7890\nexternal-controller: 0.0.0.0:9090\nproxies:\n  - name: node-a\n    type: ss\n    server: 1.1.1.1\nproxy-groups:\n  - name: unsafe-group\n    type: select\n    proxies: [node-a]\nrules: [MATCH,node-a]\n",
        )
        .unwrap();
        assert_eq!(parsed.proxy_count(), 1);
        let output: Value = serde_yaml::from_slice(parsed.as_bytes()).unwrap();
        let output = output.as_mapping().unwrap();
        assert_eq!(output.len(), 1);
        assert!(output.contains_key(Value::String("proxies".to_owned())));
        assert!(!output.contains_key(Value::String("external-controller".to_owned())));
        assert!(!output.contains_key(Value::String("proxy-groups".to_owned())));
        assert!(!output.contains_key(Value::String("rules".to_owned())));
    }

    #[test]
    fn parser_rejects_shape_duplicates_and_yaml_features() {
        for input in [
            "proxies: []\n",
            "proxies:\n  - name: a\n  - name: a\n",
            "proxies:\n  - name: &name a\n",
            "proxies:\n  - name: !custom a\n",
            "proxies:\n  - name: a\n    name: b\n",
            "proxies:\n  - name: a\nproxies:\n  - name: b\n",
        ] {
            assert!(
                parse_mihomo_subscription(input.as_bytes()).is_err(),
                "{input}"
            );
        }
    }

    #[test]
    fn parser_enforces_resource_limits() {
        assert!(parse_mihomo_subscription(&vec![b'a'; MAX_SUBSCRIPTION_BYTES + 1]).is_err());

        let scalar = "a".repeat(MAX_YAML_SCALAR_BYTES + 1);
        let document = format!("proxies:\n  - name: node\n    password: {scalar}\n");
        assert_eq!(
            parse_mihomo_subscription(document.as_bytes()),
            Err(SubscriptionError::ScalarLimit)
        );

        let nested = format!(
            "{}{}{}",
            "proxies:\n  - name: node\n    nested: ",
            "[".repeat(MAX_YAML_DEPTH + 1),
            "]".repeat(MAX_YAML_DEPTH + 1)
        );
        assert!(matches!(
            parse_mihomo_subscription(nested.as_bytes()),
            Err(SubscriptionError::DepthLimit) | Err(SubscriptionError::InvalidYaml(_))
        ));
    }

    #[test]
    fn generation_rejects_traversal() {
        assert!(GenerationId::parse("generation_01").is_ok());
        assert!(GenerationId::parse("../current").is_err());
        assert!(GenerationId::parse("a/b").is_err());
    }
}
