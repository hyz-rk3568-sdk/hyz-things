use std::{
    collections::{HashMap, HashSet},
    fs,
    sync::{Mutex, OnceLock},
    time::Duration,
};

use serde_json::Value;

use super::{
    paths::{MIHOMO_CONTROLLER_ADDRESS, MIHOMO_CONTROLLER_SECRET},
    process::LinuxRouterPlatform,
    storage,
};
use crate::{
    application::ports::{PanelPlatformPort, PlatformError},
    domain::panel::{
        valid_control_name, DisplayRequest, DisplayStatus, ProxyDelayResult, ProxyGroup,
        ProxyGroupKind, ProxyOption, ProxySelectionRequest, MAX_PROXY_GROUPS, MAX_PROXY_OPTIONS,
    },
};

const BACKLIGHT_DIR: &str = "/sys/class/backlight/backlight1";
const CONTROLLER_RESPONSE_LIMIT: usize = 2 * 1024 * 1024;
const DELAY_TEST_URL: &str = "https://www.gstatic.com/generate_204";
const DELAY_TIMEOUT_MS: u32 = 5_000;
const MAX_DELAY_GROUP_CALLS: usize = 16;
type ProxyMetadata = (Option<u32>, Option<bool>);
type DelayCache = HashMap<String, ProxyMetadata>;
static DELAY_CACHE: OnceLock<Mutex<DelayCache>> = OnceLock::new();

impl PanelPlatformPort for LinuxRouterPlatform {
    fn display_status(&self) -> Result<DisplayStatus, PlatformError> {
        read_display_status()
    }

    fn set_display(&self, request: &DisplayRequest) -> Result<DisplayStatus, PlatformError> {
        let current = read_display_status()?;
        if request.enabled {
            let brightness = request.brightness.unwrap_or_else(|| {
                if current.brightness > 0 {
                    current.brightness
                } else {
                    (current.max_brightness / 2).max(1)
                }
            });
            if brightness == 0 || brightness > current.max_brightness {
                return Err(PlatformError::InvalidState(
                    "display brightness is outside the live backlight range".to_owned(),
                ));
            }
            write_backlight("bl_power", 0)?;
            write_backlight("brightness", brightness)?;
        } else {
            if request.brightness.is_some() {
                return Err(PlatformError::InvalidState(
                    "disabled display request may not include brightness".to_owned(),
                ));
            }
            write_backlight("brightness", 0)?;
            write_backlight("bl_power", 4)?;
        }
        let observed = read_display_status()?;
        if observed.enabled != request.enabled
            || request.enabled
                && request
                    .brightness
                    .is_some_and(|brightness| observed.actual_brightness != brightness)
        {
            return Err(PlatformError::UnsafeToCutOver(
                "display control did not reach the requested backlight state".to_owned(),
            ));
        }
        Ok(observed)
    }

    fn proxy_groups(&self) -> Result<Vec<ProxyGroup>, PlatformError> {
        let proxies = controller_json("GET", "/proxies", None)?;
        let providers = controller_json("GET", "/providers/proxies", None).ok();
        let mut groups = parse_proxy_groups(&proxies, providers.as_ref())?;
        apply_cached_delays(&mut groups)?;
        Ok(groups)
    }

    fn select_proxy(&self, request: &ProxySelectionRequest) -> Result<(), PlatformError> {
        if !valid_control_name(&request.group) || !valid_control_name(&request.proxy) {
            return Err(PlatformError::InvalidState(
                "proxy selection contains an invalid name".to_owned(),
            ));
        }
        let groups = self.proxy_groups()?;
        let group = groups
            .iter()
            .find(|group| group.name == request.group)
            .ok_or_else(|| PlatformError::InvalidState("proxy group is absent".to_owned()))?;
        if !group.selectable
            || !group
                .options
                .iter()
                .any(|option| option.name == request.proxy)
        {
            return Err(PlatformError::InvalidState(
                "proxy group is not selectable or does not contain the requested node".to_owned(),
            ));
        }
        let path = format!("/proxies/{}", percent_encode(&request.group));
        let body = serde_json::to_string(&serde_json::json!({ "name": request.proxy }))
            .map_err(|error| PlatformError::InvalidState(format!("encode selection: {error}")))?;
        controller_json("PUT", &path, Some(&body))?;
        let confirmed = self.proxy_groups()?.into_iter().any(|group| {
            group.name == request.group && group.selected.as_deref() == Some(&request.proxy)
        });
        if !confirmed {
            return Err(PlatformError::UnsafeToCutOver(
                "Mihomo did not confirm the selected proxy".to_owned(),
            ));
        }
        Ok(())
    }

