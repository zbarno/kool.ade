use crate::artifacts::planning_store::PlanningStore;
use crate::core::workflow::{self, Workflow};
use sha2::Digest;

/// Test fixture helper for the complete user confirmation plus independent
/// reviewer receipt required before task generation.
pub(crate) fn approve_and_review_feature(
    store: &PlanningStore,
    workflow_record: &mut Workflow,
    feature_id: &str,
    markdown: &str,
) -> anyhow::Result<String> {
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render};
    let markdown = adopt_fixture_plan_if_needed(store, workflow_record, feature_id, markdown)?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)?
        .ok_or_else(|| anyhow::anyhow!("test feature has no identity"))?;
    let contract = workflow::feature_contract(&markdown);
    crate::core::lifecycle::plan_snapshot::record_author(
        store,
        &crate::core::lifecycle::plan_snapshot::PlanAuthorProvenanceV1 {
            feature_uid: identity.uid.clone(),
            approval_contract_sha256: format!("{:x}", sha2::Sha256::digest(contract.as_bytes())),
            author_run_id: "00000000-0000-4000-8000-000000000001".into(),
            author_session_id: "00000000-0000-4000-8000-000000000002".into(),
        },
    )?;
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        store, feature_id, &identity, &markdown, &contract,
    )
    .map_err(anyhow::Error::msg)?;
    seed_readiness(store, &snapshot)?;
    let current_revision = store.revision()?;
    let receipt = workflow::approve_feature_if_current_with_snapshot_with_revision(
        store,
        workflow_record,
        feature_id,
        Some(&contract),
        Some(&snapshot.content_hash),
        &current_revision,
    )?;
    receipt.save_result.map_err(anyhow::Error::msg)?;

    let reviewer_run_id = "00000000-0000-4000-8000-000000000003".to_owned();
    let reviewer_session_id = "00000000-0000-4000-8000-000000000004".to_owned();
    let author_run_id = snapshot.author_run_id.clone().unwrap();
    let author_session_id = snapshot.author_session_id.clone().unwrap();
    let manifest = render(&RoleInputV1 {
        role: PromptRole::PlanReviewer,
        workspace_uid: store.project_id.hyphenated().to_string(),
        feature_uid: identity.uid,
        task_uid: None,
        original_goal: snapshot.intent.clone(),
        approved_spec_ref: Some(snapshot.uid.clone()),
        approved_spec_hash: Some(snapshot.content_hash.clone()),
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "plan-review/test-only/v1".into(),
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
    .map_err(anyhow::Error::msg)?
    .manifest;
    let review = crate::core::lifecycle::GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: crate::core::lifecycle::GateActor::IndependentReviewer,
        decision: crate::core::lifecycle::GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: std::iter::once(format!("plan:{}", snapshot.uid))
            .chain(
                snapshot
                    .requirements
                    .iter()
                    .map(|requirement| format!("requirement:{}", requirement.stable_id)),
            )
            .chain(std::iter::once("test-only/explicit-plan-review".into()))
            .collect(),
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(reviewer_run_id),
        author_run_id: Some(author_run_id),
        reviewer_session_id: Some(reviewer_session_id),
        author_session_id: Some(author_session_id),
    };
    let coverage = snapshot
        .requirements
        .iter()
        .map(|requirement| crate::harness::prompt_catalog::CoverageV1 {
            requirement: requirement.stable_id.clone(),
            status: crate::harness::prompt_catalog::CoverageStatus::Pass,
            evidence: vec![format!("plan:{}", snapshot.uid)],
        })
        .collect();
    let verdict = crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: snapshot.content_hash.clone(),
        examined_files: Vec::new(),
        tested_evidence_ids: Vec::new(),
        unexamined_areas: Vec::new(),
        pass: true,
        outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass),
        coverage,
        findings: Vec::new(),
        untested: Vec::new(),
    };
    let revision = store.revision()?;
    workflow::record_plan_review_if_current(store, feature_id, &review, &verdict, &revision)?;
    *workflow_record = crate::artifacts::task_docs::load_workflow(store)?;

    snapshot_for_lifecycle(store, feature_id, &markdown)?;
    Ok(markdown)
}

