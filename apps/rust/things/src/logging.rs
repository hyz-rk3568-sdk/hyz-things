//! Structured stderr events for the native management portal.
//!
//! The device init system already captures stderr, so the portal keeps that transport while
//! making the payload queryable and stable. Dynamic values are Debug-escaped and this API does
//! not accept arbitrary error detail, request bodies, credentials, URLs, or headers.

use std::io::Write;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn event(
    level: Level,
    component: &str,
    operation: &str,
    event: &str,
    error_code: Option<&str>,
    request_id: Option<&str>,
    status: Option<u16>,
    duration_ms: Option<u128>,
) {
    let line = format_event(
        level,
        component,
        operation,
        event,
        error_code,
        request_id,
        status,
        duration_ms,
    );
    let _ = writeln!(std::io::stderr().lock(), "{line}");
}

#[allow(clippy::too_many_arguments)]
fn format_event(
    level: Level,
    component: &str,
    operation: &str,
    event: &str,
    error_code: Option<&str>,
    request_id: Option<&str>,
    status: Option<u16>,
    duration_ms: Option<u128>,
) -> String {
    let mut line = format!(
        "level={} service=hyz-things component={component:?} operation={operation:?} event={event:?}",
        level.as_str()
    );
    if let Some(error_code) = error_code {
        line.push_str(&format!(" error_code={error_code:?}"));
    }
    if let Some(request_id) = request_id {
        line.push_str(&format!(" request_id={request_id:?}"));
    }
    if let Some(status) = status {
        line.push_str(&format!(" status={status}"));
    }
    if let Some(duration_ms) = duration_ms {
        line.push_str(&format!(" duration_ms={duration_ms}"));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_event_has_stable_fields_and_escapes_dynamic_values() {
        let line = format_event(
            Level::Warn,
            "http",
            "/api/v1/control/example\nforged=true",
            "request_rejected",
            Some("network_conflict"),
            Some("0123456789abcdef"),
            Some(409),
            Some(12),
        );
        assert!(line.contains("level=WARN service=hyz-things"));
        assert!(line.contains("component=\"http\""));
        assert!(line.contains("error_code=\"network_conflict\""));
        assert!(line.contains("request_id=\"0123456789abcdef\""));
        assert!(line.contains("status=409 duration_ms=12"));
        assert!(!line.contains("\nforged=true"));
        assert!(line.contains("\\nforged=true"));
    }

    #[test]
    fn logging_contract_has_no_secret_detail_channel() {
        let source = include_str!("logging.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        for forbidden in [
            "password",
            "cookie",
            "csrf_token",
            "login_url",
            "request_body",
        ] {
            assert!(!production.contains(forbidden));
        }
    }
}
