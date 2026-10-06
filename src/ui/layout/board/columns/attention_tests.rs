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
fn attention_callout_uses_theme_specific_contrast_pairings() {
    let (dark_text, dark_fill) = user_action_colors(true);
    let (light_text, light_fill) = user_action_colors(false);
    assert_ne!(dark_text, light_text);
    assert_ne!(dark_fill, light_fill);
    assert_ne!(light_text, light_fill);
}

#[test]
fn linked_task_attention_requires_an_open_actionable_item_for_same_feature() {
    let mut board = crate::ui::planning_board::ViewModel::default();
    let mut item = crate::domain::item::OpenItem::new(
        "CLR-012".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "Product".into(),
        None,
        "Choose the release behavior".into(),
        "The implementation needs this decision".into(),
    );
    item.feature_id = Some("CHG-012".into());
    board.eligible_item_ids.insert(item.id.clone());
    board.planning_items.push(item);

    let path = ".koolade-packet/planning/tasks/CHG-012-release/CHG-012-TASK-001-release.md";
    assert_eq!(linked_user_action(&board, path).unwrap().id, "CLR-012");

    board.planning_items[0].status = crate::domain::ItemStatus::Resolved;
    assert!(linked_user_action(&board, path).is_none());
    board.planning_items[0].status = crate::domain::ItemStatus::Open;
    board.eligible_item_ids.clear();
    assert!(linked_user_action(&board, path).is_none());
}

#[test]
fn linked_task_attention_does_not_cross_feature_boundaries() {
    let mut board = crate::ui::planning_board::ViewModel::default();
    let mut item = crate::domain::item::OpenItem::new(
        "CLR-013".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "Product".into(),
        None,
        "Choose a behavior".into(),
        "Context".into(),
    );
    item.feature_id = Some("F13".into());
    board.eligible_item_ids.insert(item.id.clone());
    board.planning_items.push(item);

    assert!(
        linked_user_action(
            &board,
            ".koolade-packet/planning/tasks/CHG-012-release/CHG-012-TASK-001.md"
        )
        .is_none()
    );
}

#[test]
fn running_waiting_failed_review_and_resumed_work_project_to_distinct_columns() {
    use crate::core::implementation::{ImplementationStatus as Status, board_column};

    assert_eq!(
        board_column(Some(&implementation(Status::Implementing)), true),
        1
    );
    assert_eq!(
        board_column(Some(&implementation(Status::Blocked)), false),
        3
    );
    assert_eq!(
        board_column(Some(&implementation(Status::AwaitingReview)), false),
        2
    );
    assert_eq!(
        board_column(Some(&implementation(Status::Interrupted)), false),
        3
    );
    assert_eq!(
        board_column(Some(&implementation(Status::Implementing)), false),
        1
    );
}

fn implementation(
    status: crate::core::implementation::ImplementationStatus,
) -> crate::core::implementation::Implementation {
    crate::core::implementation::Implementation {
        ticket: "TASK-001".into(),
        task_uid: None,
        ticket_text: String::new(),
        approved_specification: None,
        approved_product_context: None,
        completed_dependency_context: None,
        branch: String::new(),
        source_branch: None,
        destination_branch: None,
        base: String::new(),
        base_commit: String::new(),
        worktree: std::path::PathBuf::new(),
        status,
        detail: String::new(),
        pr_url: None,
        verified_head: None,
        auto_merge: false,
        merged_commit: None,
        pr_state: None,
        pr_checked_at: None,
        pr_check_attempted_at: None,
        pr_check_error: None,
        independent_check: None,
        cleanup: Default::default(),
    }
}
