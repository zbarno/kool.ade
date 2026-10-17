use super::{request, scenario};
use crate::runtime_qa::RuntimeQaStore;

#[test]
fn synthesized_proposal_survives_restart_without_authorizing_execution() {
    let request = request();
    let proposal =
        crate::runtime_qa::QaScenarioProposalV1::new(&request, 4, vec![scenario(&request)])
            .unwrap();
    let root = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(root.path().join("qa"));

    store.save_scenario_proposal(&request, &proposal).unwrap();
    let reloaded_store = RuntimeQaStore::new(root.path().join("qa"));
    assert_eq!(
        reloaded_store.load_scenario_proposal(&request).unwrap(),
        proposal
    );
    assert!(reloaded_store.load_approved_scenarios(&request).is_err());

    let mut stale_request = request.clone();
    stale_request.code_head_sha = "f".repeat(64);
    assert!(
        reloaded_store
            .load_scenario_proposal(&stale_request)
            .is_err()
    );
}

#[test]
fn proposal_cannot_be_rewritten_or_have_scenarios_changed_after_hashing() {
    let request = request();
    let mut proposal =
        crate::runtime_qa::QaScenarioProposalV1::new(&request, 4, vec![scenario(&request)])
            .unwrap();
    proposal.scenarios[0].expected_results[0] = "Changed after proposal".into();
    assert!(proposal.validate_for(&request).is_err());
}

#[test]
fn proposal_approval_binds_exact_hash_and_task_revision_before_dispatch_is_eligible() {
    use crate::core::lifecycle::{GateActor, GateDecision, GateReceipt};
    let request = request();
    let proposal =
        crate::runtime_qa::QaScenarioProposalV1::new(&request, 7, vec![scenario(&request)])
            .unwrap();
    let proposal_sha256 = proposal.proposal_sha256().unwrap();
    let approval = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: request.parent_task_uid.clone(),
        subject_sha256: proposal.scenario_sha256.clone(),
        actor: GateActor::User,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![
            format!("runtime-qa:{}/approved-scenarios", request.qa_run_id),
            format!(
                "runtime-qa:{}/proposal:{proposal_sha256}:task-revision:7",
                request.qa_run_id
            ),
        ],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let root = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(root.path().join("qa"));
    store.save_scenario_proposal(&request, &proposal).unwrap();
    let expected_approval_ref = format!(
        "runtime-qa:{}/proposal:{proposal_sha256}:task-revision:7",
        request.qa_run_id
    );
    assert_eq!(proposal.task_revision, 7);
    assert_eq!(approval.subject_sha256, proposal.scenario_sha256);
    assert!(approval.evidence_refs.contains(&expected_approval_ref));
    assert!(store.load_approved_scenario_proposal(&request, 8).is_err());
    assert!(
        store
            .approve_scenario_proposal(&request, &proposal, 8, &approval)
            .is_err()
    );
    store
        .approve_scenario_proposal(&request, &proposal, 7, &approval)
        .unwrap();
    let reloaded_proposal = store.load_scenario_proposal(&request).unwrap();
    assert_eq!(
        reloaded_proposal.proposal_sha256().unwrap(),
        proposal_sha256
    );
    let reloaded_approval = store.load_approved_scenarios(&request).unwrap();
    assert!(
        reloaded_approval
            .approval
            .evidence_refs
            .contains(&expected_approval_ref)
    );
    store
        .load_approved_scenario_proposal(&request, 7)
        .unwrap_or_else(|error| panic!("approved proposal did not reload: {error}"));
    assert!(store.load_approved_scenario_proposal(&request, 8).is_err());
}

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
