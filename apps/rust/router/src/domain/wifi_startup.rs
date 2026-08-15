use crate::domain::network_config::StaConfig;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use sha1::{Digest, Sha1};
use std::fmt;

/// Freshness bound for a persisted last-good STA channel record. Older records are treated as
/// stale and fall back to the bounded shared-channel wait, keeping the cache from permanently
/// pinning a channel that the upstream may have left.
pub const LAST_GOOD_MAX_AGE_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// Radio band of the shared AP/STA channel. RTL8852BS concurrent mode uses a single radio, so
/// AP and STA must share one band and one channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ApBand {
    Ghz2,
    Ghz5,
}

/// A validated AP radio channel that the single radio can program. The allowed set mirrors the
/// adapter's `ApRadioChannel`: 2.4 GHz 1-13 and the fixed non-DFS 5 GHz set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApChannel {
    band: ApBand,
    number: u8,
}

impl ApChannel {
    pub fn new(band: ApBand, number: u8) -> Option<Self> {
        let valid = match band {
            ApBand::Ghz2 => (1..=13).contains(&number),
            ApBand::Ghz5 => matches!(number, 36 | 40 | 44 | 48 | 149 | 153 | 157 | 161 | 165),
        };
        valid.then_some(Self { band, number })
    }

    pub const fn band(self) -> ApBand {
        self.band
    }

    pub const fn number(self) -> u8 {
        self.number
    }
}

impl<'de> Deserialize<'de> for ApChannel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            band: ApBand,
            number: u8,
        }
        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.band, raw.number).ok_or_else(|| {
            de::Error::custom(format!(
                "invalid AP channel band {} / {}",
                raw.band, raw.number
            ))
        })
    }
}

impl fmt::Display for ApBand {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ghz2 => formatter.write_str("2.4GHz"),
            Self::Ghz5 => formatter.write_str("5GHz"),
        }
    }
}

/// Deterministic identity of the committed STA configuration. Derived with SHA-1 over the SSID
/// and the derived PSK so the persisted record never contains the plaintext PSK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StaFingerprint([u8; 20]);

impl StaFingerprint {
    pub fn of(config: &StaConfig) -> Self {
        let mut hasher = Sha1::new();
        hasher.update(config.ssid.as_bytes());
        hasher.update(config.psk.to_hex().as_bytes());
        let digest = hasher.finalize();
        let mut bytes = [0_u8; 20];
        bytes.copy_from_slice(&digest);
        Self(bytes)
    }

    pub fn from_hex(value: &str) -> Option<Self> {
        if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let mut bytes = [0_u8; 20];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            bytes[index] = (hex_nibble(pair[0]) << 4) | hex_nibble(pair[1]);
        }
        Some(Self(bytes))
    }

    pub fn to_hex(&self) -> String {
        let mut output = String::with_capacity(40);
        for byte in self.0.iter() {
            use fmt::Write as _;
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
        }
        output
    }
}

impl fmt::Display for StaFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl Serialize for StaFingerprint {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for StaFingerprint {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_hex(&value).ok_or_else(|| {
            de::Error::custom("STA fingerprint must contain exactly 40 hexadecimal characters")
        })
    }
}

fn hex_nibble(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => unreachable!("hex was validated before decoding"),
    }
}

/// Persisted record of the last channel on which the AP and the committed STA shared the single
/// radio. This is a runtime-derived cache, never an ownership or readiness marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LastGoodStaChannel {
    pub fingerprint: StaFingerprint,
    pub channel: ApChannel,
    pub recorded_unix_ms: u64,
}

/// How management startup should choose the AP channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupChannelPlan {
    /// A fresh last-good record matches the committed STA; start the AP directly on that shared
    /// channel and skip the bounded association wait.
    FastStart { channel: ApChannel },
    /// No usable last-good record; run the bounded wait for the committed STA's shared channel.
    WaitForStaChannel,
}

