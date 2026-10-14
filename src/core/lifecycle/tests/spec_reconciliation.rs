use super::*;

#[test]
fn spec_delta_approval_is_bound_to_user_proposal_and_both_spec_revisions() {
    let task_uid = id();
    let proposal_sha256 = "a".repeat(64);
    let mut receipt = receipt(GateActor::User, GateDecision::Approved, &task_uid);
    receipt.subject_sha256 = proposal_sha256.clone();
    let approval = SpecDeltaApprovalV1 {
        schema_version: 1,
        task_uid: task_uid.clone(),
        expected_task_revision: 7,
        proposal_sha256: proposal_sha256.clone(),
        base_spec_sha256: "b".repeat(64),
        resulting_spec_sha256: "c".repeat(64),
        receipt,
    };
    approval.validate().unwrap();

    let mut forged = approval.clone();
    forged.receipt.actor = GateActor::ApplicationPolicy;
    assert!(forged.validate().is_err());
    let mut rejected = approval.clone();
    rejected.receipt.decision = GateDecision::Rejected;
    assert!(rejected.validate().is_err());
    let mut wrong_subject = approval;
    wrong_subject.receipt.subject_sha256 = "d".repeat(64);
    assert!(wrong_subject.validate().is_err());
}
