use std::{fmt, net::Ipv6Addr};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DdnsConfigError {
    InvalidAccessKeyId,
    InvalidAccessKeySecret,
    InvalidDomainName,
    InvalidRecordName,
    InvalidEnvironment,
    IncompleteEnvironment,
    DuplicateEnvironmentKey,
    UnsupportedLine,
}

impl fmt::Display for DdnsConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidAccessKeyId => "Aliyun access key ID is invalid",
            Self::InvalidAccessKeySecret => "Aliyun access key secret is invalid",
            Self::InvalidDomainName => "Aliyun DNS domain name is invalid",
            Self::InvalidRecordName => "Aliyun DNS record name is invalid",
            Self::InvalidEnvironment => "Aliyun DDNS environment file is invalid",
            Self::IncompleteEnvironment => "Aliyun DDNS environment is incomplete",
            Self::DuplicateEnvironmentKey => "Aliyun DDNS environment key is duplicated",
            Self::UnsupportedLine => "Aliyun DDNS resolution line is unsupported",
        })
    }
}

pub struct AliyunDdnsConfig {
    access_key_id: String,
    access_key_secret: Zeroizing<String>,
    domain_name: String,
    rr: String,
}

impl AliyunDdnsConfig {
    pub fn new(
        access_key_id: String,
        access_key_secret: String,
        domain_name: String,
        rr: String,
    ) -> Result<Self, DdnsConfigError> {
        Self::new_inner(
            access_key_id,
            Zeroizing::new(access_key_secret),
            domain_name,
            rr,
        )
    }

    fn new_inner(
        access_key_id: String,
        access_key_secret: Zeroizing<String>,
        domain_name: String,
        rr: String,
    ) -> Result<Self, DdnsConfigError> {
        if access_key_id.is_empty()
            || access_key_id.len() > 128
            || !access_key_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        {
            return Err(DdnsConfigError::InvalidAccessKeyId);
        }
        if access_key_secret.is_empty()
            || access_key_secret.len() > 256
            || !access_key_secret
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
        {
            return Err(DdnsConfigError::InvalidAccessKeySecret);
        }
        if !valid_domain_name(&domain_name) {
            return Err(DdnsConfigError::InvalidDomainName);
        }
        let target_length = domain_name.len() + if rr == "@" { 0 } else { rr.len() + 1 };
        if !valid_record_name(&rr) || target_length > 253 {
            return Err(DdnsConfigError::InvalidRecordName);
        }
        Ok(Self {
            access_key_id,
            access_key_secret,
            domain_name,
            rr,
        })
    }

    pub fn access_key_id(&self) -> &str {
        &self.access_key_id
    }

    pub fn access_key_secret(&self) -> &str {
        self.access_key_secret.as_str()
    }

    pub fn domain_name(&self) -> &str {
        &self.domain_name
    }

    pub fn rr(&self) -> &str {
        &self.rr
    }
}

impl fmt::Debug for AliyunDdnsConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AliyunDdnsConfig")
            .field("access_key_id", &"[redacted]")
            .field("access_key_secret", &"[redacted]")
            .field("domain_name", &self.domain_name)
            .field("rr", &self.rr)
            .finish()
    }
}