    fn measure_proxy_delay(&self, proxy: &str) -> Result<ProxyDelayResult, PlatformError> {
        if !valid_control_name(proxy) {
            return Err(PlatformError::InvalidState(
                "delay request contains an invalid proxy name".to_owned(),
            ));
        }
        let path = format!(
            "/proxies/{}/delay?timeout={DELAY_TIMEOUT_MS}&url={}",
            percent_encode(proxy),
            percent_encode(DELAY_TEST_URL)
        );
        let value = controller_json("GET", &path, None)?;
        let delay = value
            .get("delay")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                PlatformError::ProbeFailed("Mihomo delay result is invalid".to_owned())
            })?;
        Ok(ProxyDelayResult {
            proxy: proxy.to_owned(),
            delay_ms: delay,
        })
    }

    fn refresh_proxy_delays(&self) -> Result<Vec<ProxyGroup>, PlatformError> {
        let mut groups = self.proxy_groups()?;
        let refresh_groups = delay_refresh_group_indices(&groups)?;
        let mut tested = HashSet::new();
        let mut measured = HashMap::new();

        for index in refresh_groups {
            let group = &groups[index];
            tested.extend(group.options.iter().map(|option| option.name.clone()));
            let path = format!(
                "/group/{}/delay?timeout={DELAY_TIMEOUT_MS}&url={}",
                percent_encode(&group.name),
                percent_encode(DELAY_TEST_URL)
            );
            merge_group_delay_response(&controller_json("GET", &path, None)?, &mut measured)?;
        }

        apply_group_delay_measurements(&mut groups, &tested, &measured);
        cache_group_delays(&groups)?;
        Ok(groups)
    }
}

