use crate::{
    app::session::Project,
    artifacts::{planning_store::PlanningRoot, product_docs},
    core::{
        lifecycle::plan_snapshot::{self, PlanAuthorProvenanceV1},
        planning::QuestionStore,
    },
    domain::{ArtifactIdentity, ChangeMetadata, ChangeStatus},
};
use sha2::Digest as _;

pub(super) fn prepare_reviewable_plan(project: &mut Project, feature_id: &str) {
    let store = &project.state.planning_store;
    let prior = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == feature_id)
        .map(|(_, body)| body.clone())
        .expect("pursued feature is active");
    let title = "Self-improvement approval flow";
    let metadata_marker = prior
        .lines()
        .find(|line| line.starts_with("<!-- koolade-change:v1 "))
        .expect("Pursue draft has structured change metadata");
    let body = format!(
        "# {feature_id}: {title}\n\n## Intent\n\nUse the ordinary planning gates for Kool.ad/e self-improvements.\n\n## Current Behavior\n\nPursued ideas remain unapproved planning drafts.\n\n## Desired Behavior\n\nThe app obtains current user confirmation and independent plan review before task creation.\n\n## Scope\n\n- Bind Evolve planning to the ordinary reviewed plan lifecycle.\n\n## Affected Product Areas\n\n- planning\n\n## Requirements\n\n- REQ-001 (MUST). Keep self-improvement planning separate from clone execution. Rationale: Pursue does not authorize implementation. ACs: AC-001\n\n## Decisions and Assumptions\n\n- Plan confirmation and implementation authorization are separate decisions.\n\n## Acceptance Criteria\n\n- AC-001: No task clone is allocated until the current plan is user confirmed and independently reviewed.\n\n## Capability ID\n\nself-improvement-lifecycle\n\n{metadata_marker}\n"
    );
    let prior_identity = ArtifactIdentity::from_markdown(&prior)
        .unwrap()
        .expect("Pursue assigns a stable feature identity");
    assert_eq!(prior_identity.display_id, feature_id);
    let body = ArtifactIdentity::preserve_markdown(&body, Some(&prior), feature_id, title).unwrap();
    let identity = ArtifactIdentity::from_markdown(&body).unwrap().unwrap();
    let markdown = ChangeMetadata::write_markdown(&body, &identity, ChangeStatus::Ready).unwrap();
    let logical_id = format!("feature:{feature_id}");
    let path = product_docs::document_path_for_update(store, &logical_id, &markdown).unwrap();
    let layout = store.planning_layout();
    let relative = path
        .strip_prefix(layout.root())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let index = product_docs::refreshed_index(store, &[(logical_id, markdown.clone())]).unwrap();
    let expected = store.revision().unwrap();
    store
        .transaction_with_revision(
            &[
                (relative, markdown.as_bytes().to_vec()),
                (
                    crate::artifacts::planning_store::paths::PRODUCT_INDEX.to_owned(),
                    index.into_bytes(),
                ),
            ],
            Some(&expected),
        )
        .unwrap();
    let mut workflow = crate::artifacts::task_docs::load_workflow(store).unwrap();
    let comparison = crate::domain::PlanComparison {
        alternatives: vec![
            crate::domain::PlanAlternative {
                id: "A".into(),
                objective: "Keep the existing execution path while preparing a safe handoff."
                    .into(),
                phases: vec![
                    phase("Prepare", "Add the lifecycle adapter"),
                    phase("Connect", "Route Evolve through the adapter"),
                    phase("Verify", "Check the persisted lifecycle state"),
                ],
                files_touched: vec!["src/core/self_improvement.rs".into()],
                state_changes: vec!["Preserve the current run until handoff".into()],
                failure_modes: vec!["Adapter setup can fail before work starts".into()],
                effort_band: "Small — one lifecycle adapter".into(),
                known_risks: vec!["The adapter may need follow-up for additional stages".into()],
                reversibility: "Remove the adapter and restore the existing route".into(),
            },
            crate::domain::PlanAlternative {
                id: "B".into(),
                objective: "Move the run through the ordinary planning lifecycle.".into(),
                phases: vec![
                    phase("Prepare", "Create and review the feature plan"),
                    phase("Authorize", "Generate tasks after plan approval"),
                    phase("Verify", "Check Evolve remains in planning"),
                ],
                files_touched: vec!["src/app/root/opportunity_actions.rs".into()],
                state_changes: vec!["Keep Evolve planning until task generation".into()],
                failure_modes: vec!["Missing plan receipts prevent task creation".into()],
                effort_band: "Medium — planning workflow changes".into(),
                known_risks: vec!["The plan must stay current through adoption".into()],
                reversibility: "Restore the prior planning route and retain the run".into(),
            },
        ],
        recommendation: crate::domain::PlanRecommendation {
            plan_id: "B".into(),
            rationale: "Use the established reviewed planning path.".into(),
            evidence: vec!["src/app/feature_approval/approval.rs".into()],
        },
        selected_plan: None,
    };
    let record = crate::core::workflow::PlanComparisonRecord {
        schema_version: 1,
        feature_id: feature_id.to_owned(),
        alternatives: comparison,
        history: Vec::new(),
        transcript: "Deterministic app integration test comparison.".into(),
        status: crate::core::workflow::PlanComparisonStatus::Proposed,
        selected_plan: None,
        updated_at_ms: 0,
    };
    record.validate().unwrap();
    workflow
        .plan_comparisons
        .insert(feature_id.to_owned(), record);
    workflow.brief = Some(crate::core::workflow::InterviewBrief {
        feature_name: format!("{feature_id}: {title}"),
        problem: "A pursued product improvement needs a reviewed plan before task work.".into(),
        goal: "Create implementation tasks from the exact confirmed and reviewed plan.".into(),
        target_users: "Kool.ad/e maintainers planning product improvements.".into(),
        intended_outcome: "The approved Evolve plan appears as actionable task stories.".into(),
        success_criteria: vec![
            "The generated tasks cover each requirement in the reviewed plan.".into(),
        ],
        in_scope: vec!["Generate tasks for the selected reviewed plan.".into()],
        out_of_scope: vec!["Allocate implementation clones before task approval.".into()],
        constraints: vec!["Pursue authorizes planning, not implementation.".into()],
        ready_for_tasks: true,
    });
    workflow.reviewed_specification = Some(markdown.clone());
    workflow.feature_branch_targets.insert(
        feature_id.to_owned(),
        crate::core::workflow::BranchTargets {
            source: "main".into(),
            destination: "main".into(),
        },
    );
    crate::artifacts::task_docs::save_workflow(store, &workflow).unwrap();
    project.state.workflow = crate::artifacts::task_docs::load_workflow(store).unwrap();
    project
        .state
        .active_features
        .iter_mut()
        .find(|(id, _)| id == feature_id)
        .unwrap()
        .1 = markdown.clone();
    project.state.active_feature = Some((feature_id.to_owned(), markdown));
    project.state.baseline_planning_revision = store.revision().unwrap();
}