pub fn parse_aliyun_ddns_env(input: &str) -> Result<Option<AliyunDdnsConfig>, DdnsConfigError> {
    let mut access_key_id = None;
    let mut access_key_secret: Option<Zeroizing<String>> = None;
    let mut domain_name = None;
    let mut rr = None;
    let mut line_seen = false;
    let mut has_aliyun_settings = false;

    for raw_line in input.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or(DdnsConfigError::InvalidEnvironment)?;
        let key = key.trim();
        let value = unquote_env_value(value.trim())?;
        match key {
            "ALIYUN_ACCESS_KEY_ID" => {
                set_env_value(&mut access_key_id, value)?;
                has_aliyun_settings = true;
            }
            "ALIYUN_ACCESS_KEY_SECRET" => {
                if access_key_secret.is_some() {
                    return Err(DdnsConfigError::DuplicateEnvironmentKey);
                }
                access_key_secret = Some(Zeroizing::new(value.to_owned()));
                has_aliyun_settings = true;
            }
            "ALIYUN_DOMAIN" => {
                set_env_value(&mut domain_name, value)?;
                has_aliyun_settings = true;
            }
            "ALIYUN_RR" => {
                set_env_value(&mut rr, value)?;
                has_aliyun_settings = true;
            }
            "ALIYUN_LINE" => {
                if line_seen {
                    return Err(DdnsConfigError::DuplicateEnvironmentKey);
                }
                line_seen = true;
                has_aliyun_settings = true;
                if value != "default" {
                    return Err(DdnsConfigError::UnsupportedLine);
                }
            }
            _ => {}
        }
    }

    if !has_aliyun_settings {
        return Ok(None);
    }
    let (Some(access_key_id), Some(access_key_secret), Some(domain_name), Some(rr)) =
        (access_key_id, access_key_secret, domain_name, rr)
    else {
        return Err(DdnsConfigError::IncompleteEnvironment);
    };
    AliyunDdnsConfig::new_inner(access_key_id, access_key_secret, domain_name, rr).map(Some)
}

fn set_env_value(slot: &mut Option<String>, value: &str) -> Result<(), DdnsConfigError> {
    if slot.is_some() {
        return Err(DdnsConfigError::DuplicateEnvironmentKey);
    }
    *slot = Some(value.to_owned());
    Ok(())
}

fn unquote_env_value(value: &str) -> Result<&str, DdnsConfigError> {
    let starts_quote = value.starts_with('"');
    let ends_quote = value.ends_with('"');
    match (starts_quote, ends_quote) {
        (true, true) if value.len() >= 2 => Ok(&value[1..value.len() - 1]),
        (false, false) => Ok(value),
        _ => Err(DdnsConfigError::InvalidEnvironment),
    }
}

fn valid_domain_name(value: &str) -> bool {
    value.len() <= 253 && value.split('.').count() >= 2 && valid_hostname_labels(value)
}

fn valid_record_name(value: &str) -> bool {
    value == "@" || value.len() <= 253 && valid_hostname_labels(value)
}

fn valid_hostname_labels(value: &str) -> bool {
    value.split('.').all(|label| {
        let bytes = label.as_bytes();
        !bytes.is_empty()
            && bytes.len() <= 63
            && bytes[0].is_ascii_alphanumeric()
            && bytes[bytes.len() - 1].is_ascii_alphanumeric()
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
    })
}

pub fn select_stable_public_ipv6(
    candidates: impl IntoIterator<Item = (Ipv6Addr, bool)>,
) -> Option<Ipv6Addr> {
    candidates.into_iter().find_map(|(address, temporary)| {
        (!temporary && is_public_global_ipv6(address)).then_some(address)
    })
}

