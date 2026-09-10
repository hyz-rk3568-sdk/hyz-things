use super::*;

#[test]
fn countdown_snapshot_tracks_future_target() {
    let snapshot = countdown_snapshot(1_000, 0, 10_000);

    assert_eq!(snapshot.remaining_seconds, 9);
    assert_eq!(snapshot.days, 0);
    assert_eq!(snapshot.hours, 0);
    assert_eq!(snapshot.minutes, 0);
    assert_eq!(snapshot.seconds, 9);
    assert_eq!(snapshot.progress_percent, 10);
    assert!(!snapshot.finished);
}

#[test]
fn countdown_snapshot_clamps_past_target() {
    let snapshot = countdown_snapshot(15_000, 0, 10_000);

    assert_eq!(snapshot.remaining_seconds, 0);
    assert_eq!(snapshot.progress_percent, 100);
    assert!(snapshot.finished);
}

#[test]
fn countdown_snapshot_handles_target_boundary_without_early_finish() {
    let just_before = countdown_snapshot(9_999, 0, 10_000);
    assert_eq!(just_before.remaining_seconds, 1);
    assert_eq!(just_before.progress_percent, 99);
    assert!(!just_before.finished);

    let at_target = countdown_snapshot(10_000, 0, 10_000);
    assert_eq!(at_target.remaining_seconds, 0);
    assert_eq!(at_target.progress_percent, 100);
    assert!(at_target.finished);
}

#[test]
fn custom_countdown_round_trips_duration_and_idle_storage() {
    let total = custom_duration_from_parts(1, 2, 3).expect("valid custom duration");
    assert_eq!(total, 3_723);
    assert_eq!(custom_duration_parts(total), (1, 2, 3));

    let state = decode_custom_countdown_state("1|3723|idle").expect("valid stored state");
    assert_eq!(encode_custom_countdown_state(state), "1|3723|idle");

    let snapshot = custom_countdown_snapshot(123_456, state);
    assert_eq!(snapshot.remaining_seconds, 3_723);
    assert_eq!(snapshot.hours, 1);
    assert_eq!(snapshot.minutes, 2);
    assert_eq!(snapshot.seconds, 3);
    assert_eq!(snapshot.progress_percent, 0);
    assert!(!snapshot.finished);
}

#[test]
fn invalid_custom_storage_data_is_rejected() {
    for invalid in [
        "",
        "2|1500|idle",
        "1|0|idle",
        "1|1500|idle|extra",
        "1|1500|running|0|0|1000|0|0",
        "1|1500|paused|10|0|1000|11|50",
        "1|1500|paused|10|0|1000|5|101",
        "1|not-a-number|idle",
    ] {
        assert!(
            decode_custom_countdown_state(invalid).is_none(),
            "invalid state should be rejected: {invalid}"
        );
    }

    assert!(decode_custom_countdowns("2;1|60|idle;1|60|idle").is_none());
    assert!(decode_custom_countdowns("2;1|60|idle;1|60|idle;1|60|idle;extra").is_none());
}
