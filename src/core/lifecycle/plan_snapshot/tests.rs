use super::*;
use crate::domain::ArtifactIdentity;

fn spec() -> String {
    "# CHG-021: Report exports\n\n## Intent\n\nHelp operators export reports.\n\n## Current Behavior\n\nReports cannot be exported.\n\n## Desired Behavior\n\nOperators can download reports.\n\n## Scope\n\n- CSV export\n- Out of scope: scheduled delivery\n\n## Affected Product Areas\n\nReporting and downloads.\n\n## Requirements\n\n- REQ-001: Exports include selected rows.\n- Keep the current filename behavior.\n\n## Decisions and Assumptions\n\n- CSV is sufficient for this release.\n\n## Acceptance Criteria\n\n1. Selected rows are exported.\n2. Empty results produce a valid file.\n".into()
}

fn identified(markdown: &str) -> (ArtifactIdentity, String) {
    let identified =
        ArtifactIdentity::preserve_markdown(markdown, None, "CHG-021", "Report exports").unwrap();
    let identity = ArtifactIdentity::from_markdown(&identified)
        .unwrap()
        .unwrap();
    (identity, identified)
}

fn plan_receipt(
    snapshot: &PlanSnapshotV1,
    decision: crate::core::lifecycle::GateDecision,
) -> crate::core::lifecycle::GateReceipt {
    use crate::core::lifecycle::{GateActor, GateReceipt};
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render};
    let author_run = snapshot
        .author_run_id
        .clone()
        .expect("test review plan has explicit author provenance");
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let manifest = render(&RoleInputV1 {
        role: PromptRole::PlanReviewer,
        workspace_uid: "fixture-workspace".into(),
        feature_uid: snapshot.feature_uid.clone(),
        task_uid: None,
        original_goal: "Adversarially review the user-approved plan".into(),
        approved_spec_ref: Some(snapshot.uid.clone()),
        approved_spec_hash: Some(snapshot.content_hash.clone()),
        decision_refs: Vec::new(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "plan-review/v1".into(),
        output_schema_version: PromptRole::PlanReviewer.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: snapshot.spec_revision.clone(),
            subject_hash: snapshot.content_hash.clone(),
            author_run_id: author_run.clone(),
            reviewer_run_id: reviewer_run.clone(),
            author_session_id: Some("00000000-0000-4000-8000-000000000005".into()),
            reviewer_session_id: Some("00000000-0000-4000-8000-000000000006".into()),
            requirements_matrix: snapshot
                .requirements
                .iter()
                .map(|req| req.text.clone())
                .collect(),
            explicit_exclusions: snapshot.non_goals.clone(),
        },
    })
    .unwrap()
    .manifest;
    GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: GateActor::IndependentReviewer,
        decision,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["plan-review/fixture".into()],
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(reviewer_run),
        author_run_id: Some(author_run),
        reviewer_session_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
        author_session_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
    }
}

fn persist_review(
    store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
    receipt: &crate::core::lifecycle::GateReceipt,
) {
    let (path, bytes, check) = review_change(store, snapshot, receipt).unwrap().unwrap();
    store
        .transaction_with_record_revisions(&[(path, bytes)], &[check])
        .unwrap();
}

fn persist_snapshot(
    store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) {
    let (path, bytes, check) = record_change(store, snapshot).unwrap().unwrap();
    store
        .transaction_with_record_revisions(&[(path, bytes)], &[check])
        .unwrap();
}

#[test]
fn snapshot_is_stable_content_addressed_and_detects_edits() {
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let first = PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    let repeated =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    assert_eq!(first, repeated);
    assert_eq!(first.requirements[0].stable_id, "REQ-001");
    assert_eq!(first.requirements[1].stable_id.len(), 16);
    assert_eq!(first.non_goals, ["Out of scope: scheduled delivery"]);

    let changed = markdown.replace("valid file", "valid CSV file");
    let changed_contract = crate::core::workflow::feature_contract(&changed);
    let changed_snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &changed, &changed_contract).unwrap();
    assert_ne!(first.content_hash, changed_snapshot.content_hash);
    assert_ne!(first.uid, changed_snapshot.uid);

    let mut corrupt = first;
    corrupt.outcome.push_str(" changed");
    assert!(corrupt.validate().is_err());
}

