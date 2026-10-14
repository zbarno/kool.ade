use super::*;

#[test]
fn independent_tasks_can_verify_claims_in_the_same_planning_checkout() {
    let fixture = Fixture::new();
    let first = ClaimLeaseHandle::new(
        claim(&fixture, &fixture.clones[0], "first")
            .unwrap()
            .unwrap(),
    );
    let second = ClaimLeaseHandle::new(
        claim(&fixture, &fixture.clones[0], "second")
            .unwrap()
            .unwrap(),
    );
    let barrier = Barrier::new(2);
    let failures = std::thread::scope(|scope| {
        let first_run = scope.spawn(|| verify_claims(&first, &barrier));
        let second_run = scope.spawn(|| verify_claims(&second, &barrier));
        [first_run.join().unwrap(), second_run.join().unwrap()].concat()
    });
    assert!(
        failures.is_empty(),
        "independent claim verification failed: {failures:?}"
    );
}

fn verify_claims(handle: &ClaimLeaseHandle, barrier: &Barrier) -> Vec<String> {
    let mut failures = Vec::new();
    for _ in 0..8 {
        barrier.wait();
        if let Err(error) = handle.verify() {
            failures.push(error.to_string());
        }
        barrier.wait();
    }
    failures
}

#[test]
fn lost_claim_keeps_the_first_failure_for_execution_diagnostics() {
    let fixture = Fixture::new();
    let handle = ClaimLeaseHandle::new(
        claim(&fixture, &fixture.clones[0], "deleted")
            .unwrap()
            .unwrap(),
    );
    let reference = reference(fixture.project_uid, "deleted").unwrap();
    git_at(&fixture.planning_remote, &["update-ref", "-d", &reference]);
    let first_error = handle.verify().unwrap_err().to_string();
    let repeated_error = handle.verify().unwrap_err().to_string();
    assert!(first_error.contains("disappeared"));
    assert_eq!(repeated_error, first_error);
}
