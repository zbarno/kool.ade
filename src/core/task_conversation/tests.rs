use super::*;

#[test]
fn references_match_whole_ids_in_headings_bullets_and_tables() {
    let document = "# Scope\nOverview\n- **FR-1** Use corporate SSO.\n  Require MFA.\n- **FR-10** Unrelated reporting.\n## D-2 Authentication decision\nUse the existing directory.\n## D-20 Reporting decision\nUnrelated exports.\n| A-3 | SSO is available |\n| A-30 | Reporting is enabled |";
    let selected =
        super::references::referenced_sections(document, "Implement FR-1 based on D-2 and A-3");
    assert!(selected.contains("Require MFA"));
    assert!(selected.contains("existing directory"));
    assert!(selected.contains("SSO is available"));
    assert!(!selected.contains("Unrelated"));
    assert!(!selected.contains("Reporting is enabled"));
}

#[test]
fn task_story_prompt_includes_explicit_dependency_and_referenced_specification() {
    let root = std::env::temp_dir().join(format!("koolade_task_prompt_{}", std::process::id()));
    let directory = ".koolade-packet/planning/tasks/authentication";
    std::fs::create_dir_all(root.join(directory)).unwrap();
    let mut state = PlannerState::load(&root).unwrap();
    state.spec_text = Some("# Product\n## FR-1 Sign in\nUse corporate SSO.\n## FR-10 Reporting\nUnrelated reporting details.".into());
    state
        .workflow
        .task_batches
        .push(crate::core::workflow::TaskBatchRef {
            identity: None,
            feature: "Authentication".into(),
            directory: directory.into(),
            count: 3,
            created_at_ms: 0,
        });
    std::fs::write(
        root.join(directory).join("001-login.md"),
        "# Sign in\nImplement FR-1.\n## Dependencies\n[Directory](002-directory.md)",
    )
    .unwrap();
    std::fs::write(
        root.join(directory).join("002-directory.md"),
        "# Directory\nProvision the corporate tenant.",
    )
    .unwrap();
    std::fs::write(
        root.join(directory).join("003-reporting.md"),
        "# Reporting\nUNRELATED TASK DETAILS",
    )
    .unwrap();
    let body = prompt(
        &state,
        &format!("{directory}/001-login.md"),
        "Use the existing provider",
        &[("User".into(), "OUR TASK HISTORY".into())],
    )
    .unwrap();
    assert!(body.contains("Use corporate SSO."));
    assert!(body.contains("Provision the corporate tenant."));
    assert!(body.contains("OUR TASK HISTORY"));
    assert!(!body.contains("UNRELATED TASK DETAILS"));
    assert!(!body.contains("Unrelated reporting details"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn improvement_proposal_conversation_is_scoped_to_read_only_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let state = PlannerState::load(temp.path()).unwrap();
    let proposal = crate::core::opportunity::OpportunityV1 {
        schema_version: 1,
        opportunity_uid: uuid::Uuid::new_v4().to_string(),
        workspace_uid: uuid::Uuid::new_v4().to_string(),
        origin: crate::core::opportunity::OpportunityOriginV1::ReviewFinding,
        kind: crate::core::opportunity::OpportunityKindV1::Quality,
        target: crate::core::opportunity::OpportunityTargetV1 {
            kind: crate::core::opportunity::OpportunityTargetKindV1::ActiveUserProject,
            repository_uid: uuid::Uuid::new_v4().to_string(),
            canonical_remote: "https://github.com/fixture/repo".into(),
            source_sha: "a".repeat(40),
        },
        related_feature_uids: Vec::new(),
        related_task_uids: Vec::new(),
        discovered_at_ms: 1,
        observation_revision: 1,
        evidence_refs: vec![crate::core::opportunity::OpportunityEvidenceRefV1 {
            evidence_uid: uuid::Uuid::new_v4().to_string(),
            sha256: "b".repeat(64),
            confidentiality: crate::core::opportunity::EvidenceConfidentialityV1::UserLocal,
        }],
        problem_or_opportunity: "Reduce repeated planning retries".into(),
        user_value: "Keep planning responsive".into(),
        expected_outcome: "Retries are bounded".into(),
        possible_approaches: vec!["Bound retry count".into()],
        recommended_approach: "Use a bounded retry".into(),
        alternatives: vec!["Require a manual retry".into()],
        estimated_scope: crate::core::opportunity::OpportunityScopeV1::Small,
        impact: "Fewer interruptions".into(),
        cost_or_risk: "Adds retry state".into(),
        affected_systems: vec!["planning".into()],
        current_capability_state: "Retries are unbounded".into(),
        generic_workaround: None,
        proposed_approach: "Bound retry count".into(),
        dedup_fingerprint: "c".repeat(64),
        status: crate::core::opportunity::OpportunityStatusV1::AwaitingDecision,
        revision: 1,
    };
    crate::core::opportunity::OpportunityStore::new(&state.planning_store)
        .save_advisory(proposal.clone())
        .unwrap();
    let key = opportunity_conversation_key(&proposal.opportunity_uid);
    let (context, greeting) = presentation(&state, &[], &key).unwrap();
    assert!(context.contains(&proposal.opportunity_uid));
    assert!(greeting.contains("advisory"));

    let body = prompt(
        &state,
        &key,
        "Why is the retry limit preferred?",
        &[(
            "User".into(),
            "Only this proposal's prior discussion".into(),
        )],
    )
    .unwrap();
    assert!(body.contains("IMPROVEMENT PROPOSAL DISCUSSION"));
    assert!(body.contains("Only this proposal's prior discussion"));
    assert!(body.contains("THIS PROPOSAL'S DISCUSSION ONLY"));
    assert!(body.contains(&proposal.evidence_refs[0].evidence_uid));
    assert!(!body.contains("create or update the project's specifications"));
    assert!(is_opportunity_conversation_key(&key));
    assert!(!is_opportunity_conversation_key("opportunity:not-a-uuid"));
}

#[test]
fn improvement_proposal_discussion_rejects_structured_project_changes() {
    let safe: crate::harness::TurnEnvelope =
        serde_json::from_value(serde_json::json!({"assistantMessage":"Here are the tradeoffs."}))
            .unwrap();
    assert!(!opportunity_discussion_requests_project_change(&safe));

    let follow_up: crate::harness::TurnEnvelope = serde_json::from_value(serde_json::json!({
        "assistantMessage":"I can create a task.",
        "followUpTask":{"title":"Implement retry","description":"Add a bounded retry."}
    }))
    .unwrap();
    assert!(opportunity_discussion_requests_project_change(&follow_up));

    let board_change: crate::harness::TurnEnvelope = serde_json::from_value(serde_json::json!({
        "updatedSpecification":"# silently changed"
    }))
    .unwrap();
    assert!(opportunity_discussion_requests_project_change(
        &board_change
    ));
}
