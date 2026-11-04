use super::super::*;
use sha2::Digest;

pub(crate) fn record_test_plan_review_and_merge(
    app: &mut KooladeApp,
    id: &str,
    decision: crate::core::lifecycle::GateDecision,
) {
    record_test_plan_review(app, id, decision);
    if decision != crate::core::lifecycle::GateDecision::Approved {
        return;
    }
    let Screen::Connected(project) = &mut app.screen else {
        panic!()
    };
    let (_, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(feature_id, _)| feature_id == id)
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        id,
        &identity,
        markdown,
        &crate::core::workflow::feature_contract(markdown),
    )
    .unwrap();
    if crate::app::root::feature_approval::publication::has_pending(
        &project.state.planning_store,
        &identity.uid,
        &snapshot.content_hash,
    ) {
        crate::app::root::feature_approval::publication::simulate_merged_feature(
            project,
            &identity.uid,
            &snapshot.content_hash,
        )
        .unwrap();
    }
}

pub(crate) fn record_test_plan_review(
    app: &mut KooladeApp,
    id: &str,
    decision: crate::core::lifecycle::GateDecision,
) {
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render};
    ensure_test_plan_adopted(app, id);
    record_test_plan_author(app, id);
    let Screen::Connected(project) = &mut app.screen else {
        panic!()
    };
    let (_, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(feature_id, _)| feature_id == id)
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    let contract = crate::core::workflow::feature_contract(markdown);
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        id,
        &identity,
        markdown,
        &contract,
    )
    .unwrap();
    // Exercise the production approval API to record explicit user confirmation
    // and the matching feature approval for this exact immutable snapshot.
    // Reviewer fixtures must not silently bypass the user-consent gate.
    let mut workflow =
        crate::artifacts::task_docs::load_workflow(&project.state.planning_store).unwrap();
    let expected_revision = project.state.planning_store.revision().unwrap();
    let approval = crate::core::workflow::approve_feature_if_current_with_snapshot_with_revision(
        &project.state.planning_store,
        &mut workflow,
        id,
        Some(&contract),
        Some(&snapshot.content_hash),
        &expected_revision,
    )
    .unwrap();
    approval
        .save_result
        .unwrap_or_else(|error| panic!("test user confirmation was not persisted: {error}"));
    project.state.workflow = workflow;
    project.state.baseline_planning_revision = approval.planning_revision;
    let author_run_id = snapshot.author_run_id.clone().unwrap();
    let author_session_id = snapshot.author_session_id.clone().unwrap();
    let reviewer_run_id = "00000000-0000-4000-8000-000000000003".to_owned();
    let reviewer_session_id = "00000000-0000-4000-8000-000000000004".to_owned();
    let manifest = render(&RoleInputV1 {
        role: PromptRole::PlanReviewer,
        workspace_uid: project
            .state
            .planning_store
            .project_id
            .hyphenated()
            .to_string(),
        feature_uid: identity.uid.clone(),
        task_uid: None,
        original_goal: snapshot.intent.clone(),
        approved_spec_ref: Some(snapshot.uid.clone()),
        approved_spec_hash: Some(snapshot.content_hash.clone()),
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "plan-review/read-only/v1".into(),
        output_schema_version: PromptRole::PlanReviewer.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: snapshot.spec_revision.clone(),
            subject_hash: snapshot.content_hash.clone(),
            author_run_id: author_run_id.clone(),
            reviewer_run_id: reviewer_run_id.clone(),
            author_session_id: Some(author_session_id.clone()),
            reviewer_session_id: Some(reviewer_session_id.clone()),
            requirements_matrix: snapshot
                .requirements
                .iter()
                .map(|requirement| requirement.text.clone())
                .collect(),
            explicit_exclusions: snapshot.non_goals.clone(),
        },
    })
    .unwrap()
    .manifest;
    let receipt = crate::core::lifecycle::GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: crate::core::lifecycle::GateActor::IndependentReviewer,
        decision,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![format!("plan:{}", snapshot.uid)],
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(reviewer_run_id),
        author_run_id: Some(author_run_id),
        reviewer_session_id: Some(reviewer_session_id),
        author_session_id: Some(author_session_id),
    };
    let approved = decision == crate::core::lifecycle::GateDecision::Approved;
    let coverage = snapshot
        .requirements
        .iter()
        .enumerate()
        .map(
            |(index, requirement)| crate::harness::prompt_catalog::CoverageV1 {
                requirement: requirement.stable_id.clone(),
                status: if !approved && index == 0 {
                    crate::harness::prompt_catalog::CoverageStatus::Fail
                } else {
                    crate::harness::prompt_catalog::CoverageStatus::Pass
                },
                evidence: vec![format!("plan:{}", snapshot.uid)],
            },
        )
        .collect();
    let findings = if approved {
        Vec::new()
    } else {
        vec![crate::harness::prompt_catalog::ReviewFindingV1 {
            id: "TEST-FINDING".into(),
            requirement: snapshot.requirements[0].stable_id.clone(),
            location_detail: None,
            rationale: String::new(),
            affected_requirement_ids: Vec::new(),
            suggested_action: None,
            severity: crate::harness::prompt_catalog::Severity::High,
            kind: crate::harness::prompt_catalog::FindingKind::Requirements,
            scope: None,
            location: "test fixture requirement".into(),
            evidence: vec![format!("plan:{}", snapshot.uid)],
            repro: Some("The fixture omits one requirement detail.".into()),
            proposed_correction: "Clarify the missing requirement detail.".into(),
        }]
    };
    let verdict = crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: snapshot.content_hash.clone(),
        examined_files: Vec::new(),
        tested_evidence_ids: Vec::new(),
        unexamined_areas: Vec::new(),
        pass: approved,
        outcome: Some(if approved {
            crate::harness::prompt_catalog::ReviewOutcomeV1::Pass
        } else {
            crate::harness::prompt_catalog::ReviewOutcomeV1::BlockingFindings
        }),
        coverage,
        findings,
        untested: Vec::new(),
    };
    crate::core::workflow::record_plan_review_if_current(
        &project.state.planning_store,
        id,
        &receipt,
        &verdict,
        &project.state.baseline_planning_revision,
    )
    .unwrap();
    if decision == crate::core::lifecycle::GateDecision::Approved {
        let (readiness, _) =
            crate::core::planning::QuestionStore::new(&project.state.planning_store)
                .load_readiness(&identity.uid)
                .unwrap()
                .unwrap();
        let mut readiness_refs = readiness.evidence_refs;
        readiness_refs.extend(
            readiness
                .acceptance_tests
                .into_iter()
                .map(|acceptance_test| acceptance_test.test_ref),
        );
        readiness_refs.sort();
        readiness_refs.dedup();
        let user = crate::core::lifecycle::plan_snapshot::user_confirmation_receipt(
            &project.state.planning_store,
            &snapshot,
        )
        .unwrap()
        .unwrap();
        let review = crate::core::lifecycle::plan_snapshot::plan_review_receipt(
            &project.state.planning_store,
            &snapshot,
        )
        .unwrap()
        .unwrap();
        crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store)
            .reconcile_reviewed_plan(&snapshot, markdown, readiness_refs, &user, &review)
            .unwrap();
    }
    project.state.baseline_planning_revision = project.state.planning_store.revision().unwrap();
}

