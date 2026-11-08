use super::*;

/// Production boundary: a UI adoption request reaches the local Manager event
/// queue, where the existing revision-bound Manager workspace can authorize
/// #487 evidence. Testing the broker alone cannot prove the visible action
/// enters that path. Uses only a temporary local project; no network, Docker,
/// or SSH.
#[test]
fn baseline_code_review_action_queues_a_local_manager_investigation() {
    let mut app = crate::app::root::board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let old_len = project.activity.pending.len();

    app.dispatch_ui_command(
        crate::ui::ApplicationCommand::RequestProductBaselineEvidence {
            review_id: uuid::Uuid::new_v4().to_string(),
            repository_id: "root".into(),
        },
    );

    let Screen::Connected(project) = &app.screen else {
        unreachable!()
    };
    assert_eq!(project.activity.pending.len(), old_len + 1);
    let event = project.activity.pending.last().unwrap();
    assert!(event.starts_with("Product baseline evidence review:"));
    assert!(!event.contains("baseline_review_id="));
    assert!(event.contains("bounded koolade_investigate evidence"));
    assert!(event.contains("commit SHA, path, and test references"));
    assert_eq!(
        project
            .activity
            .pending_baseline_review
            .as_ref()
            .unwrap()
            .repository_id,
        "root"
    );
}
