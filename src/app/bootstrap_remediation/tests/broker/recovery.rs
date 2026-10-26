use super::super::super::*;
use super::super::support::*;
use crate::{
    app::bootstrap_remediation::supervisor::{
        ElevationReceipt, NativeCommandRunner, NativeElevation, OperationRegistry, ProcessOutput,
        ProcessRunError, ProcessRunFailure, ReadinessVerifier, TrustedInvocation,
        VerificationReceipt,
    },
    app::bootstrap_repair_policy::Host,
    persistence::setup_tasks::{
        AttemptState, FailureCode, RemediationProcessStatusV1, RemediationVerificationStatusV1,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

// Production boundary: drives supervisor failure, probe, and restart-recovery
// outcomes through the remediation contract. Fakes prove journal invariants;
// native host behavior is covered in #346–#351 and needs those OS environments.
// These cases need no Docker, SSH, network, or privileged host service.

#[test]
fn declining_native_elevation_cancels_without_running_the_operation() {
    let plan = plan(Host::Linux, Some(PrivilegeLevel::Administrator));
    let now = now();
    let identity =
        HostIdentity::from_facts(Host::Linux, "10.2.0", "synthetic-machine", "synthetic-user")
            .unwrap();
    let mut store = ready_store(&plan.prerequisite_id);
    let mut gate = ApprovalGate::default();
    let (_attempt, approval) = approved(&plan, &identity, &mut store, &mut gate, now);
    let result = RemediationSupervisor::execute_for_test(
        approval,
        &plan,
        &identity,
        &mut store,
        &FakeRegistry(Host::Linux),
        &FakeRunner(Host::Linux),
        &FakeElevation { declined: true },
        &FreshReadiness,
        &Cancellation::new(),
        now + 2,
    );
    assert!(result.is_err());
    let attempt = &store.tasks[&plan.prerequisite_id].attempts[0];
    assert_eq!(attempt.state, AttemptState::Cancelled);
    assert!(attempt.authorization_binding.is_none());
    assert_eq!(
        attempt.remediation_result.as_ref().unwrap().process_status,
        RemediationProcessStatusV1::NotStarted
    );
}

#[test]
fn launched_timeout_under_elevation_runs_probes_and_persists_partial_state() {
    struct TimedOutRunner;
    impl NativeCommandRunner for TimedOutRunner {
        fn run(
            &self,
            _invocation: TrustedInvocation,
            _cancellation: &Cancellation,
        ) -> Result<ProcessOutput, ProcessRunError> {
            Err(ProcessRunError::after_launch(ProcessRunFailure::TimedOut))
        }

        fn run_elevated(
            &self,
            invocation: TrustedInvocation,
            _elevation: ElevationReceipt,
            cancellation: &Cancellation,
        ) -> Result<ProcessOutput, ProcessRunError> {
            self.run(invocation, cancellation)
        }
    }
    struct NotReady(Arc<AtomicUsize>);
    impl ReadinessVerifier for NotReady {
        fn verify(&self, plan: &RemediationPlan) -> anyhow::Result<VerificationReceipt> {
            self.0.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(2));
            Ok(VerificationReceipt {
                probe_ids: plan.verification_probe_ids.clone(),
                observed_at_unix_ms: now(),
                evidence_digest: "b".repeat(64),
                ready: false,
            })
        }
    }

    let plan = plan(Host::Linux, Some(PrivilegeLevel::Administrator));
    let identity =
        HostIdentity::from_facts(Host::Linux, "10.2.0", "synthetic-machine", "synthetic-user")
            .unwrap();
    let now = now();
    let mut store = ready_store(&plan.prerequisite_id);
    let mut gate = ApprovalGate::default();
    let (_attempt, approval) = approved(&plan, &identity, &mut store, &mut gate, now);
    let probe_calls = Arc::new(AtomicUsize::new(0));
    let result = RemediationSupervisor::execute_for_test(
        approval,
        &plan,
        &identity,
        &mut store,
        &FakeRegistry(Host::Linux),
        &TimedOutRunner,
        &FakeElevation { declined: false },
        &NotReady(probe_calls.clone()),
        &Cancellation::new(),
        now + 2,
    );
    assert!(result.is_err());
    assert_eq!(probe_calls.load(Ordering::SeqCst), 1);
    let attempt = &store.tasks[&plan.prerequisite_id].attempts[0];
    assert_eq!(attempt.state, AttemptState::Failed);
    assert_eq!(attempt.failure, Some(FailureCode::Unknown));
    let result = attempt.remediation_result.as_ref().unwrap();
    assert_eq!(
        result.process_status,
        RemediationProcessStatusV1::InterruptedAfterLaunch
    );
    assert_eq!(
        result.verification.as_ref().unwrap().status,
        RemediationVerificationStatusV1::NotReady
    );
    assert!(!result.rollback_guidance.is_empty());
}

#[test]
fn offline_registry_failure_is_journaled_without_launch_or_readiness_claim() {
    struct OfflineRegistry;
    impl OperationRegistry for OfflineRegistry {
        fn compile(&self, _plan: &RemediationPlan) -> anyhow::Result<TrustedInvocation> {
            anyhow::bail!("synthetic trusted repository is offline")
        }
    }

    let plan = plan(Host::Linux, None);
    let identity =
        HostIdentity::from_facts(Host::Linux, "10.2.0", "synthetic-machine", "synthetic-user")
            .unwrap();
    let now = now();
    let mut store = ready_store(&plan.prerequisite_id);
    let mut gate = ApprovalGate::default();
    let (_attempt, approval) = approved(&plan, &identity, &mut store, &mut gate, now);
    assert!(
        RemediationSupervisor::execute_for_test(
            approval,
            &plan,
            &identity,
            &mut store,
            &OfflineRegistry,
            &FakeRunner(Host::Linux),
            &FakeElevation { declined: false },
            &FreshReadiness,
            &Cancellation::new(),
            now + 2,
        )
        .is_err()
    );
    let attempt = &store.tasks[&plan.prerequisite_id].attempts[0];
    assert_eq!(attempt.state, AttemptState::Failed);
    let result = attempt.remediation_result.as_ref().unwrap();
    assert_eq!(
        result.process_status,
        RemediationProcessStatusV1::NotStarted
    );
    assert!(result.verification.is_none());
}

#[test]
fn process_exit_alone_does_not_mark_readiness_verified() {
    struct FailedVerification;
    impl ReadinessVerifier for FailedVerification {
        fn verify(&self, _plan: &RemediationPlan) -> anyhow::Result<VerificationReceipt> {
            anyhow::bail!("synthetic readiness probe failed")
        }
    }

    let plan = plan(Host::Linux, None);
    let identity =
        HostIdentity::from_facts(Host::Linux, "10.2.0", "synthetic-machine", "synthetic-user")
            .unwrap();
    let now = now();
    let mut store = ready_store(&plan.prerequisite_id);
    let mut gate = ApprovalGate::default();
    let (_attempt, approval) = approved(&plan, &identity, &mut store, &mut gate, now);
    assert!(
        RemediationSupervisor::execute_for_test(
            approval,
            &plan,
            &identity,
            &mut store,
            &FakeRegistry(Host::Linux),
            &FakeRunner(Host::Linux),
            &FakeElevation { declined: false },
            &FailedVerification,
            &Cancellation::new(),
            now + 2,
        )
        .is_err()
    );
    let attempt = &store.tasks[&plan.prerequisite_id].attempts[0];
    assert_eq!(attempt.state, AttemptState::Failed);
    assert_eq!(attempt.failure, Some(FailureCode::Unknown));
    assert_eq!(
        attempt
            .remediation_result
            .as_ref()
            .unwrap()
            .verification
            .as_ref()
            .unwrap()
            .status,
        RemediationVerificationStatusV1::Unavailable
    );
}

#[test]
// Production boundary: recovery must revoke an approved administrator
// capability and refuse to reissue a process after restart. The fake OS grant
// cannot prove a real crash under elevation; that host check belongs to #346–
// #351. This journal test needs no Docker, SSH, network, or privileged service.
fn interrupted_privileged_recovery_records_unknown_state_and_refuses_replay() {
    let plan = plan(Host::Linux, Some(PrivilegeLevel::Administrator));
    let identity =
        HostIdentity::from_facts(Host::Linux, "10.2.0", "synthetic-machine", "synthetic-user")
            .unwrap();
    let now = now();
    let mut store = ready_store(&plan.prerequisite_id);
    let mut gate = ApprovalGate::default();
    let (_attempt, approval) = approved(&plan, &identity, &mut store, &mut gate, now);
    let invocation = FakeRegistry(Host::Linux).compile(&plan).unwrap();
    let elevation = FakeElevation { declined: false }
        .request(&plan, &identity, &invocation, &Cancellation::new())
        .unwrap()
        .unwrap();
    drop(elevation);
    assert!(store.recover_interrupted(now + 3, "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", |_| false,));
    store.validate().unwrap();
    let persisted = &store.tasks[&plan.prerequisite_id].attempts[0];
    assert_eq!(persisted.state, AttemptState::Interrupted);
    assert_eq!(persisted.failure, Some(FailureCode::InterruptedByRestart));
    assert_eq!(
        persisted
            .remediation_result
            .as_ref()
            .unwrap()
            .process_status,
        RemediationProcessStatusV1::InterruptedByRestart
    );

    let launches = Arc::new(AtomicUsize::new(0));
    struct CountingRunner(Arc<AtomicUsize>);
    impl NativeCommandRunner for CountingRunner {
        fn run(
            &self,
            _invocation: TrustedInvocation,
            _cancellation: &Cancellation,
        ) -> Result<ProcessOutput, ProcessRunError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(ProcessRunError::not_started(ProcessRunFailure::Unavailable))
        }
    }
    assert!(
        RemediationSupervisor::execute_for_test(
            approval,
            &plan,
            &identity,
            &mut store,
            &FakeRegistry(Host::Linux),
            &CountingRunner(launches.clone()),
            &FakeElevation { declined: false },
            &FreshReadiness,
            &Cancellation::new(),
            now + 4,
        )
        .is_err()
    );
    assert_eq!(launches.load(Ordering::SeqCst), 0);
}