pub(crate) fn ensure_test_plan_adopted(app: &mut KooladeApp, id: &str) {
    let plan_id = {
        let Screen::Connected(project) = &mut app.screen else {
            panic!()
        };
        if project
            .state
            .workflow
            .plan_comparisons
            .get(id)
            .is_some_and(|record| {
                record.status == crate::core::workflow::PlanComparisonStatus::Adopted
                    && record.selected_plan.is_some()
            })
        {
            return;
        }

        let mut workflow =
            crate::artifacts::task_docs::load_workflow(&project.state.planning_store).unwrap();
        let plan_id = if let Some(record) = workflow.plan_comparisons.get(id) {
            record.alternatives.recommendation.plan_id.clone()
        } else {
            let alternative = |plan_id: &str| crate::domain::PlanAlternative {
                id: plan_id.into(),
                objective: format!("Test plan {plan_id} for the confirmed feature."),
                phases: vec![
                    crate::domain::PlanPhase {
                        name: "Prepare".into(),
                        subtasks: vec!["Prepare the selected implementation path.".into()],
                    },
                    crate::domain::PlanPhase {
                        name: "Implement".into(),
                        subtasks: vec!["Implement the approved feature requirements.".into()],
                    },
                    crate::domain::PlanPhase {
                        name: "Verify".into(),
                        subtasks: vec!["Verify the approved acceptance criteria.".into()],
                    },
                ],
                files_touched: vec![format!("src/plan_{plan_id}.rs")],
                state_changes: vec![format!("Persist plan {plan_id} implementation state.")],
                failure_modes: vec![format!("Plan {plan_id} may fail verification.")],
                effort_band: "Small — one module".into(),
                known_risks: vec![format!("Plan {plan_id} may need follow-up.")],
                reversibility: format!("Revert plan {plan_id} changes."),
            };
            let comparison = crate::domain::PlanComparison {
                alternatives: vec![alternative("A"), alternative("B")],
                recommendation: crate::domain::PlanRecommendation {
                    plan_id: "A".into(),
                    rationale: "Use the focused fixture plan.".into(),
                    evidence: vec![format!("feature:{id}")],
                },
                selected_plan: None,
            };
            let record = crate::core::workflow::PlanComparisonRecord {
                schema_version: 1,
                feature_id: id.into(),
                alternatives: comparison,
                history: Vec::new(),
                transcript: "The test fixture presents two plans for an explicit choice.".into(),
                status: crate::core::workflow::PlanComparisonStatus::Proposed,
                selected_plan: None,
                updated_at_ms: 1,
            };
            record.validate().unwrap();
            workflow.plan_comparisons.insert(id.into(), record);
            "A".to_owned()
        };
        workflow.approved_features.remove(id);
        let (changes, checks) = crate::artifacts::task_docs::workflow_record_changes(
            &project.state.planning_store,
            &workflow,
        )
        .unwrap();
        project
            .state
            .planning_store
            .transaction_with_revision_and_record_revisions(
                &changes,
                Some(&project.state.baseline_planning_revision),
                &checks,
            )
            .unwrap();
        let code_root = project.state.repo_root.clone();
        let planning_store = project.state.planning_store.clone();
        project.state =
            crate::core::state::PlannerState::load_with_store(&code_root, &planning_store).unwrap();
        plan_id
    };

    let Screen::Connected(project) = &mut app.screen else {
        panic!()
    };
    let store = project.state.planning_store.clone();
    let code_root = project.state.repo_root.clone();
    let expected_revision = project.state.baseline_planning_revision.clone();
    let (_, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(feature_id, _)| feature_id == id)
        .expect("fixture feature is available");
    let markdown = markdown.clone();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .expect("fixture feature has stable identity");
    let comparison = project
        .state
        .workflow
        .plan_comparisons
        .get(id)
        .expect("fixture comparison was persisted")
        .alternatives
        .clone();
    let (decision_path, decision_content) = crate::artifacts::koolade::prepare_plan_choice_record(
        &store,
        id,
        &identity.uid,
        &comparison,
        &plan_id,
    )
    .unwrap();
    let adopted_markdown = crate::domain::ChangeMetadata::adopt_typed_plan(
        &markdown,
        &comparison,
        &plan_id,
        Some(&decision_path),
    )
    .unwrap();
    let mut workflow = crate::artifacts::task_docs::load_workflow(&store).unwrap();
    let record = workflow
        .plan_comparisons
        .get_mut(id)
        .expect("fixture comparison is persisted");
    record.alternatives.selected_plan = Some(plan_id.clone());
    record.selected_plan = Some(plan_id.clone());
    record.status = crate::core::workflow::PlanComparisonStatus::Adopted;
    record.updated_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(record.updated_at_ms);
    record.validate().unwrap();
    workflow.approved_features.remove(id);
    let (mut changes, checks) =
        crate::artifacts::task_docs::workflow_record_changes(&store, &workflow).unwrap();
    changes.extend(
        crate::core::apply::private_document_draft_changes(
            &store,
            &[(format!("feature:{id}"), adopted_markdown)],
        )
        .unwrap(),
    );
    changes.push((decision_path, decision_content.into_bytes()));
    store
        .transaction_with_revision_and_record_revisions(&changes, Some(&expected_revision), &checks)
        .unwrap();
    project.state = crate::core::state::PlannerState::load_with_store(&code_root, &store).unwrap();
    let record = project
        .state
        .workflow
        .plan_comparisons
        .get(id)
        .expect("fixture plan adoption was persisted");
    assert_eq!(
        record.status,
        crate::core::workflow::PlanComparisonStatus::Adopted
    );
    assert_eq!(record.selected_plan.as_deref(), Some(plan_id.as_str()));
}

