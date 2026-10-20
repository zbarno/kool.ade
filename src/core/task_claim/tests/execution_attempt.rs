use super::*;

#[test]
fn starting_a_new_remote_attempt_fences_the_previous_worker_session() {
    // Production boundary: this uses the real claim lease and Git compare-and-swap
    // against a bare planning remote. A unit-only mock cannot prove the remote ref
    // replacement invalidates a session that shares the same local handle.
    // External dependency: local Git; setup cost: two temporary clones and bare remotes.
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    let code_repo = &fixture.code_clones[0];
    let base = git_at(code_repo, &["rev-parse", "HEAD"]);
    let lease = ClaimLease::acquire(
        repo,
        fixture.project_uid,
        "task-attempt-fence",
        "web",
        &base,
    )
    .unwrap()
    .unwrap();
    let handle = ClaimLeaseHandle::new(lease);
    let old_attempt = uuid::Uuid::new_v4().hyphenated().to_string();
    let old_generation =
        crate::execution::session::AuthorizationGenerationVerifier::begin_remote_attempt(
            &handle,
            &old_attempt,
            "worker-a",
        )
        .unwrap();
    crate::execution::session::AuthorizationGenerationVerifier::verify(&handle, &old_generation)
        .unwrap();
    crate::execution::session::AuthorizationGenerationVerifier::mark_remote_attempt_started(
        &handle,
    )
    .unwrap();

    let new_attempt = uuid::Uuid::new_v4().hyphenated().to_string();
    assert!(
        crate::execution::session::AuthorizationGenerationVerifier::begin_remote_attempt(
            &handle,
            &new_attempt,
            "worker-b",
        )
        .is_err(),
        "a running attempt must not be superseded before its loss is confirmed"
    );
    crate::execution::session::AuthorizationGenerationVerifier::confirm_remote_attempt_lost(
        &handle,
    )
    .unwrap();
    let new_generation =
        crate::execution::session::AuthorizationGenerationVerifier::begin_remote_attempt(
            &handle,
            &new_attempt,
            "worker-b",
        )
        .unwrap();

    assert!(
        crate::execution::session::AuthorizationGenerationVerifier::verify(
            &handle,
            &old_generation
        )
        .is_err()
    );
    crate::execution::session::AuthorizationGenerationVerifier::verify(&handle, &new_generation)
        .unwrap();
    let attempt = handle.execution_attempt_for_test().unwrap();
    assert_eq!(attempt.attempt_id, new_attempt);
    let claim_ref = reference(fixture.project_uid, "task-attempt-fence").unwrap();
    let persisted_claim = git_at(&fixture.planning_remote, &["show", &claim_ref]);
    assert!(persisted_claim.contains(&new_attempt));
    assert!(!persisted_claim.contains("worker-a"));
    assert!(!persisted_claim.contains("worker-b"));
}
