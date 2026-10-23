use crate::{
    adapters::outbound::{process::LinuxRouterPlatform, status::stable_public_ipv6_address},
    application::{
        ddns::{DdnsAddressPort, DdnsConfigPort, DdnsProviderPort, DdnsProviderResult},
        ports::PlatformError,
    },
    domain::ddns::{parse_aliyun_ddns_env, AliyunDdnsConfig},
};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::Read,
    net::Ipv6Addr,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use ureq::{
    tls::{TlsConfig, TlsProvider},
    Agent,
};

const ENV_PATH: &str = "/userdata/hyz-things/.env";
const MAX_ENV_BYTES: u64 = 16 * 1024;
const API_HOST: &str = "alidns.aliyuncs.com";
const API_ENDPOINT: &str = "https://alidns.aliyuncs.com/";
const API_VERSION: &str = "2015-01-09";
const SIGNED_HEADERS: &str =
    "host;x-acs-action;x-acs-content-sha256;x-acs-date;x-acs-signature-nonce;x-acs-version";
const EMPTY_BODY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const MAX_API_RESPONSE_BYTES: u64 = 64 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct AliyunDdnsAdapter {
    router: LinuxRouterPlatform,
    agent: Agent,
}

impl AliyunDdnsAdapter {
    pub fn new(router: LinuxRouterPlatform) -> Self {
        let agent = Agent::new_with_config(
            Agent::config_builder()
                .https_only(true)
                .proxy(None)
                .max_redirects(0)
                .timeout_global(Some(REQUEST_TIMEOUT))
                .tls_config(TlsConfig::builder().provider(TlsProvider::Rustls).build())
                .build(),
        );
        Self { router, agent }
    }

    fn call_api(
        &self,
        config: &AliyunDdnsConfig,
        action: &'static str,
        parameters: &[(&str, String)],
    ) -> Result<serde_json::Value, PlatformError> {
        let date = utc_timestamp()?;
        let nonce = request_nonce()?;
        let query = parameters
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect::<Vec<_>>();
        let signed = sign_request(
            API_HOST,
            action,
            API_VERSION,
            &date,
            &nonce,
            config.access_key_id(),
            config.access_key_secret(),
            &query,
        );
        let url = format!("{API_ENDPOINT}?{}", signed.canonical_query);
        let mut response = self
            .agent
            .post(&url)
            .header("x-acs-action", action)
            .header("x-acs-version", API_VERSION)
            .header("x-acs-date", &date)
            .header("x-acs-signature-nonce", &nonce)
            .header("x-acs-content-sha256", EMPTY_BODY_SHA256)
            .header("Authorization", &signed.authorization)
            .header("Accept", "application/json")
            .send_empty()
            .map_err(|_| PlatformError::CommandFailed(format!("AliDNS {action} request failed")))?;
        let status = response.status().as_u16();
        let mut body = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(MAX_API_RESPONSE_BYTES + 1)
            .read_to_end(&mut body)
            .map_err(|_| PlatformError::ProbeFailed("AliDNS response could not be read".into()))?;
        if body.len() as u64 > MAX_API_RESPONSE_BYTES {
            return Err(PlatformError::ProbeFailed(
                "AliDNS response exceeded the size limit".to_owned(),
            ));
        }
        if !(200..300).contains(&status) {
            return Err(PlatformError::CommandFailed(format!(
                "AliDNS {action} returned HTTP {status}"
            )));
        }
        let value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|_| PlatformError::ProbeFailed("AliDNS response was invalid JSON".into()))?;
        if let Some(code) = value.get("Code").and_then(serde_json::Value::as_str) {
            let safe_code = if !code.is_empty()
                && code.len() <= 96
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                code
            } else {
                "unknown"
            };
            return Err(PlatformError::CommandFailed(format!(
                "AliDNS {action} was rejected ({safe_code})"
            )));
        }
        Ok(value)
    }

    fn find_target_record(&self, config: &AliyunDdnsConfig) -> Result<AliyunRecord, PlatformError> {
        let value = self.call_api(
            config,
            "DescribeDomainRecords",
            &[
                ("DomainName", config.domain_name().to_owned()),
                ("RRKeyWord", config.rr().to_owned()),
                ("Type", "AAAA".to_owned()),
                ("Line", "default".to_owned()),
                ("SearchMode", "COMBINATION".to_owned()),
                ("PageNumber", "1".to_owned()),
                ("PageSize", "100".to_owned()),
            ],
        )?;
        let response: DescribeRecordsResponse = serde_json::from_value(value)
            .map_err(|_| PlatformError::ProbeFailed("AliDNS record response was invalid".into()))?;
        if response.total_count != 1 || response.domain_records.records.len() != 1 {
            return Err(PlatformError::Conflict(
                "AliDNS must contain exactly one matching default-line AAAA record".to_owned(),
            ));
        }
        let record = response.domain_records.records.into_iter().next().unwrap();
        if !record
            .domain_name
            .eq_ignore_ascii_case(config.domain_name())
            || !record.rr.eq_ignore_ascii_case(config.rr())
            || record.record_type != "AAAA"
            || record.line != "default"
            || !record.status.eq_ignore_ascii_case("Enable")
            || record.record_id.is_empty()
            || record.record_id.len() > 96
            || !record
                .record_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
            || record.ttl == 0
            || record.value.parse::<Ipv6Addr>().is_err()
        {
            return Err(PlatformError::Conflict(
                "AliDNS returned a record outside the configured target".to_owned(),
            ));
        }
        Ok(record)
    }

    fn update_target_record(
        &self,
        config: &AliyunDdnsConfig,
        record: &AliyunRecord,
        address: Ipv6Addr,
    ) -> Result<(), PlatformError> {
        let value = self.call_api(
            config,
            "UpdateDomainRecord",
            &[
                ("RecordId", record.record_id.clone()),
                ("RR", config.rr().to_owned()),
                ("Type", "AAAA".to_owned()),
                ("Value", address.to_string()),
                ("TTL", record.ttl.to_string()),
                ("Line", "default".to_owned()),
            ],
        )?;
        let response: UpdateRecordResponse = serde_json::from_value(value)
            .map_err(|_| PlatformError::ProbeFailed("AliDNS update response was invalid".into()))?;
        if response.record_id != record.record_id {
            return Err(PlatformError::Conflict(
                "AliDNS update response did not match the target record".to_owned(),
            ));
        }
        Ok(())
    }
}

