use super::super::*;

pub(crate) fn reviewed_feature(
    fixture: &Fixture,
    store: &PlanningStore,
    feature_id: &str,
    folder: &str,
) -> (String, String) {
    let markdown = crate::domain::ArtifactIdentity::preserve_markdown(
        &format!(
            "# {feature_id}: Trace fixture\n\n**Status:** Implementing\n\n## Intent\n\nPersist an explicit trace.\n\n## Current Behavior\n\nNo persisted trace.\n\n## Desired Behavior\n\nTrace each requirement to a test.\n\n## Scope\n\nTraceability.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\n- Requirement has a primary test mapping.\n- Requirement has an independent test mapping.\n\n## Decisions and Assumptions\n\nUse durable records.\n\n## Acceptance Criteria\n\n- Test reference resolves to its requirement.\n- Decisions are scoped to their requirement.\n"
        ),
        None,
        feature_id,
        "Trace fixture",
    )
    .unwrap();
    for (key, value) in [
        ("user.name", "Fixture"),
        ("user.email", "fixture@example.test"),
    ] {
        let status = std::process::Command::new("git")
            .args(["config", key, value])
            .current_dir(&fixture.0)
            .status()
            .unwrap();
        assert!(status.success());
    }
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .unwrap();
    let markdown = crate::domain::ChangeMetadata::write_markdown(
        &markdown,
        &identity,
        crate::domain::ChangeStatus::Implementing,
    )
    .unwrap();
    let path = fixture.0.join(format!(
        ".koolade-packet/planning/changes/{folder}/specification.md"
    ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &markdown).unwrap();
    let mut workflow = crate::core::workflow::Workflow::default();
    crate::artifacts::task_docs::save_workflow(&fixture.0, &workflow).unwrap();
    let output = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(&fixture.0)
        .status()
        .unwrap();
    assert!(output.success());
    let output = std::process::Command::new("git")
        .args(["commit", "-qm", "approved trace fixture"])
        .current_dir(&fixture.0)
        .status()
        .unwrap();
    assert!(output.success());
    crate::core::lifecycle::test_support::approve_and_review_feature(
        store,
        &mut workflow,
        feature_id,
        &markdown,
    )
    .unwrap();
    (identity.uid, markdown)
}

pub(crate) fn answered_decision(
    store: &PlanningStore,
    feature_uid: &str,
    requirement_id: &str,
    subject: &str,
) -> String {
    let uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let question = crate::core::planning::QuestionV1 {
        uid: uid.clone(),
        feature_uid: feature_uid.to_owned(),
        plan_revision: 1,
        decision_subject: subject.into(),
        decision_domain: "traceability".into(),
        origin_requirement_ids: vec![requirement_id.to_owned()],
        category: crate::core::planning::QuestionCategory::Testing,
        priority: crate::core::planning::QuestionPriority::Important,
        question: format!("Choose the decision for {subject}"),
        reason: "The choice affects the requirement trace.".into(),
        options: vec![crate::core::planning::QuestionOption {
            id: "retain".into(),
            label: "Retain explicit trace".into(),
            tradeoffs: "Keeps the reference auditable.".into(),
        }],
        recommended_option_id: Some("retain".into()),
        no_answer_impact: "The mapping remains unresolved.".into(),
        evidence_refs: vec![format!("plan:{feature_uid}")],
        assumption_if_deferred: None,
        deferred: None,
        depends_on_question_ids: Vec::new(),
        status: crate::core::planning::QuestionStatus::Answered,
        answer_text: Some("Retain the explicit mapping.".into()),
        selected_option: Some("retain".into()),
        answered_by: Some("Fixture operator".into()),
        answered_at_ms: Some(1),
        revision: 0,
    };
    crate::core::planning::QuestionStore::new(store)
        .save(&question, 0)
        .unwrap();
    uid
}