pub(crate) fn record_test_plan_author(app: &mut KooladeApp, id: &str) {
    // Author provenance must describe the explicitly adopted plan snapshot.
    // Adoption may update the private draft, so load the feature only after it.
    ensure_test_plan_adopted(app, id);
    let Screen::Connected(project) = &mut app.screen else {
        panic!()
    };
    let (_, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(feature_id, _)| feature_id == id)
        .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .unwrap()
        .unwrap();
    let contract = crate::core::workflow::feature_contract(markdown);
    crate::core::lifecycle::plan_snapshot::record_author(
        &project.state.planning_store,
        &crate::core::lifecycle::plan_snapshot::PlanAuthorProvenanceV1 {
            feature_uid: identity.uid,
            approval_contract_sha256: format!("{:x}", sha2::Sha256::digest(contract.as_bytes())),
            author_run_id: "00000000-0000-4000-8000-000000000001".into(),
            author_session_id: "00000000-0000-4000-8000-000000000002".into(),
        },
    )
    .unwrap();
    project.state.baseline_planning_revision = project.state.planning_store.revision().unwrap();
    readiness::seed_test_readiness(&project.state.planning_store, id, markdown);
    project.state.baseline_planning_revision = project.state.planning_store.revision().unwrap();
}

pub(crate) fn assert_current_test_plan_confirmation(app: &mut KooladeApp, id: &str) {
    let (confirmed, has_approval, has_receipt, readiness, pending) = match &app.screen {
        Screen::Connected(project) => {
            let (_, markdown) = project
                .state
                .active_features
                .iter()
                .find(|(feature_id, _)| feature_id == id)
                .expect("test feature is loaded");
            let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
                .unwrap()
                .unwrap();
            let contract = crate::core::workflow::feature_contract(markdown);
            let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
                &project.state.planning_store,
                id,
                &identity,
                markdown,
                &contract,
            )
            .unwrap();
            let has_receipt = crate::core::lifecycle::plan_snapshot::has_user_confirmation(
                &project.state.planning_store,
                &snapshot,
            )
            .unwrap_or(false);
            let has_approval = crate::persistence::feature_approvals::contains(
                &project.state.planning_store.local_root(),
                id,
                &contract,
            );
            let readiness = crate::app::root::feature_approval::readiness::current(
                &project.state.planning_store,
                &snapshot,
            )
            .map_err(|error| error.to_string());
            (
                has_receipt && has_approval,
                has_approval,
                has_receipt,
                readiness,
                project.activity.pending.join(" | "),
            )
        }
        Screen::Welcome => (
            false,
            false,
            false,
            Err("application is on Welcome".into()),
            "application is on Welcome".into(),
        ),
    };
    if !confirmed {
        let toast = crate::app::root::board_tests::canvas_text(
            &crate::app::root::board_tests::frame_toasting(app, &egui::Context::default()),
        );
        panic!(
            "explicit test-plan approval was not persisted; feature_approval={has_approval}; user_confirmation_receipt={has_receipt}; readiness={readiness:?}; pending={pending:?}; toast={toast:?}"
        );
    }
}