impl DdnsConfigPort for AliyunDdnsAdapter {
    fn load_aliyun_ddns_config(&self) -> Result<Option<AliyunDdnsConfig>, PlatformError> {
        let Some(contents) = read_private_env_file()? else {
            return Ok(None);
        };
        parse_aliyun_ddns_env(&contents)
            .map_err(|error| PlatformError::InvalidState(error.to_string()))
    }
}

impl DdnsAddressPort for AliyunDdnsAdapter {
    fn stable_public_ipv6(&self) -> Result<Option<Ipv6Addr>, PlatformError> {
        Ok(stable_public_ipv6_address(&self.router))
    }
}

impl DdnsProviderPort for AliyunDdnsAdapter {
    fn reconcile_aaaa(
        &self,
        config: &AliyunDdnsConfig,
        address: Ipv6Addr,
    ) -> Result<DdnsProviderResult, PlatformError> {
        let record = self.find_target_record(config)?;
        if record.value.parse::<Ipv6Addr>().ok() == Some(address) {
            return Ok(DdnsProviderResult::Unchanged);
        }
        self.update_target_record(config, &record, address)?;
        Ok(DdnsProviderResult::Updated)
    }
}

fn read_private_env_file() -> Result<Option<String>, PlatformError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = match options.open(ENV_PATH) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(PlatformError::InvalidState(
                "Things .env file cannot be opened safely".to_owned(),
            ))
        }
    };
    let metadata = file
        .metadata()
        .map_err(|_| PlatformError::InvalidState("Things .env metadata is unavailable".into()))?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o077 != 0
        || metadata.len() > MAX_ENV_BYTES
    {
        return Err(PlatformError::InvalidState(
            "Things .env must be a root-owned private regular file under 16 KiB".to_owned(),
        ));
    }
    let mut contents = String::new();
    file.take(MAX_ENV_BYTES + 1)
        .read_to_string(&mut contents)
        .map_err(|_| PlatformError::InvalidState("Things .env is not valid UTF-8".into()))?;
    if contents.len() as u64 > MAX_ENV_BYTES {
        return Err(PlatformError::InvalidState(
            "Things .env exceeds the size limit".to_owned(),
        ));
    }
    Ok(Some(contents))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct DescribeRecordsResponse {
    total_count: usize,
    domain_records: DomainRecords,
}

