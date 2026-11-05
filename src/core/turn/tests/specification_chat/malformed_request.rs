use super::*;
use std::sync::mpsc::channel;

/// Production boundary: execute a captured Specification Chat request through
/// the turn runner. A unit test of `mode_matches` cannot prove malformed
/// bindings are rejected before provider invocation or mutation. Uses a local
/// Git fixture and capture harness only; no network, Docker, or SSH (low setup
/// cost).
#[test]
fn mismatched_user_text_and_captured_request_are_rejected_before_provider() {
    let (mut inputs, root) = inputs_for("spec_chat_malformed_request", "initial text");
    let target = target(&inputs);
    inputs.purpose = crate::core::workflow::TurnPurpose::Question;
    inputs.user_message = "User submitted a question.".into();
    inputs.specification_chat = Some(crate::core::specification_chat::Request {
        text: "Different text would be sent to the provider.".into(),
        intent: crate::core::specification_chat::Intent::Ask,
        target,
        omitted_history_messages: 0,
    });

    let before_head = git_stdout(&root, &["rev-parse", "HEAD"]);
    let captured = Arc::new(std::sync::Mutex::new(None));
    let outcome = super::super::super::execute::run_turn_read_only(
        &inputs,
        &CaptureHarness {
            raw: question_envelope(),
            sink: captured.clone(),
        },
        &Arc::new(std::sync::atomic::AtomicBool::new(false)),
        channel().0,
        None,
        None,
        Duration::from_secs(30),
    );

    assert!(matches!(outcome, TurnOutcome::Rejected { .. }));
    assert!(captured.lock().unwrap().is_none());
    assert!(
        crate::core::apply::load_current(&inputs.state.planning_store)
            .unwrap()
            .is_none()
    );
    assert_eq!(git_stdout(&root, &["rev-parse", "HEAD"]), before_head);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}
