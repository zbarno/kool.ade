use super::*;
use std::sync::mpsc::channel;

fn assert_rejected_before_provider(inputs: &TurnInputs, root: &std::path::Path, read_only: bool) {
    let before_head = git_stdout(root, &["rev-parse", "HEAD"]);
    let captured = Arc::new(std::sync::Mutex::new(None));
    let harness = CaptureHarness {
        raw: question_envelope(),
        sink: captured.clone(),
    };
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let outcome = if read_only {
        super::super::super::execute::run_turn_read_only(
            inputs,
            &harness,
            &cancel,
            channel().0,
            None,
            None,
            Duration::from_secs(30),
        )
    } else {
        super::super::super::execute::run_turn(
            inputs,
            &harness,
            &cancel,
            channel().0,
            None,
            None,
            Duration::from_secs(30),
        )
    };

    assert!(matches!(outcome, TurnOutcome::Rejected { .. }));
    assert!(captured.lock().unwrap().is_none());
    assert!(
        crate::core::apply::load_current(&inputs.state.planning_store)
            .unwrap()
            .is_none()
    );
    assert_eq!(git_stdout(root, &["rev-parse", "HEAD"]), before_head);
}

fn remove_fixture(root: &std::path::Path) {
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}

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

    assert_rejected_before_provider(&inputs, &root, true);
    remove_fixture(&root);
}

/// Production boundary: a captured Propose intent cannot be sent through the
/// read-only Ask path. A unit test of the mode predicate cannot prove early
/// rejection prevents provider use and all document or Git mutation. Uses a
/// local Git fixture and capture harness only; no network, Docker, or SSH (low
/// setup cost).
#[test]
fn proposal_intent_in_read_only_turn_is_rejected_before_provider() {
    let (mut inputs, root) = inputs_for("spec_chat_malformed_intent", "Explain the product.");
    let target = target(&inputs);
    inputs.purpose = crate::core::workflow::TurnPurpose::Question;
    inputs.specification_chat = Some(crate::core::specification_chat::Request {
        text: inputs.user_message.clone(),
        intent: crate::core::specification_chat::Intent::Propose,
        target,
        omitted_history_messages: 0,
    });

    assert_rejected_before_provider(&inputs, &root, true);
    remove_fixture(&root);
}

/// Production boundary: malformed captured source identity must fail base
/// snapshot validation before the provider receives a prompt. A validator-only
/// test cannot prove the turn runner stops before provider use and mutation.
/// Uses a local Git fixture and capture harness only; no network, Docker, or
/// SSH (low setup cost).
#[test]
fn tampered_source_identity_is_rejected_before_provider() {
    let (mut inputs, root) = inputs_for("spec_chat_malformed_source", "Explain the product.");
    let mut target = target(&inputs);
    target.source_documents[0].sha256 = "f".repeat(64);
    inputs.purpose = crate::core::workflow::TurnPurpose::Question;
    inputs.specification_chat = Some(crate::core::specification_chat::Request {
        text: inputs.user_message.clone(),
        intent: crate::core::specification_chat::Intent::Ask,
        target,
        omitted_history_messages: 0,
    });

    assert_rejected_before_provider(&inputs, &root, true);
    remove_fixture(&root);
}

/// Production boundary: an Ask request must not enter the mutating turn path.
/// A predicate-only test cannot prove the turn runner prevents provider use,
/// draft creation, or Git mutation. Uses a local Git fixture and capture
/// harness; no network, Docker, or SSH (low setup cost).
#[test]
fn ask_intent_in_mutating_turn_is_rejected_before_provider() {
    let (mut inputs, root) = inputs_for("spec_chat_malformed_ask_mode", "Explain the product.");
    let target = target(&inputs);
    inputs.purpose = crate::core::workflow::TurnPurpose::Question;
    inputs.specification_chat = Some(crate::core::specification_chat::Request {
        text: inputs.user_message.clone(),
        intent: crate::core::specification_chat::Intent::Ask,
        target,
        omitted_history_messages: 0,
    });

    assert_rejected_before_provider(&inputs, &root, false);
    remove_fixture(&root);
}

/// Production boundary: the provider prompt must not be built from a request
/// whose pinned source path differs from the captured target. A source-hash
/// test alone does not cover path substitution. This exercises turn-runner
/// rejection, before provider use or mutation, with only a local Git fixture
/// and capture harness (low setup cost; no network, Docker, or SSH).
#[test]
fn tampered_source_path_is_rejected_before_provider() {
    let (mut inputs, root) = inputs_for("spec_chat_malformed_path", "Explain the product.");
    let mut target = target(&inputs);
    target.source_documents[0].path.push_str(".tampered");
    inputs.purpose = crate::core::workflow::TurnPurpose::Question;
    inputs.specification_chat = Some(crate::core::specification_chat::Request {
        text: inputs.user_message.clone(),
        intent: crate::core::specification_chat::Intent::Ask,
        target,
        omitted_history_messages: 0,
    });

    assert_rejected_before_provider(&inputs, &root, true);
    remove_fixture(&root);
}
