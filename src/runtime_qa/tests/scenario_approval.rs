use super::{request, scenario};
use crate::runtime_qa::RuntimeQaStore;

#[test]
fn approved_scenarios_are_durable_and_bound_to_user_receipt_and_request() {
    use sha2::{Digest, Sha256};
    let request = request();
    let scenarios = vec![scenario(&request)];
    let encoded = serde_json::to_vec(&scenarios).unwrap();
    let scenario_hash = format!("{:x}", Sha256::digest(encoded));
    let approval = crate::core::lifecycle::GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: request.parent_task_uid.clone(),
        subject_sha256: scenario_hash,
        actor: crate::core::lifecycle::GateActor::User,
        decision: crate::core::lifecycle::GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![format!(
            "runtime-qa:{}/approved-scenarios",
            request.qa_run_id
        )],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let root = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(root.path().join("qa"));
    store
        .approve_scenarios(&request, &scenarios, &approval)
        .unwrap();
    assert_eq!(
        store.load_approved_scenarios(&request).unwrap().scenarios,
        scenarios
    );
    let mut stale = request.clone();
    stale.code_head_sha = "f".repeat(64);
    assert!(store.load_approved_scenarios(&stale).is_err());
}
