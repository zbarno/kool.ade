use super::*;

#[test]
fn independent_review_receipts_require_a_current_prompt_and_artifact_hash() {
    let mut receipt = receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &id(),
    );
    assert!(receipt.validate().is_ok());
    receipt.subject_sha256 = "not-a-digest".into();
    assert!(receipt.validate().is_err());
    receipt.subject_sha256 = "a".repeat(64);
    receipt.prompt_sha256 = None;
    assert!(receipt.validate().is_err());
    set_prompt_role(
        &mut receipt,
        crate::harness::prompt_catalog::PromptRole::CodeReviewer,
    );
    receipt.prompt_manifest.as_mut().unwrap().role_version = "code_review/v99".into();
    assert!(receipt.validate().is_err());
}
