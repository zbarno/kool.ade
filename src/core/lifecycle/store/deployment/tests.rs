use super::{
    CurrentImplementationEvidenceV1, DeploymentEvidenceV1, DeploymentObservation,
    matches_observation,
};

fn current() -> CurrentImplementationEvidenceV1 {
    CurrentImplementationEvidenceV1 {
        schema_version: 1,
        uid: "11111111-1111-4111-8111-111111111111".into(),
        feature_uid: "11111111-1111-4111-8111-111111111111".into(),
        task_uid: "22222222-2222-4222-8222-222222222222".into(),
        work_item_uid: "33333333-3333-4333-8333-333333333333".into(),
        repository_id: "fixture-repo".into(),
        base_commit_sha: "a".repeat(40),
        pull_request_head_sha: "b".repeat(40),
        merge_commit_sha: "c".repeat(40),
        checked_out_head_sha: "c".repeat(40),
        specification_revision: 4,
        specification_hash: "d".repeat(64),
        revision: 2,
        recorded_at_ms: 100,
    }
}

fn deployment() -> DeploymentEvidenceV1 {
    DeploymentEvidenceV1 {
        schema_version: 1,
        feature_uid: current().feature_uid,
        current_implementation_revision: 2,
        specification_revision: 4,
        specification_hash: "d".repeat(64),
        merge_commit_sha: "c".repeat(40),
        repository_id: "fixture-repo".into(),
        deployment_id: Some(8),
        status_id: Some(9),
        environment: Some("production".into()),
        production_environment: true,
        state: Some("success".into()),
        observed_at_ms: 200,
        revision: 1,
    }
}

#[test]
fn deployed_hash_requires_success_for_the_exact_current_production_receipt() {
    let current = current();
    let mut evidence = deployment();
    assert_eq!(
        evidence.deployed_spec_hash(&current),
        Some("d".repeat(64).as_str())
    );

    evidence.state = Some("in_progress".into());
    assert_eq!(evidence.deployed_spec_hash(&current), None);
    evidence.state = Some("success".into());
    evidence.production_environment = false;
    assert_eq!(evidence.deployed_spec_hash(&current), None);
    evidence.production_environment = true;
    evidence.current_implementation_revision += 1;
    assert_eq!(evidence.deployed_spec_hash(&current), None);
    evidence.current_implementation_revision = current.revision;
    evidence.specification_revision += 1;
    assert_eq!(evidence.deployed_spec_hash(&current), None);
    evidence.specification_revision = current.specification_revision;
    evidence.merge_commit_sha = "e".repeat(40);
    assert_eq!(evidence.deployed_spec_hash(&current), None);
    evidence.merge_commit_sha = current.merge_commit_sha.clone();
    evidence.repository_id = "other-repository".into();
    assert_eq!(evidence.deployed_spec_hash(&current), None);
}

#[test]
fn repeated_identical_provider_observation_is_idempotent() {
    let current = current();
    let evidence = deployment();
    let observation = DeploymentObservation {
        deployment_id: Some(8),
        status_id: Some(9),
        environment: Some("production".into()),
        production_environment: true,
        state: Some("success".into()),
    };
    assert!(matches_observation(&evidence, &current, &observation));

    let mut changed = observation.clone();
    changed.status_id = Some(10);
    assert!(!matches_observation(&evidence, &current, &changed));
}
