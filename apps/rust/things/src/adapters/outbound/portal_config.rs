use std::{fs, os::unix::fs::MetadataExt};
use url::Url;

const ENV_PATH: &str = "/userdata/hyz-things/.env";
const MAX_ENV_BYTES: u64 = 16 * 1024;

pub fn load_allowed_origins(listener_port: u16) -> Result<Vec<String>, &'static str> {
    let metadata = match fs::symlink_metadata(ENV_PATH) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Things .env cannot be read"),
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.mode() & 0o077 != 0
        || metadata.len() > MAX_ENV_BYTES
    {
        return Err("Things .env must be a root-owned private regular file");
    }
    let input = fs::read_to_string(ENV_PATH).map_err(|_| "Things .env cannot be read")?;
    parse_allowed_origins(&input, listener_port)
}

pub fn parse_allowed_origins(input: &str, listener_port: u16) -> Result<Vec<String>, &'static str> {
    let mut domain = None;
    let mut record = None;
    let mut public_port = None;
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let slot = match key.trim() {
            "ALIYUN_DOMAIN" => &mut domain,
            "ALIYUN_RR" => &mut record,
            "HYZ_THINGS_PUBLIC_PORT" => &mut public_port,
            _ => continue,
        };
        if slot.is_some() {
            return Err("Things public hostname configuration is duplicated");
        }
        *slot = Some(unquote(value.trim())?.to_owned());
    }
    let (domain, record) = match (domain, record) {
        (None, None) => return Ok(Vec::new()),
        (Some(domain), Some(record)) => (domain, record),
        _ => return Err("Things public hostname configuration is incomplete"),
    };
    if domain.len() > 253 || domain.split('.').count() < 2 || !valid_hostname_labels(&domain) {
        return Err("Things public DNS domain is invalid");
    }
    if record != "@" && !valid_hostname_labels(&record) {
        return Err("Things public DNS record is invalid");
    }
    let hostname = if record == "@" {
        domain
    } else {
        format!("{record}.{domain}")
    };
    let public_port = public_port
        .map(|value| value.parse::<u16>())
        .transpose()
        .map_err(|_| "Things public port is invalid")?
        .unwrap_or(listener_port);
    if hostname.len() > 253 || public_port == 0 {
        return Err("Things public origin is invalid");
    }
    Ok(vec![parse_https_origin(&format!(
        "https://{hostname}:{public_port}"
    ))?])
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

fn unquote(value: &str) -> Result<&str, &'static str> {
    match (value.starts_with('"'), value.ends_with('"')) {
        (true, true) if value.len() >= 2 => Ok(&value[1..value.len() - 1]),
        (false, false) => Ok(value),
        _ => Err("Things public hostname configuration has mismatched quotes"),
    }
}

fn parse_https_origin(value: &str) -> Result<String, &'static str> {
    let url = Url::parse(value).map_err(|_| "Things allowed origin is invalid")?;
    if url.scheme() != "https"
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
    {
        return Err("Things allowed origin must be an exact HTTPS origin");
    }
    match url.origin() {
        url::Origin::Tuple(..) => Ok(url.origin().ascii_serialization()),
        url::Origin::Opaque(_) => Err("Things allowed origin must be an exact HTTPS origin"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_https_origin_from_the_configured_domain_and_record() {
        for (domain, rr, host) in [
            ("example.invalid", "home", "home.example.invalid"),
            ("portal.example.org", "@", "portal.example.org"),
            ("example.net", "router", "router.example.net"),
        ] {
            let input = format!("ALIYUN_DOMAIN={domain}\nALIYUN_RR={rr}\n");
            assert_eq!(
                parse_allowed_origins(&input, 8080).unwrap(),
                [format!("https://{host}:8080")]
            );
        }
    }

    #[test]
    fn public_port_can_describe_an_https_reverse_proxy() {
        let input = "ALIYUN_DOMAIN=example.invalid\nALIYUN_RR=home\nHYZ_THINGS_PUBLIC_PORT=443\n";
        assert_eq!(
            parse_allowed_origins(input, 8080).unwrap(),
            ["https://home.example.invalid"]
        );
    }

    #[test]
    fn rejects_incomplete_duplicate_or_unsafe_target_configuration() {
        for input in [
            "ALIYUN_DOMAIN='example.invalid'\nALIYUN_RR=home\n",
            "ALIYUN_DOMAIN=example.invalid\n",
            "ALIYUN_DOMAIN=example.invalid\nALIYUN_RR=home\nALIYUN_RR=other\n",
            "ALIYUN_DOMAIN=example.invalid/path\nALIYUN_RR=home\n",
            "ALIYUN_DOMAIN=example.invalid\nALIYUN_RR=*\n",
            "ALIYUN_DOMAIN=example.invalid\nALIYUN_RR=home\nHYZ_THINGS_PUBLIC_PORT=0\n",
            "ALIYUN_DOMAIN=example.invalid\nALIYUN_RR=home\nHYZ_THINGS_PUBLIC_PORT=bad\n",
        ] {
            assert!(
                parse_allowed_origins(input, 8080).is_err(),
                "accepted unsafe configuration"
            );
        }
    }

    #[test]
    fn absent_target_produces_no_additional_origins() {
        assert!(parse_allowed_origins("OTHER_SETTING=value\n", 8080)
            .unwrap()
            .is_empty());
    }
}
