use super::super::*;

pub(crate) fn reviewed_feature(
    fixture: &Fixture,
    store: &mut PlanningStore,
    feature_id: &str,
    _folder: &str,
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
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    if crate::core::apply::load_current(store).unwrap().is_some() {
        let proposal = crate::core::apply::private_document_draft_changes(
            store,
            &[(format!("feature:{feature_id}"), markdown.clone())],
        )
        .unwrap();
        store.transaction(&proposal, None).unwrap();
    } else {
        let published_feature_root = fixture.0.join("docs/koolade/features");
        std::fs::create_dir_all(&published_feature_root).unwrap();
        let path = published_feature_root.join(format!("{}.md", identity.uid));
        std::fs::write(&path, &markdown).unwrap();
        let index_path = published_feature_root.join("AGENTS.md");
        let mut navigation =
            std::fs::read_to_string(&index_path).unwrap_or_else(|_| "# Navigation\n".into());
        if !navigation.ends_with('\n') {
            navigation.push('\n');
        }
        navigation.push_str(&format!(
            "\n- [{feature_id}: Trace fixture](./{}.md) - Synthetic published feature.\n",
            identity.uid
        ));
        std::fs::write(index_path, navigation).unwrap();
        let output = std::process::Command::new("git")
            .args(["add", "docs/koolade/features"])
            .current_dir(&fixture.0)
            .status()
            .unwrap();
        assert!(output.success());
        let output = std::process::Command::new("git")
            .args(["commit", "-qm", "seed published trace fixture"])
            .current_dir(&fixture.0)
            .status()
            .unwrap();
        assert!(output.success());
        let reader =
            crate::core::published_specs::PublishedSpecReader::for_primary_repository(&fixture.0)
                .unwrap();
        store.published_spec_pin = Some(reader.pin().unwrap());
    }
    let comparison = crate::domain::PlanComparison {
        alternatives: ["A", "B"]
            .into_iter()
            .map(|id| crate::domain::PlanAlternative {
                id: id.into(),
                objective: format!("Trace fixture using plan {id}"),
                phases: vec![
                    crate::domain::PlanPhase {
                        name: "Prepare".into(),
                        subtasks: vec!["Record the selected plan".into()],
                    },
                    crate::domain::PlanPhase {
                        name: "Implement".into(),
                        subtasks: vec!["Preserve requirement traceability".into()],
                    },
                    crate::domain::PlanPhase {
                        name: "Verify".into(),
                        subtasks: vec!["Check the persisted trace".into()],
                    },
                ],
                files_touched: vec![format!("src/trace_{id}.rs")],
                state_changes: vec![format!("Persist plan {id}")],
                failure_modes: vec![format!("Plan {id} cannot be verified")],
                effort_band: "Small".into(),
                known_risks: vec![format!("Plan {id} could omit a trace")],
                reversibility: format!("Revert plan {id}"),
            })
            .collect(),
        recommendation: crate::domain::PlanRecommendation {
            plan_id: "A".into(),
            rationale: "The fixture needs a stable, explicit plan choice.".into(),
            evidence: vec!["tests/traceability.rs".into()],
        },
        selected_plan: None,
    };
    let (decision_path, decision_content) = crate::artifacts::koolade::prepare_plan_choice_record(
        store,
        feature_id,
        &identity.uid,
        &comparison,
        "A",
    )
    .unwrap();
    let adopted_markdown = crate::domain::ChangeMetadata::adopt_typed_plan(
        &markdown,
        &comparison,
        "A",
        Some(&decision_path),
    )
    .unwrap();
    let selected_comparison = crate::domain::PlanComparison {
        selected_plan: Some("A".into()),
        ..comparison.clone()
    };
    let mut workflow = crate::core::workflow::Workflow::default();
    let adopted = crate::core::workflow::PlanComparisonRecord {
        schema_version: 1,
        feature_id: feature_id.to_owned(),
        alternatives: selected_comparison,
        history: Vec::new(),
        transcript: "Synthetic alternatives; the fixture explicitly adopted plan A.".into(),
        status: crate::core::workflow::PlanComparisonStatus::Adopted,
        selected_plan: Some("A".into()),
        updated_at_ms: 1,
    };
    adopted.validate().unwrap();
    workflow
        .plan_comparisons
        .insert(feature_id.to_owned(), adopted);
    let (mut changes, checks) =
        crate::artifacts::task_docs::workflow_record_changes(store, &workflow).unwrap();
    changes.extend(
        crate::core::apply::private_document_draft_changes(
            store,
            &[(format!("feature:{feature_id}"), adopted_markdown.clone())],
        )
        .unwrap(),
    );
    store
        .transaction_with_revision_and_record_revisions(
            &[
                changes,
                vec![(decision_path, decision_content.into_bytes())],
            ]
            .concat(),
            None,
            &checks,
        )
        .unwrap();
    let markdown = adopted_markdown;
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
        evidence_resolution: None,
        selected_option: Some("retain".into()),
        answered_by: Some("Fixture operator".into()),
        answered_at_ms: Some(1),
        deferred_by: None,
        deferred_at_ms: None,
        revision: 0,
    };
    crate::core::planning::QuestionStore::new(store)
        .save(&question, 0)
        .unwrap();
    uid
}