/// Test-only user consent uses the same plan-choice ADR, typed adoption
/// metadata, and revision-fenced transaction as the application action.
/// Tests may start from a feature in a later lifecycle state; model the
/// explicit adoption while Ready, then restore that state before approval.
fn adopt_fixture_plan_if_needed(
    store: &PlanningStore,
    workflow_record: &mut Workflow,
    feature_id: &str,
    markdown: &str,
) -> anyhow::Result<String> {
    if workflow_record
        .plan_comparisons
        .get(feature_id)
        .is_some_and(|record| {
            record.status == workflow::PlanComparisonStatus::Adopted
                && record.selected_plan.is_some()
        })
    {
        return Ok(markdown.to_owned());
    }

    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)?
        .ok_or_else(|| anyhow::anyhow!("test feature has no identity"))?;
    let original = crate::domain::ChangeMetadata::require_markdown(markdown)?;
    let ready = crate::domain::ChangeMetadata::write_markdown(
        markdown,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )?;
    let alternative = |plan_id: &str| crate::domain::PlanAlternative {
        id: plan_id.into(),
        objective: format!("Test plan {plan_id} for the approved feature."),
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
    let comparison = workflow_record
        .plan_comparisons
        .get(feature_id)
        .map(|record| record.alternatives.clone())
        .unwrap_or_else(|| crate::domain::PlanComparison {
            alternatives: vec![alternative("A"), alternative("B")],
            recommendation: crate::domain::PlanRecommendation {
                plan_id: "A".into(),
                rationale: "Use the focused fixture plan.".into(),
                evidence: vec![format!("feature:{}", identity.uid)],
            },
            selected_plan: None,
        });
    let adr = crate::artifacts::koolade::prepare_plan_choice_record(
        store,
        feature_id,
        &identity.uid,
        &comparison,
        "A",
    )?;
    let mut adopted =
        crate::domain::ChangeMetadata::adopt_typed_plan(&ready, &comparison, "A", Some(&adr.0))?;
    if original.status != crate::domain::ChangeStatus::Ready {
        adopted =
            crate::domain::ChangeMetadata::write_markdown(&adopted, &identity, original.status)?;
    }

    let record = workflow_record
        .plan_comparisons
        .entry(feature_id.to_owned())
        .or_insert_with(|| workflow::PlanComparisonRecord {
            schema_version: 1,
            feature_id: feature_id.to_owned(),
            alternatives: comparison.clone(),
            history: Vec::new(),
            transcript: "Fixture plan presented for explicit user selection.".into(),
            status: workflow::PlanComparisonStatus::Proposed,
            selected_plan: None,
            updated_at_ms: 1,
        });
    record.alternatives.selected_plan = Some("A".into());
    record.selected_plan = Some("A".into());
    record.status = workflow::PlanComparisonStatus::Adopted;
    record.validate()?;

    let (mut changes, checks) =
        crate::artifacts::task_docs::workflow_record_changes(store, workflow_record)?;
    changes.extend(crate::core::apply::private_document_draft_changes(
        store,
        &[(format!("feature:{feature_id}"), adopted.clone())],
    )?);
    changes.push((adr.0, adr.1.into_bytes()));
    let expected = store.revision()?;
    store.transaction_with_revision_and_record_revisions(&changes, Some(&expected), &checks)?;
    *workflow_record = crate::artifacts::task_docs::load_workflow(store)?;
    Ok(adopted)
}