impl LinuxRouterPlatform {
    pub(crate) fn wait_for_mihomo_controller(&self) -> Result<(), PlatformError> {
        for _ in 0..60 {
            if controller_json("GET", "/version", None).is_ok() {
                return Ok(());
            }
            if self.mihomo_identity()?.is_none() {
                return Err(PlatformError::InvalidState(
                    "Mihomo exited before its controller became ready".to_owned(),
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(PlatformError::UnsafeToCutOver(
            "Mihomo controller did not become ready".to_owned(),
        ))
    }
}

fn read_display_status() -> Result<DisplayStatus, PlatformError> {
    let brightness = read_backlight("brightness")?;
    let actual_brightness = read_backlight("actual_brightness")?;
    let max_brightness = read_backlight("max_brightness")?;
    let power = read_backlight("bl_power")?;
    if max_brightness == 0
        || brightness > max_brightness
        || actual_brightness > max_brightness
        || !matches!(power, 0..=4)
    {
        return Err(PlatformError::ProbeFailed(
            "backlight sysfs values are inconsistent".to_owned(),
        ));
    }
    Ok(DisplayStatus {
        enabled: power == 0 && actual_brightness > 0,
        brightness,
        actual_brightness,
        max_brightness,
    })
}

fn read_backlight(name: &str) -> Result<u16, PlatformError> {
    let path = format!("{BACKLIGHT_DIR}/{name}");
    let value = fs::read_to_string(&path)
        .map_err(|error| PlatformError::ProbeFailed(format!("read {path}: {error}")))?;
    value
        .trim()
        .parse::<u16>()
        .map_err(|_| PlatformError::ProbeFailed(format!("parse {path}")))
}

fn write_backlight(name: &str, value: u16) -> Result<(), PlatformError> {
    let path = format!("{BACKLIGHT_DIR}/{name}");
    fs::write(&path, format!("{value}\n"))
        .map_err(|error| PlatformError::Io(format!("write {path}: {error}")))
}

fn controller_json(method: &str, path: &str, body: Option<&str>) -> Result<Value, PlatformError> {
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
    let url = format!("http://{MIHOMO_CONTROLLER_ADDRESS}{path}");
    let authorization = format!("Bearer {secret}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .max_redirects(0)
        .proxy(None)
        .build()
        .into();
    let response = match (method, body) {
        ("GET", None) => agent
            .get(&url)
            .header("Authorization", &authorization)
            .call(),
        ("PUT", Some(body)) => agent
            .put(&url)
            .header("Authorization", &authorization)
            .header("Content-Type", "application/json")
            .send(body.as_bytes()),
        _ => {
            return Err(PlatformError::InvalidState(
                "unsupported Mihomo controller request shape".to_owned(),
            ))
        }
    }
    .map_err(|_| PlatformError::ProbeFailed("Mihomo controller request failed".to_owned()))?;
    let mut response = response;
    let body = response
        .body_mut()
        .with_config()
        .limit(CONTROLLER_RESPONSE_LIMIT as u64)
        .read_to_string()
        .map_err(|_| PlatformError::ProbeFailed("Mihomo controller body failed".to_owned()))?;
    if body.len() > CONTROLLER_RESPONSE_LIMIT {
        return Err(PlatformError::ProbeFailed(
            "Mihomo controller response exceeds limit".to_owned(),
        ));
    }
    if body.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&body)
        .map_err(|_| PlatformError::ProbeFailed("Mihomo controller JSON is invalid".to_owned()))
}

fn delay_refresh_group_indices(groups: &[ProxyGroup]) -> Result<Vec<usize>, PlatformError> {
    let mut remaining = groups
        .iter()
        .flat_map(|group| group.options.iter().map(|option| option.name.clone()))
        .collect::<HashSet<_>>();
    let mut selected = Vec::new();
    let mut used = HashSet::new();

    while !remaining.is_empty() {
        let Some((index, gain)) = groups
            .iter()
            .enumerate()
            .filter(|(index, _)| !used.contains(index))
            .map(|(index, group)| {
                let gain = group
                    .options
                    .iter()
                    .filter(|option| remaining.contains(&option.name))
                    .count();
                (index, gain)
            })
            .max_by_key(|(_, gain)| *gain)
        else {
            break;
        };
        if gain == 0 {
            break;
        }
        if selected.len() >= MAX_DELAY_GROUP_CALLS {
            return Err(PlatformError::InvalidState(
                "proxy groups require too many bounded delay requests".to_owned(),
            ));
        }
        used.insert(index);
        selected.push(index);
        for option in &groups[index].options {
            remaining.remove(&option.name);
        }
    }
    if !remaining.is_empty() {
        return Err(PlatformError::InvalidState(
            "proxy delay request plan did not cover every displayed option".to_owned(),
        ));
    }
    Ok(selected)
}

fn merge_group_delay_response(
    value: &Value,
    measured: &mut HashMap<String, u32>,
) -> Result<(), PlatformError> {
    let delays = value.as_object().ok_or_else(|| {
        PlatformError::ProbeFailed("Mihomo group delay response is not an object".to_owned())
    })?;
    if delays.len() > MAX_PROXY_OPTIONS {
        return Err(PlatformError::ProbeFailed(
            "Mihomo group delay response exceeds limit".to_owned(),
        ));
    }
    for (name, delay) in delays {
        if !valid_control_name(name) {
            return Err(PlatformError::ProbeFailed(
                "Mihomo group delay response contains an invalid name".to_owned(),
            ));
        }
        if let Some(delay) = delay
            .as_u64()
            .and_then(|delay| u32::try_from(delay).ok())
            .filter(|delay| *delay > 0)
        {
            measured.insert(name.clone(), delay);
        }
    }
    Ok(())
}

fn apply_group_delay_measurements(
    groups: &mut [ProxyGroup],
    tested: &HashSet<String>,
    measured: &HashMap<String, u32>,
) {
    for group in groups {
        for option in &mut group.options {
            if tested.contains(&option.name) {
                option.delay_ms = measured.get(&option.name).copied();
                option.alive = Some(option.delay_ms.is_some());
            }
        }
    }
}

fn apply_cached_delays(groups: &mut [ProxyGroup]) -> Result<(), PlatformError> {
    let mut cache = DELAY_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| PlatformError::InvalidState("proxy delay cache is poisoned".to_owned()))?;
    if cache.is_empty() {
        return Ok(());
    }
    if !merge_cached_delays(groups, &cache) {
        cache.clear();
    }
    Ok(())
}

fn merge_cached_delays(groups: &mut [ProxyGroup], cache: &DelayCache) -> bool {
    let visible = groups
        .iter()
        .flat_map(|group| group.options.iter().map(|option| option.name.as_str()))
        .collect::<HashSet<_>>();
    if visible.len() != cache.len() || !visible.iter().all(|name| cache.contains_key(*name)) {
        return false;
    }
    for group in groups {
        for option in &mut group.options {
            if let Some((delay, alive)) = cache.get(&option.name) {
                option.delay_ms = *delay;
                option.alive = *alive;
            }
        }
    }
    true
}

fn cache_group_delays(groups: &[ProxyGroup]) -> Result<(), PlatformError> {
    let refreshed = groups
        .iter()
        .flat_map(|group| {
            group
                .options
                .iter()
                .map(|option| (option.name.clone(), (option.delay_ms, option.alive)))
        })
        .collect::<DelayCache>();
    let mut cache = DELAY_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| PlatformError::InvalidState("proxy delay cache is poisoned".to_owned()))?;
    *cache = refreshed;
    Ok(())
}

fn parse_provider_metadata(value: &Value) -> Result<HashMap<String, ProxyMetadata>, PlatformError> {
    let Some(providers) = value.get("providers").and_then(Value::as_object) else {
        return Ok(HashMap::new());
    };
    let mut metadata = HashMap::new();
    let mut ambiguous = HashSet::new();
    let mut total = 0usize;
    for provider in providers.values() {
        let Some(proxies) = provider.get("proxies").and_then(Value::as_array) else {
            continue;
        };
        total = total.saturating_add(proxies.len());
        if total > MAX_PROXY_OPTIONS * 2 {
            return Err(PlatformError::ProbeFailed(
                "Mihomo provider catalog exceeds limit".to_owned(),
            ));
        }
        for proxy in proxies {
            let Some(name) = proxy
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| valid_control_name(name))
            else {
                continue;
            };
            if ambiguous.contains(name) {
                continue;
            }
            let details = (
                last_delay(proxy),
                proxy.get("alive").and_then(Value::as_bool),
            );
            if metadata.insert(name.to_owned(), details).is_some() {
                metadata.remove(name);
                ambiguous.insert(name.to_owned());
            }
        }
    }
    Ok(metadata)
}

fn parse_proxy_groups(
    value: &Value,
    providers: Option<&Value>,
) -> Result<Vec<ProxyGroup>, PlatformError> {
    let proxies = value
        .get("proxies")
        .and_then(Value::as_object)
        .ok_or_else(|| PlatformError::ProbeFailed("Mihomo proxies object is absent".to_owned()))?;
    if proxies.len() > MAX_PROXY_OPTIONS * 2 {
        return Err(PlatformError::ProbeFailed(
            "Mihomo proxy catalog exceeds limit".to_owned(),
        ));
    }
    let metadata = proxies
        .iter()
        .map(|(name, value)| (name.as_str(), value))
        .collect::<HashMap<_, _>>();
    let provider_metadata = providers
        .map(parse_provider_metadata)
        .transpose()?
        .unwrap_or_default();
    let mut groups = Vec::new();
    let mut total_options = 0usize;
    for (name, group) in proxies {
        if name == "GLOBAL" {
            continue;
        }
        let Some(all) = group.get("all").and_then(Value::as_array) else {
            continue;
        };
        if !valid_control_name(name) || all.is_empty() {
            continue;
        }
        total_options = total_options.saturating_add(all.len());
        if groups.len() >= MAX_PROXY_GROUPS || total_options > MAX_PROXY_OPTIONS {
            return Err(PlatformError::ProbeFailed(
                "Mihomo proxy groups exceed display limits".to_owned(),
            ));
        }
        let kind = proxy_group_kind(group.get("type").and_then(Value::as_str));
        let mut options = Vec::with_capacity(all.len());
        for option in all {
            let Some(option) = option.as_str().filter(|value| valid_control_name(value)) else {
                continue;
            };
            let details = metadata.get(option).copied();
            let provider_details = provider_metadata.get(option).copied();
            options.push(ProxyOption {
                name: option.to_owned(),
                region: infer_region(option),
                delay_ms: details
                    .and_then(last_delay)
                    .or_else(|| provider_details.and_then(|details| details.0)),
                alive: details
                    .and_then(|value| value.get("alive"))
                    .and_then(Value::as_bool)
                    .or_else(|| provider_details.and_then(|details| details.1)),
            });
        }
        if options.is_empty() {
            continue;
        }
        let selected = group
            .get("now")
            .and_then(Value::as_str)
            .filter(|selected| options.iter().any(|option| option.name == *selected))
            .map(str::to_owned);
        groups.push(ProxyGroup {
            name: name.to_owned(),
            kind,
            selectable: kind == ProxyGroupKind::Selector,
            selected,
            options,
        });
    }
    groups.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(groups)
}

fn proxy_group_kind(value: Option<&str>) -> ProxyGroupKind {
    match value {
        Some("Selector") => ProxyGroupKind::Selector,
        Some("URLTest") => ProxyGroupKind::UrlTest,
        Some("Fallback") => ProxyGroupKind::Fallback,
        Some("LoadBalance") => ProxyGroupKind::LoadBalance,
        Some("Relay") => ProxyGroupKind::Relay,
        _ => ProxyGroupKind::Other,
    }
}

fn last_delay(value: &Value) -> Option<u32> {
    value
        .get("history")?
        .as_array()?
        .iter()
        .rev()
        .find_map(|entry| {
            entry
                .get("delay")?
                .as_u64()
                .and_then(|delay| u32::try_from(delay).ok())
                .filter(|delay| *delay > 0)
        })
}

fn percent_encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

fn infer_region(name: &str) -> Option<String> {
    let chars = name.chars().collect::<Vec<_>>();
    for pair in chars.windows(2) {
        let first = u32::from(pair[0]);
        let second = u32::from(pair[1]);
        if (0x1f1e6..=0x1f1ff).contains(&first) && (0x1f1e6..=0x1f1ff).contains(&second) {
            let code = [
                char::from_u32(u32::from(b'A') + first - 0x1f1e6)?,
                char::from_u32(u32::from(b'A') + second - 0x1f1e6)?,
            ]
            .iter()
            .collect::<String>();
            return Some(region_label(&code));
        }
    }
    let lower = name.to_ascii_lowercase();
    for (needles, code) in [
        (&["香港", "hong kong", " hk ", "hk-"][..], "HK"),
        (&["台湾", "taiwan", " tw ", "tw-"][..], "TW"),
        (&["日本", "japan", " tokyo", " jp ", "jp-"][..], "JP"),
        (&["新加坡", "singapore", " sg ", "sg-"][..], "SG"),
        (&["美国", "united states", " usa", " us ", "us-"][..], "US"),
        (&["韩国", "korea", " kr ", "kr-"][..], "KR"),
        (&["英国", "united kingdom", " uk ", "uk-"][..], "GB"),
        (&["德国", "germany", " de ", "de-"][..], "DE"),
        (&["法国", "france", " fr ", "fr-"][..], "FR"),
        (&["加拿大", "canada", " ca ", "ca-"][..], "CA"),
        (&["澳大利亚", "australia", " au ", "au-"][..], "AU"),
    ] {
        if needles.iter().any(|needle| lower.contains(needle)) {
            return Some(region_label(code));
        }
    }
    None
}

fn region_label(code: &str) -> String {
    let name = match code {
        "HK" => "香港",
        "TW" => "台湾",
        "JP" => "日本",
        "SG" => "新加坡",
        "US" => "美国",
        "KR" => "韩国",
        "GB" => "英国",
        "DE" => "德国",
        "FR" => "法国",
        "CA" => "加拿大",
        "AU" => "澳大利亚",
        _ => return code.to_owned(),
    };
    format!("{name} ({code})")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(name: &str, options: &[&str]) -> ProxyGroup {
        ProxyGroup {
            name: name.to_owned(),
            kind: ProxyGroupKind::Selector,
            selectable: true,
            selected: None,
            options: options
                .iter()
                .map(|name| ProxyOption {
                    name: (*name).to_owned(),
                    region: None,
                    delay_ms: None,
                    alive: None,
                })
                .collect(),
        }
    }

    #[test]
    fn group_delay_plan_covers_inline_items_with_bounded_deduplication() {
        let groups = vec![
            group("ALL", &["a", "b", "c"]),
            group("SUBSET", &["b", "c"]),
            group("OTHER", &["d"]),
        ];
        let selected = delay_refresh_group_indices(&groups).unwrap();
        let names = selected
            .into_iter()
            .map(|index| groups[index].name.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(names, HashSet::from(["ALL", "OTHER"]));
    }

    #[test]
    fn group_delay_response_marks_measured_items_and_missing_items_as_timeout() {
        let mut groups = vec![group("ALL", &["a", "b", "c"])];
        let tested = HashSet::from(["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        let mut measured = HashMap::new();
        merge_group_delay_response(&serde_json::json!({"a": 42, "b": 0}), &mut measured).unwrap();
        apply_group_delay_measurements(&mut groups, &tested, &measured);

        assert_eq!(groups[0].options[0].delay_ms, Some(42));
        assert_eq!(groups[0].options[0].alive, Some(true));
        assert_eq!(groups[0].options[1].delay_ms, None);
        assert_eq!(groups[0].options[1].alive, Some(false));
        assert_eq!(groups[0].options[2].delay_ms, None);
        assert_eq!(groups[0].options[2].alive, Some(false));
    }

    #[test]
    fn cached_group_delays_survive_follow_up_catalog_reads() {
        let mut groups = vec![group("ALL", &["a", "b"]), group("SUBSET", &["b"])];
        let cache = HashMap::from([
            ("a".to_owned(), (Some(42), Some(true))),
            ("b".to_owned(), (None, Some(false))),
        ]);

        assert!(merge_cached_delays(&mut groups, &cache));
        assert_eq!(groups[0].options[0].delay_ms, Some(42));
        assert_eq!(groups[0].options[0].alive, Some(true));
        assert_eq!(groups[0].options[1].delay_ms, None);
        assert_eq!(groups[0].options[1].alive, Some(false));
        assert_eq!(groups[1].options[0].alive, Some(false));
    }

    #[test]
    fn cached_group_delays_are_rejected_when_catalog_changes() {
        let mut groups = vec![group("ALL", &["new-a", "new-b"])];
        let cache = HashMap::from([
            ("old-a".to_owned(), (Some(42), Some(true))),
            ("old-b".to_owned(), (None, Some(false))),
        ]);

        assert!(!merge_cached_delays(&mut groups, &cache));
        assert!(groups[0]
            .options
            .iter()
            .all(|option| option.delay_ms.is_none() && option.alive.is_none()));
    }

    #[test]
    fn proxy_catalog_exposes_only_sanitized_group_fields() {
        let value = serde_json::json!({
            "proxies": {
                "AUTO": {"type":"Selector", "now":"东京 🇯🇵", "all":["东京 🇯🇵", "DIRECT"]},
                "东京 🇯🇵": {"type":"Shadowsocks", "alive":true, "history":[{"time":"secret", "delay":42}], "server":"do-not-expose"},
                "DIRECT": {"type":"Direct", "alive":true, "history":[]}
            }
        });
        let groups = parse_proxy_groups(&value, None).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].selected.as_deref(), Some("东京 🇯🇵"));
        assert_eq!(groups[0].options[0].delay_ms, Some(42));
        assert_eq!(groups[0].options[0].region.as_deref(), Some("日本 (JP)"));
        let json = serde_json::to_string(&groups).unwrap();
        assert!(!json.contains("server"));
        assert!(!json.contains("do-not-expose"));
        assert!(!json.contains("secret"));
    }

    #[test]
    fn provider_history_populates_items_and_builtin_global_is_hidden() {
        let proxies = serde_json::json!({
            "proxies": {
                "GLOBAL": {"type":"Selector", "now":"PROXY", "all":["PROXY", "DIRECT"]},
                "PROXY": {"type":"Selector", "now":"node-a", "all":["node-a", "node-b"]}
            }
        });
        let providers = serde_json::json!({
            "providers": {
                "subscription": {
                    "vehicleType":"File",
                    "proxies":[
                        {"name":"node-a", "alive":true, "history":[{"delay":123}]},
                        {"name":"node-b", "alive":false, "history":[]}
                    ]
                }
            }
        });

        let groups = parse_proxy_groups(&proxies, Some(&providers)).unwrap();

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "PROXY");
        assert_eq!(groups[0].options[0].delay_ms, Some(123));
        assert_eq!(groups[0].options[0].alive, Some(true));
        assert_eq!(groups[0].options[1].delay_ms, None);
        assert_eq!(groups[0].options[1].alive, Some(false));
    }

    #[test]
    fn url_test_group_remains_read_only() {
        let value = serde_json::json!({
            "proxies": {
                "PROXY": {"type":"URLTest", "now":"node-a", "all":["node-a", "node-b"]},
                "node-a": {"type":"Shadowsocks", "alive":true, "history":[]},
                "node-b": {"type":"Shadowsocks", "alive":true, "history":[]}
            }
        });

        let groups = parse_proxy_groups(&value, None).unwrap();

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].kind, ProxyGroupKind::UrlTest);
        assert!(!groups[0].selectable);
        assert_eq!(groups[0].selected.as_deref(), Some("node-a"));
    }

    #[test]
    fn percent_encoding_is_utf8_and_region_inference_is_best_effort() {
        assert_eq!(percent_encode("A/B 东京"), "A%2FB%20%E4%B8%9C%E4%BA%AC");
        assert_eq!(infer_region("香港 HK-01").as_deref(), Some("香港 (HK)"));
        assert_eq!(infer_region("plain node"), None);
    }
}
