use super::*;
use crate::domain::ArtifactIdentity;

#[path = "tests/confirmation_concurrency.rs"]
mod confirmation_concurrency;
#[path = "tests/deferred_context.rs"]
mod deferred_context;
#[path = "tests/review_evidence.rs"]
mod review_evidence;
#[path = "tests/review_gate.rs"]
mod review_gate;
#[path = "tests/review_retry.rs"]
mod review_retry;
#[path = "tests/snapshot_content.rs"]
mod snapshot_content;

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

#[test]
fn capability_id_is_validated_and_bound_to_the_confirmed_snapshot_hash() {
    let markdown = format!(
        "{}\n## Capability ID\n\nself-improvement-lifecycle\n",
        spec()
    );
    let (identity, markdown) = identified(&markdown);
    let contract = crate::core::workflow::feature_contract(&markdown);
    let snapshot = PlanSnapshotV1::from_feature_with_original_goal(
        "CHG-021",
        &identity,
        &markdown,
        &contract,
        "planning_work:fixture:request_sha256:abc",
        "Add self-improvement lifecycle support",
    )
    .unwrap();
    assert_eq!(
        snapshot.capability_id.as_deref(),
        Some("self-improvement-lifecycle")
    );

    let mut tampered = snapshot.clone();
    tampered.capability_id = Some("other-capability".into());
    assert!(tampered.validate().is_err());

    let invalid = format!("{}\n## Capability ID\n\nSelf-Improvement\n", spec());
    let (identity, invalid) = identified(&invalid);
    let contract = crate::core::workflow::feature_contract(&invalid);
    assert!(
        PlanSnapshotV1::from_feature_with_original_goal(
            "CHG-021",
            &identity,
            &invalid,
            &contract,
            "planning_work:fixture:request_sha256:abc",
            "Add a capability",
        )
        .is_err()
    );
}

#[test]
fn capability_without_original_goal_uses_the_legacy_compatible_schema() {
    let markdown = format!(
        "{}\n## Capability ID\n\nself-improvement-lifecycle\n",
        spec()
    );
    let (identity, markdown) = identified(&markdown);
    let contract = crate::core::workflow::feature_contract(&markdown);
    let snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();

    assert_eq!(snapshot.schema_version, 2);
    assert!(snapshot.original_goal.is_empty());
    assert_eq!(
        snapshot.capability_id.as_deref(),
        Some("self-improvement-lifecycle")
    );
    snapshot.validate().unwrap();
}

fn legacy_snapshot() -> PlanSnapshotV1 {
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.schema_version = 1;
    snapshot.original_goal.clear();
    snapshot.content_hash = hash::content_hash(&snapshot).unwrap();
    snapshot.uid = snapshot_uid(&snapshot.feature_uid, &snapshot.content_hash).unwrap();
    snapshot
}

#[test]
fn legacy_snapshot_without_original_goal_remains_valid() {
    legacy_snapshot().validate().unwrap();
}

#[test]
fn persisted_legacy_snapshot_loads_with_original_hash_and_uid() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    let root = std::env::temp_dir().join(format!("legacy-plan-snapshot-{}", uuid::Uuid::new_v4()));
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let snapshot = legacy_snapshot();
    let path = format!("state/lifecycle_plans/{}.json", snapshot.uid);
    let record = serde_json::json!({
        "schemaVersion": 1,
        "uid": snapshot.uid,
        "createdAtMs": 1,
        "updatedAtMs": 1,
        "revision": 1,
        "data": snapshot.clone(),
    });
    store
        .transaction(&[(path, serde_json::to_vec(&record).unwrap())], None)
        .unwrap();
    let loaded = crate::core::lifecycle::plan_snapshot::load_persisted(&store, &snapshot.uid)
        .unwrap()
        .unwrap();
    assert_eq!(loaded, snapshot);
    let _ = std::fs::remove_dir_all(root);
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
            author_session_id: snapshot.author_session_id.clone(),
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
        evidence_refs: vec![
            "plan-review/fixture".into(),
            format!("plan:{}", snapshot.uid),
            "product-spec:reporting-contract".into(),
        ],
        prompt_sha256: Some(manifest.identity_sha256()),
        prompt_manifest: Some(manifest),
        reviewer_run_id: Some(reviewer_run),
        author_run_id: Some(author_run),
        reviewer_session_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
        author_session_id: snapshot.author_session_id.clone(),
    }
}

fn persist_review(
    store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
    receipt: &crate::core::lifecycle::GateReceipt,
    verdict: &crate::harness::prompt_catalog::ReviewVerdictV1,
) {
    let (path, bytes, check) = review_change_with_verdict(store, snapshot, receipt, verdict)
        .unwrap()
        .unwrap();
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

fn persist_user_confirmation(
    store: &crate::artifacts::planning_store::PlanningStore,
    snapshot: &PlanSnapshotV1,
) {
    use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};
    let receipt = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: snapshot.uid.clone(),
        subject_sha256: snapshot.content_hash.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![
            format!("plan:{}", snapshot.uid),
            format!(
                "product-spec:{}",
                crate::core::lifecycle::product_spec::current_product_hash(store).unwrap()
            ),
        ],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let (path, bytes, check) = user_confirmation_change(store, snapshot, &receipt)
        .unwrap()
        .unwrap();
    store
        .transaction_with_record_revisions(&[(path, bytes)], &[check])
        .unwrap();
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
    let reopened = PlanningStore::new(
        store.project_id,
        store.root.clone(),
        StoreMode::ManagedLocal,
    );
    assert!(is_persisted(&reopened, &snapshot).unwrap());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn user_confirmation_is_persisted_once_and_bound_to_the_exact_snapshot() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};
    let root = std::env::temp_dir().join(format!("plan-confirmation-{}", uuid::Uuid::new_v4()));
    crate::artifacts::migration::bootstrap_product(&root, "Plan Confirmation Fixture").unwrap();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.join(".koolade-packet"),
        StoreMode::ManagedLocal,
    );
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
        evidence_refs: vec![
            format!("plan:{}", snapshot.uid),
            format!(
                "product-spec:{}",
                crate::core::lifecycle::product_spec::current_product_hash(&store).unwrap()
            ),
        ],
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
    let reopened = PlanningStore::new(
        store.project_id,
        store.root.clone(),
        StoreMode::ManagedLocal,
    );
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
