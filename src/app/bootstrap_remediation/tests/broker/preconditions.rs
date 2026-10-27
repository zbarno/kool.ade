use super::super::super::*;
use super::super::support::*;
use crate::{
    app::bootstrap_remediation::supervisor::{
        ElevationReceipt, NativeCommandRunner, NativeElevation, OperationRegistry,
        PreconditionReceipt, PreconditionVerifier, ProcessOutput, ProcessRunError,
        ProcessRunFailure, TrustedInvocation, TrustedProgram,
    },
    app::bootstrap_repair_policy::Host,
    persistence::setup_tasks::{AttemptState, FailureCode, RemediationProcessStatusV1},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Copy)]
enum EvidenceFault {
    VerifierError,
    MissingProbe,
    Stale,
    NotReady,
    ChangedProbe,
    Future,
    InvalidDigest,
}

struct FaultyPreconditions {
    fault: EvidenceFault,
    requested: Arc<Mutex<Option<Vec<String>>>>,
}

impl PreconditionVerifier for FaultyPreconditions {
    fn verify(&self, probe_ids: &[String]) -> anyhow::Result<PreconditionReceipt> {
        *self.requested.lock().unwrap() = Some(probe_ids.to_vec());
        if matches!(self.fault, EvidenceFault::VerifierError) {
            anyhow::bail!("synthetic probe evidence unavailable");
        }
        let current = now();
        let mut receipt = PreconditionReceipt {
            probe_ids: probe_ids.to_vec(),
            observed_at_unix_ms: current,
            evidence_digest: "c".repeat(64),
            ready: true,
        };
        match self.fault {
            EvidenceFault::VerifierError => unreachable!(),
            EvidenceFault::MissingProbe => receipt.probe_ids.clear(),
            EvidenceFault::Stale => {
                receipt.observed_at_unix_ms = current.saturating_sub(5 * 60 * 1000 + 1);
            }
            EvidenceFault::NotReady => receipt.ready = false,
            EvidenceFault::ChangedProbe => {
                receipt.probe_ids = vec!["host:linux:unrelated_probe".into()];
            }
            EvidenceFault::Future => receipt.observed_at_unix_ms = current + 60_000,
            EvidenceFault::InvalidDigest => receipt.evidence_digest = "not-a-digest".into(),
        }
        Ok(receipt)
    }
}

struct CountingRegistry(Arc<AtomicUsize>);

impl OperationRegistry for CountingRegistry {
    fn compile(&self, _plan: &RemediationPlan) -> anyhow::Result<TrustedInvocation> {
        self.0.fetch_add(1, Ordering::SeqCst);
        TrustedInvocation::new(
            Host::Linux,
            TrustedProgram::TestHelper,
            Vec::new(),
            std::time::Duration::from_secs(5),
        )
    }
}

struct CountingElevation(Arc<AtomicUsize>);

impl NativeElevation for CountingElevation {
    fn request(
        &self,
        plan: &RemediationPlan,
        identity: &HostIdentity,
        invocation: &TrustedInvocation,
        _cancellation: &Cancellation,
    ) -> anyhow::Result<Option<ElevationReceipt>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Some(ElevationReceipt::for_test(
            plan, identity, invocation,
        )?))
    }
}

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

    fn run_elevated(
        &self,
        _invocation: TrustedInvocation,
        _elevation: ElevationReceipt,
        _cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(ProcessRunError::not_started(ProcessRunFailure::Unavailable))
    }
}

// Production boundary: proves the broker refuses to compile, request native
// elevation, or run a privileged Linux setup operation unless every fresh
// precondition passes. This journal/controller test uses fakes and needs no
// Docker, SSH, network, or privileged host service.
#[test]
fn failed_preconditions_are_journaled_before_registry_elevation_or_runner() {
    let faults = [
        EvidenceFault::VerifierError,
        EvidenceFault::MissingProbe,
        EvidenceFault::Stale,
        EvidenceFault::NotReady,
        EvidenceFault::ChangedProbe,
        EvidenceFault::Future,
        EvidenceFault::InvalidDigest,
    ];
    for fault in faults {
        let (plan, identity) = rootless_packages_plan();
        assert_eq!(plan.privilege, PrivilegeLevel::Administrator);
        let started_at = now();
        let mut store = ready_store(&plan.prerequisite_id);
        let mut gate = ApprovalGate::default();
        let (_attempt, approval) = approved(&plan, &identity, &mut store, &mut gate, started_at);
        let requested = Arc::new(Mutex::new(None));
        let registry_calls = Arc::new(AtomicUsize::new(0));
        let elevation_calls = Arc::new(AtomicUsize::new(0));
        let runner_calls = Arc::new(AtomicUsize::new(0));

        let result = RemediationSupervisor::execute_for_test(
            approval,
            &plan,
            &identity,
            &mut store,
            &CountingRegistry(registry_calls.clone()),
            &FaultyPreconditions {
                fault,
                requested: requested.clone(),
            },
            &CountingRunner(runner_calls.clone()),
            &CountingElevation(elevation_calls.clone()),
            &FreshReadiness,
            &Cancellation::new(),
            started_at + 3,
        );

        assert!(result.is_err());
        assert_eq!(
            requested.lock().unwrap().as_deref(),
            Some(plan.precondition_probe_ids.as_slice())
        );
        assert_eq!(registry_calls.load(Ordering::SeqCst), 0);
        assert_eq!(elevation_calls.load(Ordering::SeqCst), 0);
        assert_eq!(runner_calls.load(Ordering::SeqCst), 0);
        let attempt = &store.tasks[&plan.prerequisite_id].attempts[0];
        assert_eq!(attempt.state, AttemptState::Failed);
        assert_eq!(attempt.failure, Some(FailureCode::VerificationFailed));
        assert!(attempt.authorization_binding.is_none());
        let result = attempt.remediation_result.as_ref().unwrap();
        assert_eq!(
            result.process_status,
            RemediationProcessStatusV1::NotStarted
        );
        assert!(result.verification.is_none());
    }
}
