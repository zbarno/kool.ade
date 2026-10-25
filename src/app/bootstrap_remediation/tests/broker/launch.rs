use super::super::super::*;
use super::super::support::*;
use crate::{
    app::{
        bootstrap_remediation::supervisor::{
            ElevationReceipt, NativeCommandRunner, NativeElevation, ProcessOutput, ProcessRunError,
            ProcessRunFailure, TrustedInvocation,
        },
        bootstrap_repair_policy::Host,
    },
    persistence::setup_tasks::{AttemptState, FailureCode, RemediationProcessStatusV1},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

// Production boundary: authorization is checked again after OS consent and
// before native process dispatch. A fake elevation adapter can model an old
// approval without waiting ten minutes; this needs no host service or network.
#[test]
fn expired_admin_approval_after_elevation_never_dispatches_process() {
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

    let plan = plan(Host::Linux, Some(PrivilegeLevel::Administrator));
    let identity =
        HostIdentity::from_facts(Host::Linux, "10.2.0", "synthetic-machine", "synthetic-user")
            .unwrap();
    let mut store = ready_store(&plan.prerequisite_id);
    let mut gate = ApprovalGate::default();
    let (_, approval) = approved(&plan, &identity, &mut store, &mut gate, 1);
    let elevation_calls = Arc::new(AtomicUsize::new(0));
    let runner_calls = Arc::new(AtomicUsize::new(0));

    assert!(
        RemediationSupervisor::execute_for_test(
            approval,
            &plan,
            &identity,
            &mut store,
            &FakeRegistry(Host::Linux),
            &CountingRunner(runner_calls.clone()),
            &CountingElevation(elevation_calls.clone()),
            &FreshReadiness,
            &Cancellation::new(),
            3,
        )
        .is_err()
    );
    assert_eq!(elevation_calls.load(Ordering::SeqCst), 1);
    assert_eq!(runner_calls.load(Ordering::SeqCst), 0);
    let attempt = &store.tasks[&plan.prerequisite_id].attempts[0];
    assert_eq!(attempt.state, AttemptState::Failed);
    assert_eq!(attempt.failure, Some(FailureCode::PermissionDenied));
    assert_eq!(
        attempt.remediation_result.as_ref().unwrap().process_status,
        RemediationProcessStatusV1::NotStarted
    );
}
