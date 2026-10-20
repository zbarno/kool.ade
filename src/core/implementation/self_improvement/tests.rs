use super::*;
use crate::execution::{
    errors::{ExecutionError, ExecutionErrorKind},
    session::AuthorizationGenerationVerifier,
};

#[path = "production_dispatch.rs"]
mod production_dispatch;

#[derive(Debug)]
struct RevokedClaim;

impl AuthorizationGenerationVerifier for RevokedClaim {
    fn generation(&self) -> String {
        "claim-generation".into()
    }

    fn verify(&self, _generation: &str) -> Result<(), ExecutionError> {
        Err(ExecutionError::new(
            ExecutionErrorKind::PolicyDenied,
            "task claim is no longer owned",
            false,
        ))
    }
}

fn verifier(planning: PlanningStore) -> SelfImprovementExecutionVerifier {
    SelfImprovementExecutionVerifier {
        planning,
        state_root: std::path::PathBuf::new(),
        state_path: std::path::PathBuf::new(),
        planning_item_uid: uuid::Uuid::new_v4().to_string(),
        expected_run_uid: uuid::Uuid::new_v4().to_string(),
        expected_generation: uuid::Uuid::new_v4().hyphenated().to_string(),
        feature_uid: uuid::Uuid::new_v4().to_string(),
        task_uid: uuid::Uuid::new_v4().to_string(),
        repository_uid: uuid::Uuid::new_v4().to_string(),
        repository_remote: "https://github.com/zbarno/kool.ade".into(),
        source_ref: Some("feature/approved".into()),
        source_sha: Some("a".repeat(40)),
        destination_ref: Some("main".into()),
        prior_verifier: None,
        prior_generation: None,
    }
}

#[test]
fn execution_authorization_generation_is_a_canonical_uuid() {
    let run_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let generation = execution_generation(&run_uid, 3).unwrap();
    let parsed = uuid::Uuid::parse_str(&generation)
        .expect("execution generations must satisfy the session contract");

    assert_eq!(parsed.hyphenated().to_string(), generation);
    assert_ne!(generation, execution_generation(&run_uid, 4).unwrap());
}

#[test]
fn execution_verifier_rejects_a_different_authorization_generation() {
    let temp = tempfile::tempdir().unwrap();
    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.path().join("planning"),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let verifier = verifier(planning);

    let error = verifier.verify("stale-generation").unwrap_err();

    assert_eq!(error.kind, ExecutionErrorKind::PolicyDenied);
    assert!(error.message.contains("generation changed"));
}

#[test]
fn execution_verifier_fails_closed_when_persisted_authorization_disappears() {
    let temp = tempfile::tempdir().unwrap();
    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.path().join("planning"),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let verifier = verifier(planning);
    let generation = verifier.generation();

    let error = verifier.verify(&generation).unwrap_err();

    assert_eq!(error.kind, ExecutionErrorKind::PolicyDenied);
    assert!(error.message.contains("authorization disappeared"));
}

#[test]
fn execution_verifier_preserves_the_existing_task_claim_fence() {
    let temp = tempfile::tempdir().unwrap();
    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.path().join("planning"),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let mut verifier = verifier(planning);
    verifier.prior_verifier = Some(Arc::new(RevokedClaim));
    verifier.prior_generation = Some("claim-generation".into());

    let error = verifier.verify(&verifier.generation()).unwrap_err();

    assert_eq!(error.kind, ExecutionErrorKind::PolicyDenied);
    assert!(error.message.contains("claim is no longer owned"));
}

#[test]
fn plan_dispatch_requires_a_passing_linked_review_verdict() {
    let review_uid = uuid::Uuid::new_v4().to_string();
    let review_refs = vec![review_uid.clone()];

    assert!(require_approved_review_evidence(false, &review_uid, &review_refs).is_err());
    assert!(require_approved_review_evidence(true, &review_uid, &review_refs).is_ok());
    assert!(require_approved_review_evidence(true, &review_uid, &[]).is_err());
}
