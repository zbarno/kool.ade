use super::*;

#[test]
fn public_remote_constructor_fails_closed_without_claim_authority() {
    let worker = crate::execution::remote::ssh::SshWorkerConfig {
        stable_id: "worker-test".into(),
        host_alias: "worker.test".into(),
        ssh_principal: "test-user".into(),
        verified_host_key_fingerprint: format!("SHA256:{}", "A".repeat(43)),
        known_hosts_path: "known_hosts".into(),
        ssh_binary: "ssh".into(),
    };
    let error = ExecutionSession::connect_remote(
        WorkspaceRef {
            project_id: "00000000-0000-4000-8000-000000000001".into(),
            task_id: "00000000-0000-4000-8000-000000000002".into(),
            source_sha: "a".repeat(40),
        },
        ExecutionTarget::remote("worker-test"),
        &worker,
        crate::execution::contract::ExecutionModeV1::Implementation,
        crate::execution::docker::images::RuntimeProfile::General,
        ResourceBudgetV1::default(),
    )
    .err()
    .expect("remote execution must require global claim authority");

    assert_eq!(error.kind, ExecutionErrorKind::PolicyDenied);
    assert!(
        error
            .message
            .contains("active task claim authorization verifier")
    );
}

#[derive(Debug)]
struct RevocableAuthorization {
    revoked: Arc<AtomicBool>,
}

impl AuthorizationGenerationVerifier for RevocableAuthorization {
    fn generation(&self) -> String {
        "00000000-0000-4000-8000-000000000099".into()
    }

    fn verify(&self, generation: &str) -> Result<(), crate::execution::errors::ExecutionError> {
        if generation != "00000000-0000-4000-8000-000000000099"
            || self.revoked.load(Ordering::Acquire)
        {
            return Err(crate::execution::errors::ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "The task claim is no longer current",
                false,
            ));
        }
        Ok(())
    }
}

#[test]
fn claim_takeover_during_remote_execution_cancels_and_discards_results() {
    let backend = backend();
    let revoked = Arc::new(AtomicBool::new(false));
    backend.events.lock().unwrap().clear();
    *backend.invalidate_authorization_on_events.lock().unwrap() = Some(revoked.clone());
    let verifier = Arc::new(RevocableAuthorization {
        revoked: revoked.clone(),
    });
    let mut session = session(backend.clone());
    session.authorization_verifier = Some(verifier.clone());
    session.authorization_generation = verifier.generation();

    let error = session
        .run_bounded_script("printf should-not-be-accepted", Duration::from_millis(500))
        .unwrap_err();

    assert_eq!(error.kind, ExecutionErrorKind::PolicyDenied);
    assert!(revoked.load(Ordering::Acquire));
    assert!(backend.cancelled.load(Ordering::Acquire));
    assert!(backend.released.load(Ordering::Acquire));
    assert!(backend.workspace_released.load(Ordering::Acquire));
}