fn snapshot_for_lifecycle(
    store: &PlanningStore,
    feature_id: &str,
    markdown: &str,
) -> anyhow::Result<crate::core::lifecycle::PlanSnapshotV1> {
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)?
        .ok_or_else(|| anyhow::anyhow!("test feature has no identity"))?;
    let contract = workflow::feature_contract(markdown);
    let plan = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        store, feature_id, &identity, markdown, &contract,
    )
    .map_err(anyhow::Error::msg)?;
    let (readiness, _) = crate::core::planning::QuestionStore::new(store)
        .load_readiness(&plan.feature_uid)?
        .ok_or_else(|| anyhow::anyhow!("test plan has no readiness fixture"))?;
    let mut readiness_refs = readiness.evidence_refs;
    readiness_refs.extend(
        readiness
            .acceptance_tests
            .into_iter()
            .map(|acceptance_test| acceptance_test.test_ref),
    );
    readiness_refs.sort();
    readiness_refs.dedup();
    let user = crate::core::lifecycle::plan_snapshot::user_confirmation_receipt(store, &plan)?
        .ok_or_else(|| anyhow::anyhow!("test plan has no persisted user confirmation"))?;
    let reviewer = crate::core::lifecycle::plan_snapshot::plan_review_receipt(store, &plan)?
        .ok_or_else(|| anyhow::anyhow!("test plan has no persisted reviewer receipt"))?;
    crate::core::lifecycle::LifecycleStore::new(store).reconcile_reviewed_plan(
        &plan,
        markdown,
        readiness_refs,
        &user,
        &reviewer,
    )?;
    Ok(plan)
}

fn seed_readiness(
    store: &PlanningStore,
    plan: &crate::core::lifecycle::PlanSnapshotV1,
) -> anyhow::Result<Vec<String>> {
    use crate::core::planning::{
        AcceptanceTestTrace, QuestionStore, ReadinessAxis, ReadinessChecklistV1,
    };

    let mut evidence_refs = vec!["test-fixture:approved-plan-readiness".to_owned()];
    let acceptance_tests = plan
        .requirements
        .iter()
        .map(|requirement| AcceptanceTestTrace {
            requirement_id: requirement.stable_id.clone(),
            test_ref: "tests/feature.rs".into(),
        })
        .collect::<Vec<_>>();
    evidence_refs.extend(
        acceptance_tests
            .iter()
            .map(|acceptance_test| acceptance_test.test_ref.clone()),
    );
    evidence_refs.sort();
    evidence_refs.dedup();
    let axes = vec![
        ReadinessAxis::Who,
        ReadinessAxis::What,
        ReadinessAxis::When,
        ReadinessAxis::Where,
        ReadinessAxis::Why,
        ReadinessAxis::How,
        ReadinessAxis::OutOfScope,
        ReadinessAxis::CriticalPath,
        ReadinessAxis::SystemBoundaries,
        ReadinessAxis::FailureBehavior,
        ReadinessAxis::DataOwnership,
        ReadinessAxis::Authorization,
        ReadinessAxis::MeasurableSuccess,
        ReadinessAxis::TestCases,
    ];
    let questions = QuestionStore::new(store);
    let revision = questions
        .load_readiness(&plan.feature_uid)?
        .map_or(0, |(_, revision)| revision);
    let checklist = ReadinessChecklistV1 {
        feature_uid: plan.feature_uid.clone(),
        plan_revision: 1,
        spec_revision: Some(plan.spec_revision.clone()),
        required_requirement_ids: Some(
            plan.requirements
                .iter()
                .map(|requirement| requirement.stable_id.clone())
                .collect(),
        ),
        assessed_axes: axes,
        acceptance_tests,
        evidence_refs: vec!["test-fixture:approved-plan-readiness".to_owned()],
        unresolved_blocker_ids: Vec::new(),
        blocker_notes: Vec::new(),
        ready: true,
        stalled: false,
        no_progress_count: 0,
        progress_fingerprint: None,
        revision: 0,
    };
    questions.save_readiness(&checklist, revision)?;
    Ok(evidence_refs)
}
