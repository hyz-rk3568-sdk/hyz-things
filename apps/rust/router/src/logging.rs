//! Structured syslog events emitted by the headless router daemon.

#[cfg(test)]
use std::fmt::Write as _;

pub const SERVICE: &str = "hyz-router";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    #[cfg(test)]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

pub fn init() -> Result<(), Box<dyn std::error::Error>> {
    hyz_logging::init(SERVICE)
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
    let error_code = error_code.unwrap_or("");
    let request_id = request_id.unwrap_or("");
    let status = status.unwrap_or_default();
    let duration_ms = duration_ms.unwrap_or_default();
    match level {
        Level::Info => tracing::info!(
            target: SERVICE,
            service = SERVICE,
            component,
            operation,
            event,
            error_code,
            request_id,
            status,
            duration_ms,
        ),
        Level::Warn => tracing::warn!(
            target: SERVICE,
            service = SERVICE,
            component,
            operation,
            event,
            error_code,
            request_id,
            status,
            duration_ms,
        ),
        Level::Error => tracing::error!(
            target: SERVICE,
            service = SERVICE,
            component,
            operation,
            event,
            error_code,
            request_id,
            status,
            duration_ms,
        ),
    }
}

pub fn message(
    level: Level,
    component: &str,
    operation: &str,
    event: &str,
    error_code: &str,
    message: impl std::fmt::Display,
) {
    let error_code = error_code.to_owned();
    match level {
        Level::Info => tracing::info!(
            target: SERVICE,
            service = SERVICE,
            component,
            operation,
            event,
            error_code,
            message = %message,
        ),
        Level::Warn => tracing::warn!(
            target: SERVICE,
            service = SERVICE,
            component,
            operation,
            event,
            error_code,
            message = %message,
        ),
        Level::Error => tracing::error!(
            target: SERVICE,
            service = SERVICE,
            component,
            operation,
            event,
            error_code,
            message = %message,
        ),
    }
}

#[cfg(test)]
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
        "level={} service={SERVICE} component={component:?} operation={operation:?} event={event:?}",
        level.as_str()
    );
    if let Some(error_code) = error_code {
        let _ = write!(line, " error_code={error_code:?}");
    }
    if let Some(request_id) = request_id {
        let _ = write!(line, " request_id={request_id:?}");
    }
    if let Some(status) = status {
        let _ = write!(line, " status={status}");
    }
    if let Some(duration_ms) = duration_ms {
        let _ = write!(line, " duration_ms={duration_ms}");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_event_contains_service_and_stable_fields() {
        let line = format_event(
            Level::Error,
            "network",
            "reconcile",
            "operation_failed",
            Some("network_reconcile_failed"),
            None,
            None,
            None,
        );
        assert!(line.contains("service=hyz-router"));
        assert!(line.contains("component=\"network\""));
        assert!(line.contains("error_code=\"network_reconcile_failed\""));
    }

    #[test]
    fn logging_source_contains_no_direct_stderr_calls() {
        for source in [
            include_str!("main.rs"),
            include_str!("adapters/outbound/management.rs"),
        ] {
            let production = source.split("#[cfg(test)]").next().unwrap();
            assert!(!production.contains("eprintln!("));
        }
    }
}
