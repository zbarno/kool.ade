use super::*;

/// Production boundary: product Specification turns capture and revalidate a
/// source-repository pin before the turn runs. The reader test alone cannot
/// prove an empty product baseline remains a valid, stale-fenced target.
/// Uses a temporary local Git project only; no network, Docker, or SSH.
#[test]
fn unpublished_product_baseline_is_a_valid_pinned_interview_target() {
    let (mut inputs, root) = inputs_for("spec_chat_empty_baseline", "Start a product baseline.");
    git_stdout(&root, &["rm", "-rq", "docs/koolade"]);
    git_stdout(
        &root,
        &["commit", "-m", "Remove published product baseline"],
    );
    inputs.state.resync().unwrap();

    let target = crate::core::specification_chat::capture_target(
        &inputs.state,
        crate::core::specification_chat::Scope::Product,
    )
    .unwrap();
    assert!(target.source_documents.is_empty());
    assert!(!target.effective_document_sha256.is_empty());
    crate::core::specification_chat::validate_target(&inputs.state, &target).unwrap();

    let exact = "This project helps small clinics coordinate follow-up care.";
    inputs.user_message = exact.into();
    inputs.purpose = crate::core::workflow::TurnPurpose::Interview;
    inputs.specification_chat = Some(crate::core::specification_chat::Request {
        text: exact.into(),
        intent: crate::core::specification_chat::Intent::Propose,
        target: target.clone(),
        omitted_history_messages: 0,
        reviewed_repository_evidence: Vec::new(),
    });
    let before_head = git_stdout(&root, &["rev-parse", "HEAD"]);
    let captured = Arc::new(std::sync::Mutex::new(None));
    let outcome = execute::run_turn(
        &inputs,
        &CaptureHarness {
            raw: serde_json::json!({
                "schema_version": 1,
                "assistant_message": "I saved a private first draft. Who will use it most often?",
                "document_updates": [{
                    "document_id": "product:overview",
                    "content": "# Overview\n\n**User-confirmed purpose:** This project helps small clinics coordinate follow-up care.\n\n**Status:** Early draft; other product details are unknown.\n"
                }],
                "open_items_added": [{
                    "kind": "Question",
                    "priority": "Normal",
                    "authority": "Human",
                    "category": "Product",
                    "question": "Who will use this most often?",
                    "reason": "The intended users are not yet confirmed."
                }],
                "open_items_updated": [],
                "open_items_resolved": []
            }).to_string(),
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
    let draft = crate::core::apply::load_current(&inputs.state.planning_store)
        .unwrap()
        .unwrap();
    assert_eq!(draft.base_published_spec_pin, Some(target.published_pin));
    assert!(draft.changes[0].content.contains("helps small clinics"));
    let refreshed =
        crate::core::state::PlannerState::load_with_store(&root, &inputs.state.planning_store)
            .unwrap();
    assert_eq!(refreshed.items.len(), 1);
    assert_eq!(
        refreshed.items[0].authority,
        crate::domain::Authority::Human
    );
    assert!(refreshed.items[0].question.contains("Who will use this"));
    assert_eq!(git_stdout(&root, &["rev-parse", "HEAD"]), before_head);
    assert!(captured.lock().unwrap().as_ref().unwrap().contains(exact));
    assert!(
        captured
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .contains("no published product baseline")
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}

#[test]
fn product_baseline_proposals_can_add_bounded_human_questions_only() {
    let (inputs, root) = inputs_for("spec_chat_baseline_questions", "Clarify product intent.");
    let target = crate::core::specification_chat::capture_target(
        &inputs.state,
        crate::core::specification_chat::Scope::Product,
    )
    .unwrap();
    let mut proposal = turn_with_question(crate::domain::Authority::Human);
    crate::core::specification_chat::validate_proposal(&target, &mut proposal, &inputs.state)
        .unwrap();

    let mut agent_question = turn_with_question(crate::domain::Authority::Agent);
    assert!(
        crate::core::specification_chat::validate_proposal(
            &target,
            &mut agent_question,
            &inputs.state
        )
        .is_err()
    );

    let mut too_many = turn_with_question(crate::domain::Authority::Human);
    too_many.added = (0..6)
        .map(|index| question(format!("CLR-{index:03}")))
        .collect();
    assert!(
        crate::core::specification_chat::validate_proposal(&target, &mut too_many, &inputs.state)
            .is_err()
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}

#[test]
fn reviewed_repository_excerpt_requires_matching_hash_and_current_repository_revision() {
    use sha2::Digest;
    let (inputs, root) = inputs_for("spec_chat_reviewed_evidence", "Use reviewed evidence.");
    let source_sha = git_stdout(&root, &["rev-parse", "HEAD"]).trim().to_owned();
    let content = "Untrusted repository evidence at src/main.py:1: print('synthetic')".to_owned();
    let evidence = crate::domain::chatlog::VerifiedRepositoryEvidence {
        evidence_id: "investigation:synthetic".into(),
        repository_id: "root".into(),
        source_ref: "refs/heads/main".into(),
        source_sha,
        content_sha256: format!("{:x}", sha2::Sha256::digest(content.as_bytes())),
        excerpt_sha256: format!("{:x}", sha2::Sha256::digest(content.as_bytes())),
        content,
    };
    let validate = |items: &[crate::domain::chatlog::VerifiedRepositoryEvidence]| {
        crate::core::specification_chat::validate_reviewed_repository_evidence(
            &root,
            &inputs.state.planning_store,
            &inputs.state.repositories,
            items,
        )
    };
    assert_eq!(validate(std::slice::from_ref(&evidence)), Ok(()));

    let mut altered = evidence.clone();
    altered.content.push_str(" stale");
    assert!(validate(&[altered]).is_err());

    let mut stale = evidence;
    stale.source_sha = "b".repeat(40);
    assert!(validate(&[stale]).is_err());

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(root.with_extension("private-state"));
}

fn turn_with_question(
    authority: crate::domain::Authority,
) -> crate::core::validation::NormalizedTurn {
    let mut turn = crate::core::validation::NormalizedTurn {
        assistant_message: "A local choice needs your input.".into(),
        change_summary: None,
        document_updates: vec![("product:overview".into(), "# Overview\n\nDraft.\n".into())],
        planning_tasks: Vec::new(),
        task_intent_context: None,
        change_status_updates: Default::default(),
        additional_planning_artifacts: Vec::new(),
        added: vec![question("CLR-001".into())],
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
    turn.added[0].authority = authority;
    turn
}

fn question(id: String) -> crate::domain::OpenItem {
    crate::domain::OpenItem::new(
        id,
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "Product".into(),
        "Who is the primary user for this project?".into(),
        "The intended user needs confirmation.".into(),
    )
}
