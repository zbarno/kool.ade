use crate::artifacts::planning_store::PlanningStore;
use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};
use serde::Serialize;

pub(in crate::app::root::specification_trace_navigation::tests) fn save_pr_state(
    store: &PlanningStore,
    work_uid: &str,
    feature_uid: &str,
    task_uid: &str,
) -> String {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Envelope<T> {
        uid: String,
        schema_version: u16,
        created_at_ms: u64,
        updated_at_ms: u64,
        revision: u64,
        record: T,
    }
    let record = crate::core::lifecycle::PullRequestPreparation {
        schema_version: 1,
        uid: work_uid.into(),
        work_item_uid: work_uid.into(),
        feature_uid: feature_uid.into(),
        task_uid: task_uid.into(),
        included_task_uids: vec![task_uid.into()],
        revision: 1,
        created_at_ms: 1,
        updated_at_ms: 1,
        status: crate::core::lifecycle::PullRequestPreparationStatus::Opened,
        publication_authorization: Some(GateReceipt {
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            subject_uid: work_uid.into(),
            subject_sha256: "3".repeat(64),
            actor: GateActor::User,
            decision: GateDecision::Approved,
            recorded_at_unix_ms: 1,
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
        provider_pr_id: Some(901),
        provider_pr_url: Some("https://github.com/example/trace-fixture/pull/901".into()),
        head_sha: Some("4".repeat(40)),
        base_sha: None,
        merged_commit_sha: None,
    };
    let path = format!("state/lifecycle_pr_work_items/{work_uid}.json");
    store
        .save_record(
            &path,
            &Envelope {
                uid: work_uid.into(),
                schema_version: 1,
                created_at_ms: 1,
                updated_at_ms: 1,
                revision: 1,
                record,
            },
            0,
        )
        .unwrap();
    format!("provider-pr-state:{work_uid}:1")
}
