use super::*;

#[test]
fn typed_image_suggestion_waits_for_a_user_review_action() {
    let directory = tempfile::tempdir().unwrap();
    crate::persistence::bootstrap_conversations::BootstrapConversationStore::with_test_directory(
        directory.path(),
        || {
            let current = task(
                "setup:images:rust:amd64:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                crate::persistence::setup_tasks::State::ReadyToFix,
            );
            let mut controller = Controller::default();
            *controller.draft_mut(&current.id).unwrap() = "What is the image setup step?".into();
            controller
                .start(
                    &current,
                    crate::app::setup_attention::catalog::HostKind::current(),
                    Box::new(lifecycle::FakeTurnHarness {
                        route: lifecycle::fake_route(),
                        response: r#"{"schemaVersion":1,"reply":"repeat_next_step","suggestion":{"operation":"prepare_locked_image","imageProfile":"rust"}}"#.into(),
                        entered: None,
                        release: None,
                    }),
                )
                .unwrap();
            let completed = lifecycle::complete_turns(&mut controller).pop().unwrap();
            controller.finish(completed, Some(&current), Some(lifecycle::fake_route()));

            let view = controller.view(&current.id);
            assert!(!view.busy);
            assert!(matches!(
                view.suggestion.unwrap().operation,
                crate::ui::planning_board::BootstrapSuggestionOperation::PrepareLockedImage {
                    profile
                } if profile == "rust"
            ));
            // The controller only stages the typed suggestion. Execution remains
            // behind ApplyBootstrapSuggestion and the existing approval broker.
        },
    );
}
