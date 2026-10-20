use crate::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{GateActor, GateDecision, GateReceipt, PlanSnapshotV1},
    harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render},
};
use sha2::Digest;
use std::process::Command;

pub(crate) fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub(crate) fn approved_plan(store: &PlanningStore) -> (PlanSnapshotV1, GateReceipt, GateReceipt) {
    let markdown = "# CHG-175: Evolve authorization\n\n## Intent\n\nGate product task execution.\n\n## Current Behavior\n\nProduct work has normal lifecycle approval.\n\n## Desired Behavior\n\nProduct task dispatch checks persisted authorization.\n\n## Scope\n\n- Bind execution to reviewed plan\n\n## Affected Product Areas\n\nImplementation controller.\n\n## Capability ID\n\nself-improvement-lifecycle\n\n## Requirements\n\n- REQ-001 (MUST). Dispatch only the approved task source. Rationale: prevent unauthorized code execution. ACs: AC-001\n\n## Decisions and Assumptions\n\n- Use the persisted reviewed plan.\n\n## Acceptance Criteria\n\n1. The approved task source is checked before execution.\n";
    let markdown = crate::domain::ArtifactIdentity::preserve_markdown(
        markdown,
        None,
        "CHG-175",
        "Evolve authorization",
    )
    .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .unwrap();
    let contract = crate::core::workflow::feature_contract(&markdown);
    let author = crate::core::lifecycle::plan_snapshot::PlanAuthorProvenanceV1 {
        feature_uid: identity.uid.clone(),
        approval_contract_sha256: format!("{:x}", sha2::Sha256::digest(contract.as_bytes())),
        author_run_id: uuid::Uuid::new_v4().to_string(),
        author_session_id: uuid::Uuid::new_v4().to_string(),
    };
    crate::core::lifecycle::plan_snapshot::record_author(store, &author).unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        store, "CHG-175", &identity, &markdown, &contract,
    )
    .unwrap();
    persist(
        store,
        crate::core::lifecycle::plan_snapshot::record_change(store, &snapshot).unwrap(),
    );
    let product_hash = crate::core::lifecycle::product_spec::current_product_hash(store).unwrap();
    let user = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![
            format!("plan:{}", snapshot.uid),
            format!("product-spec:{product_hash}"),
        ],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    persist(
        store,
        crate::core::lifecycle::plan_snapshot::user_confirmation_change(store, &snapshot, &user)
            .unwrap(),
    );
    let reviewer_uid = uuid::Uuid::new_v4().to_string();
    let reviewer_session = uuid::Uuid::new_v4().to_string();
    let author_session = snapshot.author_session_id.clone().unwrap();
    let author_run = snapshot.author_run_id.clone().unwrap();
    let manifest = render(&RoleInputV1 {
        role: PromptRole::PlanReviewer,
        workspace_uid: store.project_id.hyphenated().to_string(),
        feature_uid: snapshot.feature_uid.clone(),
        task_uid: None,
        original_goal: snapshot.intent.clone(),
        approved_spec_ref: Some(snapshot.uid.clone()),
        approved_spec_hash: Some(snapshot.content_hash.clone()),
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "test/independent-plan-review/v1".into(),
        output_schema_version: PromptRole::PlanReviewer.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: snapshot.spec_revision.clone(),
            subject_hash: snapshot.content_hash.clone(),
            author_run_id: author_run.clone(),
            reviewer_run_id: reviewer_uid.clone(),
            author_session_id: Some(author_session.clone()),
            reviewer_session_id: Some(reviewer_session.clone()),
            requirements_matrix: snapshot
                .requirements
                .iter()
                .map(|item| item.text.clone())
                .collect(),
            explicit_exclusions: snapshot.non_goals.clone(),
        },
    })
    .unwrap()
    .manifest;
    let review = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: GateActor::IndependentReviewer,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 2,
        evidence_refs: std::iter::once(format!("plan:{}", snapshot.uid))
            .chain(
                snapshot
                    .requirements
                    .iter()
                    .map(|item| format!("requirement:{}", item.stable_id)),
            )
            .chain(std::iter::once("fixture/review-evidence".into()))
            .collect(),
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(reviewer_uid),
        author_run_id: Some(author_run),
        reviewer_session_id: Some(reviewer_session),
        author_session_id: Some(author_session),
    };
    let verdict = crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: snapshot.content_hash.clone(),
        pass: true,
        outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass),
        coverage: snapshot
            .requirements
            .iter()
            .map(|item| crate::harness::prompt_catalog::CoverageV1 {
                requirement: item.stable_id.clone(),
                status: crate::harness::prompt_catalog::CoverageStatus::Pass,
                evidence: vec![format!("requirement:{}", item.stable_id)],
            })
            .collect(),
        findings: Vec::new(),
        untested: Vec::new(),
    };
    persist(
        store,
        crate::core::lifecycle::plan_snapshot::review_change_with_verdict(
            store, &snapshot, &review, &verdict,
        )
        .unwrap(),
    );
    (snapshot, user, review)
}

fn persist(
    store: &PlanningStore,
    change: Option<(
        String,
        Vec<u8>,
        crate::artifacts::planning_store::RecordRevisionCheck,
    )>,
) {
    if let Some((path, bytes, check)) = change {
        store
            .transaction_with_record_revisions(&[(path, bytes)], &[check])
            .unwrap();
    }
}
