use koolade::{
    core::lifecycle::{
        FeatureGoal, FeaturePhase, GateActor, GateDecision, GateReceipt, TaskPhase, TaskRecord,
    },
    harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render},
};

pub(super) fn id() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}

pub(super) fn new_feature(feature_uid: String) -> FeatureGoal {
    FeatureGoal {
        schema_version: 1,
        feature_uid,
        original_intent: "Add an accessible search dialog".into(),
        draft: "A search dialog with keyboard dismissal".into(),
        scope: vec!["Search dialog".into()],
        non_goals: vec!["Deployment".into()],
        revision: 0,
        phase: FeaturePhase::Exploring,
        approved_plan_sha256: None,
        plan_snapshot_uid: None,
        plan_snapshot_sha256: None,
        unresolved_questions: Vec::new(),
        decision_refs: Vec::new(),
        review_refs: Vec::new(),
        readiness_evidence_refs: Vec::new(),
        task_uids: Vec::new(),
    }
}

pub(super) fn bootstrap_product(store: &koolade::artifacts::planning_store::PlanningStore) {
    use koolade::artifacts::product_docs::ProductManifest;
    let manifest = ProductManifest::core();
    let mut changes = vec![
        (
            "planning/product/manifest.json".to_owned(),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        ),
        (
            "planning/product/index.md".to_owned(),
            b"# Product Specification\n\nSynthetic acceptance fixture.\n".to_vec(),
        ),
    ];
    for module in manifest.modules {
        let content = format!(
            "# {}\n\n## Summary\n\nSynthetic acceptance fixture product context.\n\n## Requirements\n\nThe fixture exercises approved lifecycle reconciliation.\n",
            module.title
        );
        changes.push((
            format!("planning/product/{}", module.path),
            content.into_bytes(),
        ));
    }
    store.transaction(&changes, None).unwrap();
}

pub(super) fn product_hash(store: &koolade::artifacts::planning_store::PlanningStore) -> String {
    use sha2::Digest as _;
    let rendered = koolade::artifacts::product_docs::render_product(store)
        .unwrap()
        .unwrap();
    format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&Some(rendered)).unwrap())
    )
}

/// Deterministic fake role adapter. It produces only a typed review decision;
/// lifecycle guards still validate the role manifest and artifact binding.
pub(super) fn role_receipt(
    role: PromptRole,
    decision: GateDecision,
    subject: &str,
    hash: &str,
) -> GateReceipt {
    let author_run = id();
    let author_session = id();
    role_receipt_for_author(role, decision, subject, hash, &author_run, &author_session)
}

pub(super) fn role_receipt_for_author(
    role: PromptRole,
    decision: GateDecision,
    subject: &str,
    hash: &str,
    author_run: &str,
    author_session: &str,
) -> GateReceipt {
    let reviewer_run = id();
    let reviewer_session = id();
    let manifest = render(&RoleInputV1 {
        role,
        workspace_uid: id(),
        feature_uid: subject.to_owned(),
        task_uid: (role != PromptRole::PlanReviewer).then(|| subject.to_owned()),
        original_goal: "Add an accessible, responsive search dialog".into(),
        approved_spec_ref: None,
        approved_spec_hash: None,
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "acceptance-fixture/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: "1".into(),
            subject_hash: hash.into(),
            author_run_id: author_run.to_owned(),
            reviewer_run_id: reviewer_run.clone(),
            author_session_id: Some(author_session.to_owned()),
            reviewer_session_id: Some(reviewer_session.clone()),
            requirements_matrix: vec!["Keyboard users can search and dismiss".into()],
            explicit_exclusions: vec!["Deployment".into()],
        },
    })
    .unwrap()
    .manifest;
    GateReceipt {
        receipt_uid: id(),
        subject_uid: subject.into(),
        subject_sha256: hash.into(),
        actor: GateActor::IndependentReviewer,
        decision,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture:review-output".into()],
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(reviewer_run),
        author_run_id: Some(author_run.to_owned()),
        reviewer_session_id: Some(reviewer_session),
        author_session_id: Some(author_session.to_owned()),
    }
}

pub(super) fn task(
    feature_uid: &str,
    task_uid: &str,
    plan_hash: &str,
    parent: Option<String>,
) -> TaskRecord {
    TaskRecord {
        schema_version: 1,
        task_uid: task_uid.into(),
        feature_uid: feature_uid.into(),
        planning_item_uid: None,
        task_spec_sha256: None,
        approved_requirement_ids: Vec::new(),
        parent_task_uid: parent,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        phase: TaskPhase::Ready,
        revision: 0,
        approved_plan_sha256: plan_hash.into(),
        implementation_sha256: None,
        reviewed_code_head_sha: None,
        reviewed_code_subject_sha256: None,
        code_review: None,
        review_history: Vec::new(),
        repair_attempts: 0,
        runtime_qa: None,
        qa_execution: None,
        spec_reconciliation: None,
        spec_reconciliation_blocker: None,
        spec_reconciliation_proposal_sha256: None,
        spec_reconciliation_spec_sha256: None,
        spec_reconciliation_proposal_json: None,
        pr_work_item_uid: None,
    }
}
