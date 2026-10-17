use super::*;
use crate::{
    artifacts::{migration, planning_store::PlanningStore},
    core::lifecycle::{
        FeatureGoal, FeaturePhase, SpecReconciliationStatusV1, SpecSyncQueueStatusV1,
    },
};
use sha2::Digest as _;

#[path = "tests/concurrent.rs"]
mod concurrent;
#[path = "tests/manual_refresh.rs"]
mod manual_refresh;
#[path = "tests/origins.rs"]
mod origins;
#[path = "tests/reverse_references.rs"]
mod reverse_references;
#[path = "tests/traceability.rs"]
mod traceability;

// File-backed PlanningStore tests cover production transaction/CAS behavior
// that unit tests cannot; synthetic fixtures use local `git init` with moderate setup.
struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "koolade-spec-history-{}",
            uuid::Uuid::new_v4().hyphenated()
        ));
        std::fs::create_dir_all(&root).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
        migration::bootstrap_product(&root, "Spec History Fixture").unwrap();
        Self(root)
    }

    fn store(&self) -> PlanningStore {
        PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &self.0)
    }

    fn feature(store: &PlanningStore) -> (String, String) {
        let uid = uuid::Uuid::new_v4().hyphenated().to_string();
        let event_uid = uuid::Uuid::new_v4().hyphenated().to_string();
        LifecycleStore::new(store)
            .create_feature(
                &FeatureGoal {
                    schema_version: 1,
                    feature_uid: uid.clone(),
                    original_intent: "Keep reconciliation history durable".into(),
                    draft: String::new(),
                    scope: Vec::new(),
                    non_goals: Vec::new(),
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
                },
                &event_uid,
            )
            .unwrap();
        (uid, event_uid)
    }

    fn draft(store: &PlanningStore, feature_revision: u64, origin: String) -> SpecRevisionDraftV1 {
        let spec = crate::artifacts::product_docs::render_product(store)
            .unwrap()
            .unwrap();
        let content_hash = format!(
            "{:x}",
            sha2::Sha256::digest(serde_json::to_vec(&Some(spec)).unwrap())
        );
        SpecRevisionDraftV1 {
            no_change_reason: Some(
                "Synthetic fixture records an explicit unchanged-specification result.".into(),
            ),
            content_hash,
            source_revision: feature_revision,
            produced_by_run_id: None,
            origin_events: vec![origin],
            decision_refs: Vec::new(),
            delta_approval_refs: Vec::new(),
            approved_target_hash: None,
            implementation_head_sha: None,
            reconciliation_status: SpecReconciliationStatusV1::Synchronized,
            outcome_reason: None,
            deltas: Vec::new(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
// Invariant: current and immutable revisions persist atomically, exact retries
// are idempotent, and stale expected sequences cannot overwrite newer state.
fn revision_and_delta_history_are_atomic_idempotent_and_optimistically_fenced() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (feature_uid, _) = Fixture::feature(&store);
    let lifecycle = LifecycleStore::new(&store);
    let refresh = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "history-seed")
        .unwrap();
    let draft = Fixture::draft(&store, 1, refresh.uid);
    let written = lifecycle
        .append_spec_revision(&feature_uid, 0, draft.clone())
        .unwrap();
    assert_eq!(written.seq, 1);
    assert_eq!(
        lifecycle.load_spec_revision(&feature_uid).unwrap(),
        Some(written.clone())
    );
    let aggregate = lifecycle.product_specification_aggregate().unwrap();
    assert_eq!(aggregate.lifecycle_features.len(), 1);
    assert_eq!(aggregate.lifecycle_features[0].revision, Some(1));
    assert_eq!(aggregate.rendered_product_hash, written.content_hash);
    assert!(aggregate.findings.is_empty());
    assert_eq!(
        lifecycle.spec_revision_history(&feature_uid).unwrap().len(),
        1
    );
    assert_eq!(
        lifecycle
            .list_spec_sync_queue(&feature_uid, true)
            .unwrap()
            .into_iter()
            .find(|entry| entry.source_event_uid == written.origin_events[0])
            .unwrap()
            .status,
        SpecSyncQueueStatusV1::Completed
    );
    assert_eq!(
        lifecycle
            .append_spec_revision(&feature_uid, 0, draft.clone())
            .unwrap(),
        written
    );

    let next_refresh = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "history-stale-writer")
        .unwrap();
    let stale = Fixture::draft(&store, 1, next_refresh.uid);
    assert!(matches!(
        lifecycle.append_spec_revision(&feature_uid, 0, stale),
        Err(StoreError::StaleRecordRevision { .. })
    ));
    assert_eq!(
        lifecycle.spec_revision_history(&feature_uid).unwrap().len(),
        1
    );
}

