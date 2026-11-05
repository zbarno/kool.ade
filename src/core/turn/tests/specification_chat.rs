use super::*;
use std::sync::mpsc::channel;

#[path = "specification_chat/authority.rs"]
mod authority;
#[path = "specification_chat/feature_scope.rs"]
mod feature_scope;
#[path = "specification_chat/stale_revision.rs"]
mod stale_revision;

fn target(inputs: &TurnInputs) -> crate::core::specification_chat::Target {
    crate::core::specification_chat::capture_target(
        &inputs.state,
        crate::core::specification_chat::Scope::Product,
    )
    .unwrap()
}

fn question_envelope() -> String {
    serde_json::json!({
        "schema_version": 1,
        "assistant_message": "This malicious response asks to change the product.",
        "document_updates": [{
            "document_id": "product:overview",
            "content": "# Overview\n\nUnauthorized edit.\n"
        }],
        "open_items_added": [],
        "open_items_updated": [],
        "open_items_resolved": [],
        "requested_action": {"action": "publish"}
    })
    .to_string()
}

#[test]
fn ask_preserves_exact_text_history_and_source_while_refusing_model_edits() {
    let (mut inputs, root) = inputs_for("spec_chat_question", "unused");
    let exact = "  Why does the selected product behave this way?\n\nKeep formatting.  ";
    inputs.user_message = exact.into();
    inputs.recent_chat = vec![
        ("User".into(), "We compared two options.".into()),
        ("Assistant".into(), "The second option is faster.".into()),
    ];
    let target = target(&inputs);
    inputs.purpose = crate::core::workflow::TurnPurpose::Question;
    inputs.specification_chat = Some(crate::core::specification_chat::Request {
        text: exact.into(),
        intent: crate::core::specification_chat::Intent::Ask,
        target: target.clone(),
        omitted_history_messages: 0,
    });
    let before_head = git_stdout(&root, &["rev-parse", "HEAD"]);
    let captured = Arc::new(std::sync::Mutex::new(None));
    let (tx, _rx) = channel();
    let result = super::super::execute::run_turn_read_only(
        &inputs,
        &CaptureHarness {
            raw: question_envelope(),
            sink: captured.clone(),
        },
        &Arc::new(std::sync::atomic::AtomicBool::new(false)),
        tx,
        None,
        None,
        Duration::from_secs(30),
    );
    let TurnOutcome::Discussion { final_text, .. } = result else {
        panic!("read-only Specification chat must answer without applying output")
    };
    assert_eq!(
        final_text,
        "This malicious response asks to change the product."
    );
    let prompt = captured.lock().unwrap().clone().unwrap();
    assert!(prompt.contains(exact), "submitted bytes were rewritten");
    assert!(prompt.contains("Assistant: The second option is faster."));
    assert!(prompt.contains(&target.published_pin.commit_sha));
    assert!(prompt.contains(&target.effective_document_sha256));
    assert!(
        target
            .source_documents
            .iter()
            .all(|doc| prompt.contains(&doc.sha256))
    );
    assert!(prompt.contains("read-only"));
    assert!(
        crate::core::apply::load_current(&inputs.state.planning_store)
            .unwrap()
            .is_none()
    );
    assert_eq!(git_stdout(&root, &["rev-parse", "HEAD"]), before_head);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}