#[derive(Deserialize)]
struct DomainRecords {
    #[serde(rename = "Record", default)]
    records: Vec<AliyunRecord>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct AliyunRecord {
    record_id: String,
    domain_name: String,
    #[serde(rename = "RR")]
    rr: String,
    #[serde(rename = "Type")]
    record_type: String,
    value: String,
    line: String,
    status: String,
    #[serde(rename = "TTL")]
    ttl: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct UpdateRecordResponse {
    record_id: String,
}

#[derive(Debug, PartialEq, Eq)]
struct SignedRequest {
    canonical_query: String,
    authorization: String,
}

fn sign_request(
    host: &str,
    action: &str,
    version: &str,
    date: &str,
    nonce: &str,
    access_key_id: &str,
    access_key_secret: &str,
    query: &[(&str, &str)],
) -> SignedRequest {
    let canonical_query = canonical_query(query);
    let canonical_headers = format!(
        "host:{host}\nx-acs-action:{action}\nx-acs-content-sha256:{EMPTY_BODY_SHA256}\nx-acs-date:{date}\nx-acs-signature-nonce:{nonce}\nx-acs-version:{version}\n"
    );
    let canonical_request = format!(
        "POST\n/\n{canonical_query}\n{canonical_headers}\n{SIGNED_HEADERS}\n{EMPTY_BODY_SHA256}"
    );
    let string_to_sign = format!(
        "ACS3-HMAC-SHA256\n{}",
        hex_lower(&Sha256::digest(canonical_request.as_bytes()))
    );
    let mut mac = Hmac::<Sha256>::new_from_slice(access_key_secret.as_bytes())
        .expect("HMAC accepts keys of any length");
    mac.update(string_to_sign.as_bytes());
    let signature = hex_lower(&mac.finalize().into_bytes());
    let authorization = format!(
        "ACS3-HMAC-SHA256 Credential={access_key_id},SignedHeaders={SIGNED_HEADERS},Signature={signature}"
    );
    SignedRequest {
        canonical_query,
        authorization,
    }
}

fn canonical_query(query: &[(&str, &str)]) -> String {
    let mut pairs = query.iter().copied().collect::<Vec<_>>();
    pairs.sort_unstable_by(|left, right| left.0.cmp(right.0));
    pairs
        .into_iter()
        .map(|(key, value)| format!("{}={}", encode_component(key), encode_component(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn utc_timestamp() -> Result<String, PlatformError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| PlatformError::ProbeFailed("system time is before Unix epoch".into()))?
        .as_secs();
    let seconds: libc::time_t = seconds
        .try_into()
        .map_err(|_| PlatformError::ProbeFailed("system time is out of range".into()))?;
    let mut utc: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::gmtime_r(&seconds, &mut utc) }.is_null() {
        return Err(PlatformError::ProbeFailed(
            "system time could not be formatted".to_owned(),
        ));
    }
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        utc.tm_year + 1900,
        utc.tm_mon + 1,
        utc.tm_mday,
        utc.tm_hour,
        utc.tm_min,
        utc.tm_sec
    ))
}

fn request_nonce() -> Result<String, PlatformError> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|_| PlatformError::ProbeFailed("AliDNS nonce generation failed".into()))?;
    Ok(hex_lower(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_aliyun_record_fields_with_acronym_names() {
        let response: DescribeRecordsResponse = serde_json::from_value(serde_json::json!({
            "TotalCount": 1,
            "DomainRecords": {
                "Record": [{
                    "RecordId": "123456",
                    "DomainName": "example.invalid",
                    "RR": "things",
                    "Type": "AAAA",
                    "Value": "240e:380:434a:7310:6605:3dff:fe02:4304",
                    "Line": "default",
                    "Status": "Enable",
                    "TTL": 600
                }]
            }
        }))
        .expect("Aliyun DescribeDomainRecords response should deserialize");

        let record = response.domain_records.records.into_iter().next().unwrap();
        assert_eq!(response.total_count, 1);
        assert_eq!(record.rr, "things");
        assert_eq!(record.ttl, 600);
    }

    #[test]
    fn matches_aliyun_official_acs3_hmac_sha256_signature_example() {
        let signed = sign_request(
            "ecs.cn-shanghai.aliyuncs.com",
            "RunInstances",
            "2014-05-26",
            "2023-10-26T10:22:32Z",
            "3156853299f313e23d1673dc12e1703d",
            "YourAccessKeyId",
            "YourAccessKeySecret",
            &[
                (
                    "ImageId",
                    "win2019_1809_x64_dtc_zh-cn_40G_alibase_20230811.vhd",
                ),
                ("RegionId", "cn-shanghai"),
            ],
        );

        assert_eq!(
            signed.canonical_query,
            "ImageId=win2019_1809_x64_dtc_zh-cn_40G_alibase_20230811.vhd&RegionId=cn-shanghai"
        );
        assert_eq!(
            signed.authorization,
            "ACS3-HMAC-SHA256 Credential=YourAccessKeyId,SignedHeaders=host;x-acs-action;x-acs-content-sha256;x-acs-date;x-acs-signature-nonce;x-acs-version,Signature=06563a9e1b43f5dfe96b81484da74bceab24a1d853912eee15083a6f0f3283c0"
        );
    }
}
