use serde_yaml::{Mapping, Value};
use std::collections::HashSet;

pub use hyz_contract::subscription::{
    is_public_ip, validate_dns_results, SubscriptionError, SubscriptionSummary,
    SubscriptionSummaryState, SubscriptionUrl, MAX_SUBSCRIPTION_URL_BYTES,
};

pub const MAX_SUBSCRIPTION_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PROXY_NODES: usize = 1_024;
pub const MAX_YAML_NODES: usize = 32_768;
pub const MAX_YAML_DEPTH: usize = 64;
pub const MAX_YAML_SCALAR_BYTES: usize = 256 * 1024;
pub const MAX_GENERATION_ID_BYTES: usize = 64;
pub const HYZ_PROXY_GROUP: &str = "HYZ-PROXY";

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

pub fn summary_from_status(
    configured: bool,
    status: Option<&SubscriptionStatus>,
) -> SubscriptionSummary {
    let state = match status {
        None | Some(SubscriptionStatus::Idle) => SubscriptionSummaryState::Idle,
        Some(SubscriptionStatus::Fetching) => SubscriptionSummaryState::Fetching,
        Some(SubscriptionStatus::Active(_)) => SubscriptionSummaryState::Active,
        Some(SubscriptionStatus::Failed(_)) => SubscriptionSummaryState::Failed,
    };
    SubscriptionSummary { configured, state }
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

    let names = proxy_names(proxies)?;
    if names.iter().any(|name| name == HYZ_PROXY_GROUP) {
        return Err(SubscriptionError::InvalidShape(
            "proxy name conflicts with the reserved HYZ group",
        ));
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

/// Compose the trusted proxy inventory with product-owned routing policy while preserving
/// unrelated local source fields. Product policy and GeoData mode always override source values.
pub fn compose_managed_mihomo_source(
    current_source: &[u8],
    subscription: &ValidatedSubscription,
) -> Result<Vec<u8>, SubscriptionError> {
    let mut source: Value = serde_yaml::from_slice(current_source)
        .map_err(|error| SubscriptionError::InvalidYaml(error.to_string()))?;
    let top = source
        .as_mapping_mut()
        .ok_or(SubscriptionError::InvalidShape(
            "Mihomo source top level must be a mapping",
        ))?;

    let provider: Value = serde_yaml::from_slice(subscription.as_bytes())
        .map_err(|error| SubscriptionError::InvalidYaml(error.to_string()))?;
    let proxies = provider
        .as_mapping()
        .and_then(|mapping| mapping.get(Value::String("proxies".to_owned())))
        .and_then(Value::as_sequence)
        .ok_or(SubscriptionError::InvalidShape(
            "validated subscription has no proxies",
        ))?;
    let names = proxy_names(proxies)?;

    top.insert(
        Value::String("proxies".to_owned()),
        Value::Sequence(proxies.clone()),
    );
    top.insert(
        Value::String("mode".to_owned()),
        Value::String("rule".to_owned()),
    );
    top.insert(Value::String("geodata-mode".to_owned()), Value::Bool(false));
    top.insert(
        Value::String("geo-auto-update".to_owned()),
        Value::Bool(false),
    );
    top.insert(
        Value::String("proxy-groups".to_owned()),
        Value::Sequence(vec![managed_selector_group(&names)]),
    );
    top.insert(
        Value::String("rules".to_owned()),
        Value::Sequence(
            [
                "GEOSITE,cn,DIRECT",
                "GEOIP,CN,DIRECT,no-resolve",
                "MATCH,HYZ-PROXY",
            ]
            .into_iter()
            .map(|rule| Value::String(rule.to_owned()))
            .collect(),
        ),
    );

    serde_yaml::to_string(&source)
        .map(|yaml| yaml.into_bytes())
        .map_err(|error| SubscriptionError::InvalidYaml(error.to_string()))
}

fn proxy_names(proxies: &[Value]) -> Result<Vec<String>, SubscriptionError> {
    let mut unique = HashSet::with_capacity(proxies.len());
    let mut names = Vec::with_capacity(proxies.len());
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
        if name.is_empty() || !unique.insert(name.to_owned()) {
            return Err(SubscriptionError::DuplicateProxyName);
        }
        names.push(name.to_owned());
    }
    Ok(names)
}

fn managed_selector_group(names: &[String]) -> Value {
    let mut group = Mapping::new();
    group.insert(
        Value::String("name".to_owned()),
        Value::String(HYZ_PROXY_GROUP.to_owned()),
    );
    group.insert(
        Value::String("type".to_owned()),
        Value::String("select".to_owned()),
    );
    group.insert(
        Value::String("proxies".to_owned()),
        Value::Sequence(names.iter().cloned().map(Value::String).collect()),
    );
    Value::Mapping(group)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> Value {
        Value::String(name.to_owned())
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
        assert!(output.contains_key(key("proxies")));
        assert!(!output.contains_key(key("external-controller")));
        assert!(!output.contains_key(key("proxy-groups")));
        assert!(!output.contains_key(key("rules")));
    }

    #[test]
    fn parser_rejects_only_reserved_primary_group_name() {
        let reserved = format!("proxies:\n  - name: {HYZ_PROXY_GROUP}\n    type: ss\n");
        assert!(matches!(
            parse_mihomo_subscription(reserved.as_bytes()),
            Err(SubscriptionError::InvalidShape(_))
        ));

        assert!(parse_mihomo_subscription(
            b"proxies:\n  - name: HYZ-AUTO\n    type: ss\n"
        )
        .is_ok());
    }

    #[test]
    fn managed_policy_overwrites_source_policy_and_tracks_subscription_nodes() {
        let subscription = parse_mihomo_subscription(
            b"proxies:\n  - name: node-a\n    type: ss\n  - name: node-b\n    type: vmess\nrules: [MATCH,DIRECT]\n",
        )
        .unwrap();
        let source = compose_managed_mihomo_source(
            b"mode: global\ngeodata-mode: true\ngeo-auto-update: true\nlog-level: warning\nproxy-groups:\n  - name: stale\n    type: select\n    proxies: [DIRECT]\nrules:\n  - MATCH,DIRECT\nproxies:\n  - name: stale-node\n",
            &subscription,
        )
        .unwrap();
        let source: Value = serde_yaml::from_slice(&source).unwrap();
        let top = source.as_mapping().unwrap();

        assert_eq!(top.get(key("mode")).and_then(Value::as_str), Some("rule"));
        assert_eq!(
            top.get(key("geodata-mode")).and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            top.get(key("geo-auto-update")).and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            top.get(key("log-level")).and_then(Value::as_str),
            Some("warning")
        );
        let proxies = top
            .get(key("proxies"))
            .and_then(Value::as_sequence)
            .unwrap();
        assert_eq!(
            proxy_names(proxies).unwrap(),
            vec!["node-a".to_owned(), "node-b".to_owned()]
        );

        let groups = top
            .get(key("proxy-groups"))
            .and_then(Value::as_sequence)
            .unwrap();
        assert_eq!(groups.len(), 1);
        let selector = groups[0].as_mapping().unwrap();
        assert_eq!(
            selector.get(key("name")).and_then(Value::as_str),
            Some(HYZ_PROXY_GROUP)
        );
        assert_eq!(
            selector.get(key("type")).and_then(Value::as_str),
            Some("select")
        );
        assert_eq!(
            selector
                .get(key("proxies"))
                .and_then(Value::as_sequence)
                .unwrap(),
            &vec![
                Value::String("node-a".to_owned()),
                Value::String("node-b".to_owned()),
            ]
        );

        assert_eq!(
            top.get(key("rules")).and_then(Value::as_sequence).unwrap(),
            &vec![
                Value::String("GEOSITE,cn,DIRECT".to_owned()),
                Value::String("GEOIP,CN,DIRECT,no-resolve".to_owned()),
                Value::String("MATCH,HYZ-PROXY".to_owned()),
            ]
        );
    }

    #[test]
    fn managed_selector_keeps_subscription_nodes_without_name_filtering() {
        let subscription = parse_mihomo_subscription(
            "proxies:\n  - name: 官网 tf520.top\n    type: ss\n  - name: 剩余流量：154.94 GB\n    type: ss\n  - name: HYZ-AUTO\n    type: ss\n"
                .as_bytes(),
        )
        .unwrap();
        let source = compose_managed_mihomo_source(b"proxies: []\n", &subscription).unwrap();
        let source: Value = serde_yaml::from_slice(&source).unwrap();
        let selector = source
            .get(key("proxy-groups"))
            .and_then(Value::as_sequence)
            .and_then(|groups| groups.first())
            .and_then(Value::as_mapping)
            .unwrap();

        assert_eq!(
            selector
                .get(key("proxies"))
                .and_then(Value::as_sequence)
                .unwrap(),
            &vec![
                Value::String("官网 tf520.top".to_owned()),
                Value::String("剩余流量：154.94 GB".to_owned()),
                Value::String("HYZ-AUTO".to_owned()),
            ]
        );
    }

    #[test]
    fn unrelated_source_fields_are_preserved() {
        let subscription = parse_mihomo_subscription(
            b"proxies:\n  - name: node-a\n    type: vmess\n    server: example.com\n    port: 443\n    uuid: 00000000-0000-4000-8000-000000000000\n    cipher: auto\n",
        )
        .unwrap();
        let source = compose_managed_mihomo_source(
            b"proxy-providers:\n  subscription:\n    type: file\n    path: ./providers/subscription.yaml\n    health-check:\n      enable: true\n      url: http://www.gstatic.com/generate_204\n      interval: 600\nlog-level: warning\nproxies: []\n",
            &subscription,
        )
        .unwrap();
        let source: Value = serde_yaml::from_slice(&source).unwrap();
        let provider = source
            .get(key("proxy-providers"))
            .and_then(Value::as_mapping)
            .and_then(|providers| providers.get(key("subscription")))
            .and_then(Value::as_mapping)
            .unwrap();
        let enabled = provider
            .get(key("health-check"))
            .and_then(Value::as_mapping)
            .and_then(|health_check| health_check.get(key("enable")))
            .and_then(Value::as_bool);

        assert_eq!(enabled, Some(true));
        assert_eq!(
            source.get(key("log-level")).and_then(Value::as_str),
            Some("warning")
        );
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