pub fn is_public_global_ipv6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    let in_global_unicast_range = segments[0] & 0xe000 == 0x2000;
    let documentation = (segments[0] == 0x2001 && segments[1] == 0x0db8)
        || (segments[0] == 0x3fff && segments[1] & 0xf000 == 0);
    let benchmarking = segments[0] == 0x2001 && segments[1] == 0x0002 && segments[2] == 0;
    let orchid = segments[0] == 0x2001 && matches!(segments[1] & 0xfff0, 0x0010 | 0x0020);
    in_global_unicast_range && !documentation && !benchmarking && !orchid && segments[0] != 0x2002
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disables_when_the_shared_env_file_has_no_aliyun_keys() {
        assert!(parse_aliyun_ddns_env("HYZ_THINGS_CODEX_CWD=/workspace\n")
            .unwrap()
            .is_none());
    }

    #[test]
    fn rejects_partial_duplicate_and_non_default_line_settings() {
        assert_eq!(
            parse_aliyun_ddns_env("ALIYUN_ACCESS_KEY_ID=LTAI1234567890\n").unwrap_err(),
            DdnsConfigError::IncompleteEnvironment
        );
        assert_eq!(
            parse_aliyun_ddns_env(
                "ALIYUN_ACCESS_KEY_ID=LTAI1234567890\nALIYUN_ACCESS_KEY_ID=LTAI0987654321\n"
            )
            .unwrap_err(),
            DdnsConfigError::DuplicateEnvironmentKey
        );
        assert_eq!(
            parse_aliyun_ddns_env(
                "ALIYUN_ACCESS_KEY_ID=LTAI1234567890\nALIYUN_ACCESS_KEY_SECRET=secret\nALIYUN_DOMAIN=example.invalid\nALIYUN_RR=things\nALIYUN_LINE=cn_mobile\n"
            )
            .unwrap_err(),
            DdnsConfigError::UnsupportedLine
        );
    }

    #[test]
    fn parses_aliyun_credentials_and_target_from_existing_env_file() {
        let config = parse_aliyun_ddns_env(
            "HYZ_THINGS_CODEX_CWD=/workspace\nALIYUN_ACCESS_KEY_ID=LTAI1234567890\nALIYUN_ACCESS_KEY_SECRET=secret-value\nALIYUN_DOMAIN=example.invalid\nALIYUN_RR=things\n",
        )
        .unwrap()
        .expect("Aliyun settings are present");

        assert_eq!(config.domain_name(), "example.invalid");
        assert_eq!(config.rr(), "things");
        assert_eq!(config.access_key_id(), "LTAI1234567890");
        assert_eq!(config.access_key_secret(), "secret-value");
    }

    #[test]
    fn accepts_configured_domain_and_record_changes() {
        for (domain, rr) in [
            ("example.invalid", "home"),
            ("new.example.invalid", "@"),
            ("example.org", "router"),
        ] {
            let config = AliyunDdnsConfig::new(
                "LTAI1234567890".to_owned(),
                "test-secret".to_owned(),
                domain.to_owned(),
                rr.to_owned(),
            )
            .expect("a valid configured target can change");
            assert_eq!(config.domain_name(), domain);
            assert_eq!(config.rr(), rr);
        }
    }

    #[test]
    fn rejects_wildcard_and_non_hostname_records() {
        for rr in ["*", "_service", "-home", "home-", "a..b"] {
            assert!(
                AliyunDdnsConfig::new(
                    "LTAI1234567890".to_owned(),
                    "test-secret".to_owned(),
                    "example.invalid".to_owned(),
                    rr.to_owned(),
                )
                .is_err(),
                "accepted record {rr}"
            );
        }
    }

    #[test]
    fn apex_record_uses_the_domain_length_without_an_extra_label() {
        let domain = [
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61),
        ]
        .join(".");
        assert_eq!(domain.len(), 253);
        assert!(AliyunDdnsConfig::new(
            "LTAI1234567890".into(),
            "test-secret".into(),
            domain.clone(),
            "@".into()
        )
        .is_ok());
        assert!(AliyunDdnsConfig::new(
            "LTAI1234567890".into(),
            "test-secret".into(),
            domain,
            "home".into()
        )
        .is_err());
    }

    #[test]
    fn constructs_valid_target_and_redacts_credentials_from_debug() {
        let config = AliyunDdnsConfig::new(
            "LTAI1234567890".to_owned(),
            "super-secret-key".to_owned(),
            "example.invalid".to_owned(),
            "things".to_owned(),
        )
        .expect("valid Aliyun DDNS configuration");

        assert_eq!(config.domain_name(), "example.invalid");
        assert_eq!(config.rr(), "things");
        let debug = format!("{config:?}");
        assert!(!debug.contains("super-secret-key"));
        assert!(!debug.contains("LTAI1234567890"));
    }

    #[test]
    fn selects_non_temporary_public_ipv6_and_ignores_temporary_or_private_addresses() {
        let temporary: Ipv6Addr = "2606:4700:4700::1111".parse().unwrap();
        let private: Ipv6Addr = "fd12::1".parse().unwrap();
        let stable: Ipv6Addr = "2606:4700:4700::2222".parse().unwrap();

        assert_eq!(
            select_stable_public_ipv6([(temporary, true), (private, false), (stable, false),]),
            Some(stable)
        );
    }

    #[test]
    fn refuses_to_use_a_temporary_address_when_no_stable_public_address_exists() {
        let temporary: Ipv6Addr = "2606:4700:4700::1111".parse().unwrap();
        assert_eq!(select_stable_public_ipv6([(temporary, true)]), None);
    }
}
