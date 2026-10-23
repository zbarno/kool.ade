use super::*;
use crate::{
    artifacts::{
        migration,
        planning_store::{PlanningStore, StoreMode},
    },
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, LifecycleStore, PullRequestPreparation,
        PullRequestPreparationStatus,
    },
};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Persisted<T> {
    uid: String,
    schema_version: u16,
    created_at_ms: u64,
    updated_at_ms: u64,
    revision: u64,
    record: T,
}

#[test]
// Production boundary: history validation only admits evidence loaded from a
// durable PR record whose feature/task/included-task linkage matches. File-backed
// PlanningStore; no external service or expensive setup.
fn persisted_pr_reference_is_admitted_and_forged_linkage_is_rejected() {
    let root = std::env::temp_dir().join(format!(
        "koolade-pr-evidence-{}",
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
    migration::bootstrap_product(&root, "PR evidence fixture").unwrap();
    let store = PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.join(".koolade-packet"),
        StoreMode::ManagedLocal,
    );
    let feature_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let work_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let preparation = PullRequestPreparation {
        schema_version: 1,
        uid: work_uid.clone(),
        work_item_uid: work_uid.clone(),
        feature_uid: feature_uid.clone(),
        task_uid: task_uid.clone(),
        included_task_uids: vec![task_uid.clone()],
        revision: 1,
        created_at_ms: 1,
        updated_at_ms: 2,
        status: PullRequestPreparationStatus::Merged,
        publication_authorization: Some(GateReceipt {
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            subject_uid: work_uid.clone(),
            subject_sha256: "d".repeat(64),
            actor: GateActor::User,
            decision: GateDecision::Approved,
            recorded_at_unix_ms: 2,
            evidence_refs: vec![format!("user-action:create-pr:{work_uid}")],
            prompt_sha256: None,
            prompt_manifest: None,
            reviewer_run_id: None,
            author_run_id: None,
            reviewer_session_id: None,
            author_session_id: None,
        }),
        publication_scope: None,
        lease_epoch: 0,
        publication_authorization_event_uids: Vec::new(),
        recovery: None,
        provider_pr_id: Some(9),
        provider_pr_url: Some("https://github.com/example/project/pull/9".into()),
        head_sha: Some("a".repeat(40)),
        base_sha: Some("b".repeat(40)),
        merged_commit_sha: Some("c".repeat(40)),
    };
    let persisted = Persisted {
        uid: work_uid.clone(),
        schema_version: 1,
        created_at_ms: 1,
        updated_at_ms: 2,
        revision: 1,
        record: preparation,
    };
    store
        .save_record(
            &format!("state/lifecycle_pr_work_items/{work_uid}.json"),
            &persisted,
            0,
        )
        .unwrap();

    let lifecycle = LifecycleStore::new(&store);
    assert_eq!(
        persisted_provider_pr_state_evidence_ref(&lifecycle, &feature_uid, &task_uid, &work_uid)
            .unwrap(),
        format!("provider-pr-state:{work_uid}:1")
    );
    assert!(
        persisted_provider_pr_state_evidence_ref(
            &lifecycle,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            &task_uid,
            &work_uid,
        )
        .is_err()
    );
    assert!(
        persisted_provider_pr_state_evidence_ref(
            &lifecycle,
            &feature_uid,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            &work_uid,
        )
        .is_err()
    );
    assert!(
        persisted_provider_pr_state_evidence_ref(
            &lifecycle,
            &feature_uid,
            &task_uid,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
        )
        .is_err()
    );
    let _ = std::fs::remove_dir_all(root);
}