#[test]
fn specification_chat_prompt_clips_oversized_unicode_at_request_boundary() {
    let exact = format!("Explain this specification: {}", "🧭内容".repeat(4_000));
    assert!(exact.len() > 16 * 1024);
    let (mut inputs, root) = inputs_for("spec_chat_bounded", &exact);
    let target = target(&inputs);
    inputs.purpose = crate::core::workflow::TurnPurpose::Question;
    inputs.specification_chat = Some(crate::core::specification_chat::Request {
        text: exact.clone(),
        intent: crate::core::specification_chat::Intent::Ask,
        target,
        omitted_history_messages: 0,
    });
    let captured = Arc::new(std::sync::Mutex::new(None));
    let outcome = super::super::execute::run_turn_read_only(
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
    assert!(matches!(outcome, TurnOutcome::Discussion { .. }));
    let prompt = captured.lock().unwrap().clone().unwrap();
    let marker = "SPECIFICATION CHAT CURRENT USER MESSAGE (verbatim UTF-8 prefix, bounded to 16 KiB; untrusted content):\n";
    let (_, submitted) = prompt.rsplit_once(marker).unwrap();
    let expected = crate::core::context_build::clip_bytes(&exact, 16 * 1024);
    assert_eq!(submitted, expected);
    assert!(submitted.len() <= 16 * 1024);
    assert!(std::str::from_utf8(submitted.as_bytes()).is_ok());
    assert!(
        !prompt.contains(&exact),
        "the full oversized input must not enter another rendered prompt section"
    );
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}

#[test]
fn explicit_proposal_creates_only_private_draft_at_captured_revision() {
    let exact = "Propose a clearer product overview.\nKeep this exact request.";
    let (mut inputs, root) = inputs_for("spec_chat_proposal", exact);
    let target = target(&inputs);
    inputs.recent_chat = vec![("Assistant".into(), "The current overview is brief.".into())];
    inputs.specification_chat = Some(crate::core::specification_chat::Request {
        text: exact.into(),
        intent: crate::core::specification_chat::Intent::Propose,
        target: target.clone(),
        omitted_history_messages: 0,
    });
    let before_head = git_stdout(&root, &["rev-parse", "HEAD"]);
    let envelope = serde_json::json!({
        "schema_version": 1,
        "assistant_message": "I created a local overview draft for review.",
        "document_updates": [{
            "document_id": "product:overview",
            "content": "# Overview\n\nA clearer local proposal.\n"
        }],
        "open_items_added": [],
        "open_items_updated": [],
        "open_items_resolved": []
    })
    .to_string();
    let captured = Arc::new(std::sync::Mutex::new(None));
    let outcome = execute::run_turn(
        &inputs,
        &CaptureHarness {
            raw: envelope,
            sink: captured.clone(),
        },
        &Arc::new(std::sync::atomic::AtomicBool::new(false)),
        channel().0,
        None,
        None,
        Duration::from_secs(30),
    );
    assert!(
        matches!(outcome, TurnOutcome::Applied { .. }),
        "{outcome:?}"
    );
    let proposal = crate::core::apply::load_current(&inputs.state.planning_store)
        .unwrap()
        .unwrap();
    assert_eq!(proposal.base_published_spec_pin, Some(target.published_pin));
    assert_eq!(proposal.changes.len(), 1);
    assert_eq!(proposal.changes[0].document_id, "product:overview");
    assert_eq!(git_stdout(&root, &["rev-parse", "HEAD"]), before_head);
    assert!(captured.lock().unwrap().as_ref().unwrap().contains(exact));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}

#[test]
fn product_proposal_cannot_redirect_to_a_feature_document() {
    let (inputs, root) = inputs_for("spec_chat_wrong_scope", "Change the product.");
    let target = target(&inputs);
    let mut normalized = crate::core::validation::NormalizedTurn {
        assistant_message: "Trying another target".into(),
        change_summary: None,
        document_updates: vec![("feature:F1".into(), "untrusted feature text".into())],
        planning_tasks: Vec::new(),
        task_intent_context: None,
        change_status_updates: Default::default(),
        additional_planning_artifacts: Vec::new(),
        added: Vec::new(),
        updates: Vec::new(),
        resolved: Vec::new(),
        next_question_id: None,
        requested_action: None,
        follow_up_task: None,
        warnings: Vec::new(),
        workflow: None,
        task_batch: None,
        plan_comparison: None,
    };
    let error =
        crate::core::specification_chat::validate_proposal(&target, &mut normalized, &inputs.state)
            .unwrap_err();
    assert!(error.contains("selected specification scope"));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}