fn phase(name: &str, subtask: &str) -> crate::domain::PlanPhase {
    crate::domain::PlanPhase {
        name: name.into(),
        subtasks: vec![subtask.into()],
    }
}

pub(super) fn prepare_plan_review_snapshot(project: &mut Project, feature_id: &str) {
    let store = &project.state.planning_store;
    let markdown = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == feature_id)
        .map(|(_, body)| body.clone())
        .expect("adopted feature is active");
    let mut workflow = crate::artifacts::task_docs::load_workflow(store).unwrap();
    workflow.reviewed_specification = Some(markdown.clone());
    crate::artifacts::task_docs::save_workflow(store, &workflow).unwrap();
    project.state.workflow = crate::artifacts::task_docs::load_workflow(store).unwrap();
    project.state.baseline_planning_revision = store.revision().unwrap();
    let contract = crate::core::workflow::feature_contract(&markdown);
    let identity = ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .expect("adopted feature keeps its stable identity");
    plan_snapshot::record_author(
        store,
        &PlanAuthorProvenanceV1 {
            feature_uid: identity.uid.clone(),
            approval_contract_sha256: format!("{:x}", sha2::Sha256::digest(contract.as_bytes())),
            author_run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            author_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        },
    )
    .unwrap();
    let snapshot =
        plan_snapshot::from_feature_with_author(store, feature_id, &identity, &markdown, &contract)
            .unwrap();
    if let Some((path, bytes, check)) = plan_snapshot::record_change(store, &snapshot).unwrap() {
        store
            .transaction_with_record_revisions(&[(path, bytes)], &[check])
            .unwrap();
    }
    seed_review_readiness(store, &snapshot);
    project.state.baseline_planning_revision = store.revision().unwrap();
}

fn seed_review_readiness(
    store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &crate::core::lifecycle::PlanSnapshotV1,
) {
    use crate::core::planning::{AcceptanceTestTrace, ReadinessAxis, ReadinessChecklistV1};
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
    let acceptance_tests = snapshot
        .requirements
        .iter()
        .map(|requirement| AcceptanceTestTrace {
            requirement_id: requirement.stable_id.clone(),
            test_ref: "tests/self-improvement-lifecycle.rs".into(),
        })
        .collect();
    let checklist = ReadinessChecklistV1 {
        feature_uid: snapshot.feature_uid.clone(),
        plan_revision: 1,
        spec_revision: Some(snapshot.spec_revision.clone()),
        required_requirement_ids: Some(
            snapshot
                .requirements
                .iter()
                .map(|requirement| requirement.stable_id.clone())
                .collect(),
        ),
        assessed_axes: axes,
        acceptance_tests,
        evidence_refs: vec![format!("plan:{}", snapshot.uid)],
        unresolved_blocker_ids: Vec::new(),
        blocker_notes: Vec::new(),
        ready: true,
        stalled: false,
        no_progress_count: 0,
        progress_fingerprint: None,
        revision: 0,
    };
    QuestionStore::new(store)
        .save_readiness(&checklist, 0)
        .unwrap();
}
