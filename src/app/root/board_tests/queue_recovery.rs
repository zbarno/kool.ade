use super::*;

#[test]
fn parked_auto_queue_names_the_missing_feature_approval() {
    let _shield = crate::core::gitops::test_support::shield("auto-queue-park");
    let mut app = park_fixture_with_unapproved_feature(false);
    app.advance_auto_queue();
    let Screen::Connected(p) = &app.screen else {
        panic!("screen disconnected")
    };
    assert!(!p.queue.running, "queue must park, not spin");
    assert!(
        p.active_implementations.is_empty(),
        "no worker may start for an unapproved feature"
    );
    assert!(
        p.queue.last_error.contains("CHG-999"),
        "park message must name the blocking feature id: {}",
        p.queue.last_error
    );
    assert!(
        p.queue.last_error.contains("no recorded approval")
            && p.queue
                .last_error
                .contains("Approve feature for implementation"),
        "park message must state the remedy: {}",
        p.queue.last_error
    );
    assert!(
        p.queue
            .last_error
            .contains(".koolade-packet/planning/tasks/fixture/001-task.md"),
        "park message must name the affected tickets: {}",
        p.queue.last_error
    );
    assert!(
        !p.queue
            .last_error
            .contains(".koolade-packet/planning/tasks/fixture/003-task.md"),
        "completed tasks must not produce stale approval blockers: {}",
        p.queue.last_error
    );
}

#[test]
fn parked_auto_queue_flags_lapsed_approvals_as_reapproval_targets() {
    let _shield = crate::core::gitops::test_support::shield("auto-queue-park-lapsed");
    let mut app = park_fixture_with_unapproved_feature(true);
    app.advance_auto_queue();
    let Screen::Connected(p) = &app.screen else {
        panic!("screen disconnected")
    };
    assert!(!p.queue.running);
    assert!(
        p.queue
            .last_error
            .contains("approval lapsed after the feature document changed")
            && p.queue
                .last_error
                .contains("re-run Approve feature for implementation"),
        "lapsed approval must read as a re-approval target, not a wall: {}",
        p.queue.last_error
    );
}
