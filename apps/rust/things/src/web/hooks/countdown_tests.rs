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