pub fn startup_channel_plan(
    last_good: Option<&LastGoodStaChannel>,
    fingerprint: &StaFingerprint,
    now_unix_ms: u64,
    max_age_ms: u64,
) -> StartupChannelPlan {
    match last_good {
        Some(record)
            if record.fingerprint == *fingerprint
                && now_unix_ms.saturating_sub(record.recorded_unix_ms) <= max_age_ms =>
        {
            StartupChannelPlan::FastStart {
                channel: record.channel,
            }
        }
        _ => StartupChannelPlan::WaitForStaChannel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::network_config::{WifiPassphrase, WifiSsid};

    fn sta_config(ssid: &str, passphrase: &str) -> StaConfig {
        StaConfig::from_passphrase(
            WifiSsid::new(ssid).unwrap(),
            &WifiPassphrase::new(passphrase).unwrap(),
        )
    }

    fn record(
        fingerprint: &StaFingerprint,
        channel: ApChannel,
        recorded_unix_ms: u64,
    ) -> LastGoodStaChannel {
        LastGoodStaChannel {
            fingerprint: *fingerprint,
            channel,
            recorded_unix_ms,
        }
    }

    #[test]
    fn fingerprint_is_deterministic_and_distinct_and_never_leaks_secrets() {
        let same = sta_config("home-uplink", "correct-horse");
        let other_ssid = sta_config("other-uplink", "correct-horse");
        let other_psk = sta_config("home-uplink", "different-secret");

        let a = StaFingerprint::of(&same);
        let b = StaFingerprint::of(&same);
        assert_eq!(a, b);
        assert_ne!(a, StaFingerprint::of(&other_ssid));
        assert_ne!(a, StaFingerprint::of(&other_psk));
        assert_eq!(a.to_hex().len(), 40);
        assert!(!a.to_hex().contains("correct"));
        assert!(!a.to_hex().contains(&same.psk.to_hex().to_string()));
        assert!(!a.to_hex().contains("home-uplink"));
    }

    #[test]
    fn fingerprint_hex_roundtrips_and_rejects_malformed_input() {
        let original = StaFingerprint::of(&sta_config("roundtrip", "wifi-secret-123"));
        let hex = original.to_hex();
        assert_eq!(StaFingerprint::from_hex(&hex), Some(original));
        assert_eq!(StaFingerprint::from_hex(&hex[..38]), None);
        assert_eq!(StaFingerprint::from_hex(&"z".repeat(40)), None);
        assert_eq!(StaFingerprint::from_hex(""), None);
    }

    #[test]
    fn ap_channel_validates_bands_and_numbers() {
        assert_eq!(ApChannel::new(ApBand::Ghz2, 6).unwrap().number(), 6);
        assert_eq!(ApChannel::new(ApBand::Ghz2, 13).unwrap().number(), 13);
        assert_eq!(ApChannel::new(ApBand::Ghz5, 161).unwrap().number(), 161);
        assert_eq!(ApChannel::new(ApBand::Ghz2, 0), None);
        assert_eq!(ApChannel::new(ApBand::Ghz2, 14), None);
        assert_eq!(ApChannel::new(ApBand::Ghz5, 13), None);
        assert_eq!(ApChannel::new(ApBand::Ghz5, 52), None);
        assert_eq!(
            ApChannel::new(ApBand::Ghz5, 36),
            Some(ApChannel::new(ApBand::Ghz5, 36).unwrap())
        );
    }

    #[test]
    fn startup_plan_fast_starts_on_fresh_matching_record() {
        let fingerprint = StaFingerprint::of(&sta_config("home", "secret-pass-88"));
        let channel = ApChannel::new(ApBand::Ghz5, 161).unwrap();
        let plan = startup_channel_plan(
            Some(&record(&fingerprint, channel, 1_000_000)),
            &fingerprint,
            1_000_000 + 60_000,
            LAST_GOOD_MAX_AGE_MS,
        );
        assert_eq!(plan, StartupChannelPlan::FastStart { channel });
    }

    #[test]
    fn startup_plan_waits_on_mismatch_stale_or_missing_record() {
        let fingerprint = StaFingerprint::of(&sta_config("home", "secret-pass-88"));
        let channel = ApChannel::new(ApBand::Ghz5, 161).unwrap();
        let cases = [
            None,
            Some(record(
                &StaFingerprint::of(&sta_config("other", "secret-pass-88")),
                channel,
                1_000_000,
            )),
            Some(record(&fingerprint, channel, 1_000_000)),
        ];
        for last_good in cases {
            let now = match last_good.as_ref() {
                Some(rec) if rec.fingerprint == fingerprint => 1_000_000 + LAST_GOOD_MAX_AGE_MS + 1,
                _ => 1_000_000,
            };
            assert_eq!(
                startup_channel_plan(last_good.as_ref(), &fingerprint, now, LAST_GOOD_MAX_AGE_MS),
                StartupChannelPlan::WaitForStaChannel
            );
        }
    }

    #[test]
    fn last_good_record_serde_roundtrips_and_rejects_unknown_fields() {
        let fingerprint = StaFingerprint::of(&sta_config("home", "secret-pass-88"));
        let record = record(&fingerprint, ApChannel::new(ApBand::Ghz2, 6).unwrap(), 42);
        let json = serde_json::to_string(&record).unwrap();
        assert_eq!(
            serde_json::from_str::<LastGoodStaChannel>(&json).unwrap(),
            record
        );

        assert!(serde_json::from_str::<LastGoodStaChannel>(
            r#"{"fingerprint":"0000000000000000000000000000000000000000","channel":{"band":"Ghz2","number":6},"recorded_unix_ms":1,"extra":true}"#
        )
        .is_err());
        assert!(serde_json::from_str::<LastGoodStaChannel>(
            r#"{"fingerprint":"0000000000000000000000000000000000000000","channel":{"band":"Ghz5","number":52},"recorded_unix_ms":1}"#
        )
        .is_err());
        assert!(serde_json::from_str::<LastGoodStaChannel>(
            r#"{"fingerprint":"not-hex","channel":{"band":"Ghz2","number":6},"recorded_unix_ms":1}"#
        )
        .is_err());
    }

    #[test]
    fn serialized_record_never_contains_sta_secret() {
        let secret = "super-secret-passphrase";
        let fingerprint = StaFingerprint::of(&sta_config("home-uplink", secret));
        let json = serde_json::to_string(&record(
            &fingerprint,
            ApChannel::new(ApBand::Ghz5, 161).unwrap(),
            1,
        ))
        .unwrap();
        assert!(!json.contains(secret));
        assert!(!json.contains("home-uplink"));
    }
}