#[test]
// Invariant: a needs-decision result does not acknowledge its source trigger.
fn human_decision_revision_does_not_acknowledge_its_trigger() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (feature_uid, _) = Fixture::feature(&store);
    let lifecycle = LifecycleStore::new(&store);
    let refresh = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "human-decision-seed")
        .unwrap();
    let mut draft = Fixture::draft(&store, 1, refresh.uid);
    draft.reconciliation_status = SpecReconciliationStatusV1::NeedsHumanDecision;
    draft.outcome_reason = Some("approval required".into());
    let origin = draft.origin_events[0].clone();
    lifecycle
        .append_spec_revision(&feature_uid, 0, draft)
        .unwrap();
    assert_eq!(
        lifecycle
            .list_spec_sync_queue(&feature_uid, true)
            .unwrap()
            .into_iter()
            .find(|entry| entry.source_event_uid == origin)
            .unwrap()
            .status,
        SpecSyncQueueStatusV1::Pending
    );
}

#[test]
// Invariant: origin IDs must resolve to this feature's durable queue records.
fn revision_rejects_unresolved_origin_references() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (feature_uid, _) = Fixture::feature(&store);
    let lifecycle = LifecycleStore::new(&store);
    let refresh = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "invalid-origin-seed")
        .unwrap();
    let mut draft = Fixture::draft(&store, 1, refresh.uid);
    draft.origin_events = vec![uuid::Uuid::new_v4().hyphenated().to_string()];
    assert!(matches!(
        lifecycle.append_spec_revision(&feature_uid, 0, draft),
        Err(StoreError::Io { .. }) | Err(StoreError::MalformedState(_))
    ));
    assert!(
        lifecycle
            .load_spec_revision(&feature_uid)
            .unwrap()
            .is_none()
    );
}

#[test]
// Invariant: a Git object ID needs a current passing review for this feature.
fn revision_rejects_a_syntactically_valid_but_unreviewed_implementation_head() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (feature_uid, _) = Fixture::feature(&store);
    let lifecycle = LifecycleStore::new(&store);
    let refresh = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "forged-head-seed")
        .unwrap();
    let mut draft = Fixture::draft(&store, 1, refresh.uid);
    draft.implementation_head_sha = Some("a".repeat(40));
    assert!(matches!(
        lifecycle.append_spec_revision(&feature_uid, 0, draft),
        Err(StoreError::MalformedState(message))
            if message.contains("current passing code review")
    ));
    assert!(
        lifecycle
            .load_spec_revision(&feature_uid)
            .unwrap()
            .is_none()
    );
}

#[test]
// Invariant: a valid-looking target hash cannot become approved without a
// persisted user decision or delta-approval receipt.
fn revision_rejects_an_unapproved_target_hash() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (feature_uid, _) = Fixture::feature(&store);
    let lifecycle = LifecycleStore::new(&store);
    let refresh = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "forged-target-seed")
        .unwrap();
    let mut draft = Fixture::draft(&store, 1, refresh.uid);
    draft.approved_target_hash = Some("b".repeat(64));
    assert!(matches!(
        lifecycle.append_spec_revision(&feature_uid, 0, draft),
        Err(StoreError::MalformedState(message))
            if message.contains("initial approved target requires")
    ));
    assert!(
        lifecycle
            .load_spec_revision(&feature_uid)
            .unwrap()
            .is_none()
    );
}

#[path = "tests/reconfirmation.rs"]
mod reconfirmation;
