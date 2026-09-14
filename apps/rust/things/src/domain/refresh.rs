//! Pure refresh scheduling policy shared by the web portal and unit tests.

pub const VISIBLE_RESOURCE_DELAY_MS: u32 = 2_000;
pub const HIDDEN_RESOURCE_DELAY_MS: u32 = 60_000;
pub const MAX_RESOURCE_BACKOFF_MS: u32 = 30_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefreshSchedule {
    pub delay_ms: u32,
    pub should_refresh: bool,
}

/// Baseline-equivalent policy extracted before changing portal behavior.
///
/// The red tests below intentionally describe the target policy from the page-data plan.
pub const fn refresh_schedule(hidden: bool, _consecutive_failures: u8) -> RefreshSchedule {
    RefreshSchedule {
        delay_ms: if hidden {
            HIDDEN_RESOURCE_DELAY_MS
        } else {
            VISIBLE_RESOURCE_DELAY_MS
        },
        should_refresh: !hidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_refresh_uses_bounded_exponential_backoff() {
        assert_eq!(refresh_schedule(false, 0).delay_ms, 2_000);
        assert_eq!(refresh_schedule(false, 1).delay_ms, 4_000);
        assert_eq!(refresh_schedule(false, 2).delay_ms, 8_000);
        assert_eq!(refresh_schedule(false, 3).delay_ms, 16_000);
        assert_eq!(refresh_schedule(false, 4).delay_ms, 30_000);
        assert_eq!(refresh_schedule(false, 12).delay_ms, 30_000);
        assert!(refresh_schedule(false, 12).should_refresh);
    }

    #[test]
    fn hidden_page_keeps_a_low_frequency_refresh_subscription() {
        let schedule = refresh_schedule(true, 12);
        assert_eq!(schedule.delay_ms, HIDDEN_RESOURCE_DELAY_MS);
        assert!(schedule.should_refresh);
    }
}
