use super::*;

#[test]
fn persisted_deferred_questions_are_bound_into_the_confirmed_snapshot() {
    // Production boundary: persisted clarification records are loaded into the immutable snapshot consumed by confirmation and review.
    // Invariant: deferring an optional decision changes the candidate hash and carries owner, risk, verification, and follow-up details; parser-only tests cannot prove PlanningStore integration.
    // External dependencies: none. Setup cost: one temporary managed-local planning store.
    let root = std::env::temp_dir().join(format!("plan-deferred-context-{}", uuid::Uuid::new_v4()));
    let store = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        &root,
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let (identity, markdown) = identified(&spec());
    let feature_id = "CHG-021";
    let contract = crate::core::workflow::feature_contract(&markdown);
    let before =
        from_feature_with_author(&store, feature_id, &identity, &markdown, &contract).unwrap();
    let question_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut question = crate::core::planning::QuestionV1 {
        uid: question_uid.clone(),
        feature_uid: identity.uid.clone(),
        plan_revision: 4,
        decision_subject: "export owner".into(),
        decision_domain: "ownership".into(),
        origin_requirement_ids: vec!["REQ-001".into()],
        category: crate::core::planning::QuestionCategory::Operations,
        priority: crate::core::planning::QuestionPriority::Optional,
        question: "Who owns the exported file?".into(),
        reason: "Ownership controls support follow-up.".into(),
        options: Vec::new(),
        recommended_option_id: None,
        no_answer_impact: "The workspace owner is assumed.".into(),
        evidence_refs: Vec::new(),
        assumption_if_deferred: Some("Use the workspace owner".into()),
        deferred: None,
        depends_on_question_ids: Vec::new(),
        status: crate::core::planning::QuestionStatus::Open,
        answer_text: None,
        selected_option: None,
        answered_by: None,
        answered_at_ms: None,
        deferred_by: None,
        deferred_at_ms: None,
        revision: 0,
    };
    let questions = crate::core::planning::QuestionStore::new(&store);
    questions.save(&question, 0).unwrap();
    crate::core::planning::defer_question(
        &mut question,
        0,
        4,
        crate::core::planning::DeferredAssumption {
            assumption: "Use the workspace owner".into(),
            owner: "workspace admin".into(),
            risk: "Owner may change before release".into(),
            verification_test: "Confirm owner in workspace settings".into(),
            follow_up_task: "Recheck ownership before rollout".into(),
        },
        "operator",
        10,
    )
    .unwrap();
    let (_, revision) = questions.load(&question_uid).unwrap();
    questions.save(&question, revision).unwrap();
    let after =
        from_feature_with_author(&store, feature_id, &identity, &markdown, &contract).unwrap();
    assert_ne!(before.content_hash, after.content_hash);
    assert_eq!(after.deferred_assumptions.len(), 1);
    for detail in [
        "Use the workspace owner",
        "workspace admin",
        "Owner may change before release",
        "Confirm owner in workspace settings",
        "Recheck ownership before rollout",
    ] {
        assert!(after.deferred_assumptions[0].contains(detail));
    }
    let mut answered = question;
    answered.uid = uuid::Uuid::new_v4().hyphenated().to_string();
    answered.question = "Should exports include an audit label?".into();
    answered.status = crate::core::planning::QuestionStatus::Answered;
    answered.answer_text = Some("Include the workspace label.".into());
    answered.answered_by = Some("operator".into());
    answered.answered_at_ms = Some(20);
    answered.deferred = None;
    answered.deferred_by = None;
    answered.deferred_at_ms = None;
    answered.revision = 0;
    questions.save(&answered, 0).unwrap();
    let answered_snapshot =
        from_feature_with_author(&store, feature_id, &identity, &markdown, &contract).unwrap();
    assert_ne!(after.content_hash, answered_snapshot.content_hash);
    assert!(
        answered_snapshot
            .decisions
            .contains("Include the workspace label.")
    );
    let _ = std::fs::remove_dir_all(root);
}
