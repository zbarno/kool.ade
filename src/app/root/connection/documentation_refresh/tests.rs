use super::enqueue;
use crate::{
    artifacts::planning_store::PlanningStore,
    core::{
        lifecycle::{FeatureGoal, FeaturePhase, LifecycleStore},
        planning_work::{Work, WorkKind},
    },
};

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "koolade-documentation-refresh-{}",
            uuid::Uuid::new_v4().hyphenated()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn store(&self) -> PlanningStore {
        PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &self.0)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Production boundary: a linked Documentation Refresh reply must create a
/// durable event referencing the exact user-local chat message. File-backed
/// lifecycle storage is exercised; no external service or QA evidence is used.
#[test]
fn documentation_refresh_work_reply_persists_message_correlation() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let feature_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let event_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    LifecycleStore::new(&store)
        .create_feature(
            &FeatureGoal {
                schema_version: 1,
                feature_uid: feature_uid.clone(),
                original_intent: "Document the approved feature".into(),
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
    let mut work = Work::new(
        "docs:feature".into(),
        "Refresh feature docs".into(),
        "Review current documentation".into(),
        "In progress".into(),
    );
    work.kind = WorkKind::DocumentationRefresh;
    work.feature_uid = Some(feature_uid.clone());
    let message_uid = "17aaf010-1-42".to_owned();

    enqueue(&store, Some(&work), &message_uid).unwrap();

    let trigger = LifecycleStore::new(&store)
        .list_spec_sync_queue(&feature_uid, false)
        .unwrap()
        .into_iter()
        .find(|entry| entry.request_message_id.as_deref() == Some(message_uid.as_str()))
        .expect("manual refresh trigger resolves to the original message");
    assert_eq!(
        trigger.trigger,
        crate::core::lifecycle::SpecSyncTriggerKindV1::ManualRefresh
    );
    assert_eq!(trigger.source_task_uid, None);
}
