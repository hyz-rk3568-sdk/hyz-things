//! Pure refresh scheduling policy shared by the web portal and unit tests.

pub const VISIBLE_RESOURCE_DELAY_MS: u32 = 2_000;
pub const HIDDEN_RESOURCE_DELAY_MS: u32 = 60_000;
pub const MAX_RESOURCE_BACKOFF_MS: u32 = 30_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefreshSchedule {
    pub delay_ms: u32,
    pub should_refresh: bool,
}

pub const fn refresh_schedule(hidden: bool, consecutive_failures: u8) -> RefreshSchedule {
    if hidden {
        return RefreshSchedule {
            delay_ms: HIDDEN_RESOURCE_DELAY_MS,
            should_refresh: true,
        };
    }

    let shift = if consecutive_failures > 4 {
        4
    } else {
        consecutive_failures
    };
    let exponential = VISIBLE_RESOURCE_DELAY_MS.saturating_mul(1_u32 << shift);
    RefreshSchedule {
        delay_ms: if exponential > MAX_RESOURCE_BACKOFF_MS {
            MAX_RESOURCE_BACKOFF_MS
        } else {
            exponential
        },
        should_refresh: true,
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