#[test]
fn snapshot_record_is_create_only_and_survives_reopen() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!("plan-snapshot-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    let (path, bytes, check) = record_change(&store, &snapshot).unwrap().unwrap();
    store
        .transaction_with_record_revisions(&[(path.clone(), bytes)], &[check])
        .unwrap();
    assert!(is_persisted(&store, &snapshot).unwrap());
    assert!(record_change(&store, &snapshot).unwrap().is_none());
    let reopened = PlanningStore::new(store.project_id, &root, StoreMode::ManagedLocal);
    assert!(is_persisted(&reopened, &snapshot).unwrap());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn user_confirmation_is_persisted_once_and_bound_to_the_exact_snapshot() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};
    let root = std::env::temp_dir().join(format!("plan-confirmation-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    persist_snapshot(&store, &snapshot);
    assert!(
        !crate::core::lifecycle::plan_snapshot::has_user_confirmation(&store, &snapshot).unwrap()
    );

    let receipt = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![format!("plan:{}", snapshot.uid)],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let mut forged = receipt.clone();
    forged.subject_sha256 = "b".repeat(64);
    assert!(user_confirmation_change(&store, &snapshot, &forged).is_err());
    let (path, bytes, check) = user_confirmation_change(&store, &snapshot, &receipt)
        .unwrap()
        .unwrap();
    store
        .transaction_with_record_revisions(&[(path, bytes)], &[check])
        .unwrap();
    assert!(
        crate::core::lifecycle::plan_snapshot::has_user_confirmation(&store, &snapshot).unwrap()
    );
    assert!(
        user_confirmation_change(&store, &snapshot, &receipt)
            .unwrap()
            .is_none()
    );
    let reopened = PlanningStore::new(store.project_id, &root, StoreMode::ManagedLocal);
    assert!(
        crate::core::lifecycle::plan_snapshot::has_user_confirmation(&reopened, &snapshot).unwrap()
    );

    let changed = markdown.replace("valid file", "valid CSV file");
    let changed_contract = crate::core::workflow::feature_contract(&changed);
    let changed_snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &changed, &changed_contract).unwrap();
    assert!(
        !crate::core::lifecycle::plan_snapshot::has_user_confirmation(&store, &changed_snapshot)
            .unwrap()
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn task_gate_requires_a_current_independent_approved_plan_review() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    use crate::core::lifecycle::GateDecision;
    let root = std::env::temp_dir().join(format!("plan-review-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.author_run_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    snapshot.author_session_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    persist_snapshot(&store, &snapshot);
    assert!(
        !has_approved_review(&store, &snapshot).unwrap(),
        "missing reviewer must block"
    );

    let stale = plan_receipt(&snapshot, GateDecision::Approved);
    let mut stale = stale;
    stale.subject_sha256 = "b".repeat(64);
    assert!(
        review_change(&store, &snapshot, &stale).is_err(),
        "stale plan hash must be rejected"
    );

    let mut same_author = plan_receipt(&snapshot, GateDecision::Approved);
    same_author.reviewer_run_id = same_author.author_run_id.clone();
    assert!(
        review_change(&store, &snapshot, &same_author).is_err(),
        "same author cannot review"
    );

    let rejected = plan_receipt(&snapshot, GateDecision::Rejected);
    persist_review(&store, &snapshot, &rejected);
    assert!(
        !has_approved_review(&store, &snapshot).unwrap(),
        "rejected review must block"
    );

    let next_root =
        std::env::temp_dir().join(format!("plan-review-approved-{}", uuid::Uuid::new_v4()));
    let approved_store =
        PlanningStore::new(uuid::Uuid::new_v4(), &next_root, StoreMode::ManagedLocal);
    persist_snapshot(&approved_store, &snapshot);
    let approved = plan_receipt(&snapshot, GateDecision::Approved);
    persist_review(&approved_store, &snapshot, &approved);
    assert!(has_approved_review(&approved_store, &snapshot).unwrap());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(next_root);
}
