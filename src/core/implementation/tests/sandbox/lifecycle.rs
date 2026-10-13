use std::path::Path;

use crate::core::lifecycle::{TaskPhase, TaskRecord};
use sha2::Digest;

pub(super) fn seed_task(repo: &Path, ticket: &str, ticket_text: &str) {
    let task_uid = crate::domain::ArtifactIdentity::from_markdown(
        &std::fs::read_to_string(repo.join(ticket)).unwrap(),
    )
    .unwrap()
    .unwrap()
    .uid;
    let approved_plan_sha256 = format!("{:x}", sha2::Sha256::digest(b"approved fixture plan"));
    let task_spec_sha256 = format!(
        "{:x}",
        sha2::Sha256::digest(
            serde_json::to_vec(&(1_u16, approved_plan_sha256.as_str(), ticket_text)).unwrap()
        )
    );
    let record = TaskRecord {
        schema_version: 1,
        task_uid: task_uid.clone(),
        feature_uid: uuid::Uuid::new_v4().to_string(),
        planning_item_uid: Some(task_uid.clone()),
        task_spec_sha256: Some(task_spec_sha256),
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        phase: TaskPhase::Ready,
        revision: 0,
        approved_plan_sha256,
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
    };
    let wrapper = serde_json::json!({
        "uid": task_uid,
        "schemaVersion": 1,
        "createdAtMs": 1,
        "updatedAtMs": 1,
        "revision": 0,
        "record": record
    });
    let store =
        crate::artifacts::planning_store::PlanningStore::legacy_embedded(uuid::Uuid::nil(), repo);
    store
        .transaction(
            &[(
                format!("state/lifecycle_tasks/{task_uid}.json"),
                serde_json::to_vec(&wrapper).unwrap(),
            )],
            None,
        )
        .unwrap();
}

pub(super) fn refresh_unstarted_task_spec(repo: &Path, ticket: &str) {
    let ticket_text = crate::artifacts::task_docs::visible_content(
        &std::fs::read_to_string(repo.join(ticket)).unwrap(),
    );
    let Ok(Some(identity)) = crate::domain::ArtifactIdentity::from_markdown(
        &std::fs::read_to_string(repo.join(ticket)).unwrap(),
    ) else {
        return;
    };
    let path = format!("state/lifecycle_tasks/{}.json", identity.uid);
    let store =
        crate::artifacts::planning_store::PlanningStore::legacy_embedded(uuid::Uuid::nil(), repo);
    let Ok(bytes) = store.read(&path) else {
        return;
    };
    let mut wrapper: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let record = wrapper.get_mut("record").unwrap();
    if record["phase"] != "ready" || record["revision"] != 0 {
        return;
    }
    let Some(plan_hash) = record["approvedPlanSha256"].as_str() else {
        return;
    };
    let task_spec_sha256 = format!(
        "{:x}",
        sha2::Sha256::digest(
            serde_json::to_vec(&(1_u16, plan_hash, ticket_text.as_str())).unwrap()
        )
    );
    record["taskSpecSha256"] = serde_json::Value::String(task_spec_sha256);
    store
        .transaction(&[(path, serde_json::to_vec(&wrapper).unwrap())], None)
        .unwrap();
}
