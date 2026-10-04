use super::*;

#[test]
fn attention_cards_sort_newest_activity_first_and_keep_fallback_order() {
    let mut entries = vec![
        ("older", Some(10), 0),
        ("newer", Some(20), 1),
        ("no_timestamp_old", None, 2),
        ("no_timestamp_new", None, 3),
    ];
    sort_recency(&mut entries);
    assert_eq!(
        entries.into_iter().map(|entry| entry.0).collect::<Vec<_>>(),
        ["newer", "older", "no_timestamp_old", "no_timestamp_new"]
    );
}

#[test]
fn task_recency_uses_the_latest_persisted_progress_update() {
    let progress = crate::harness::LiveProgress {
        telemetry: crate::harness::ActivityTelemetry {
            started_ms: Some(10),
            updated_ms: Some(30),
            finished_ms: Some(20),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(progress_activity_ms(&progress), Some(30));
}

#[test]
fn recovery_disposition_distinguishes_user_action_from_waiting() {
    use crate::core::implementation::RecoveryDisposition as Recovery;

    assert_eq!(recovery_kind(Recovery::UserAction), Kind::WaitingOnUser);
    assert_eq!(recovery_kind(Recovery::ExplicitResume), Kind::WaitingOnUser);
    assert_eq!(recovery_kind(Recovery::AutomaticRetry), Kind::Blocked);
    assert_eq!(recovery_kind(Recovery::DoNotRetry), Kind::Blocked);
}

#[test]
fn attention_item_is_waiting_on_user_only_when_the_user_can_act() {
    assert_eq!(item_kind(true), Kind::WaitingOnUser);
    assert_eq!(item_kind(false), Kind::Blocked);
}
