use super::super::super::*;
use super::super::support::*;
use crate::{
    app::{
        bootstrap_remediation::supervisor::{
            ElevationReceipt, NativeCommandRunner, ProcessOutput, ProcessRunError,
            TrustedInvocation,
        },
        bootstrap_repair_policy::Host,
    },
    persistence::setup_tasks::{
        AttemptState, RemediationProcessStatusV1, RemediationVerificationStatusV1,
    },
};

// Production boundary: exercises the approved Windows broker through persisted
// authorization consumption and the reboot-required result transition. The
// runner is synthetic, so this cannot prove DISM or Windows restart behavior;
// it uses no Docker, SSH, network, or host mutation and has negligible setup cost.
#[test]
fn windows_reboot_result_consumes_approval_and_fences_another_attempt() {
    struct RebootRunner;
    impl NativeCommandRunner for RebootRunner {
        fn run(
            &self,
            _invocation: TrustedInvocation,
            _cancellation: &Cancellation,
        ) -> Result<ProcessOutput, ProcessRunError> {
            unreachable!("Windows setup requires the elevated runner")
        }

        fn run_elevated(
            &self,
            invocation: TrustedInvocation,
            _elevation: ElevationReceipt,
            _cancellation: &Cancellation,
        ) -> Result<ProcessOutput, ProcessRunError> {
            assert_eq!(invocation.host(), Host::Windows);
            Ok(ProcessOutput {
                exit_code: 3010,
                stdout: String::new(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
            })
        }
    }

    let now = now();
    let (plan, identity) = windows_wsl_features_plan();
    let mut store = ready_store(&plan.prerequisite_id);
    let mut gate = ApprovalGate::default();
    let (_, approval) = approved(&plan, &identity, &mut store, &mut gate, now);

    let result = RemediationSupervisor::execute_for_test(
        approval,
        &plan,
        &identity,
        &mut store,
        &FakeRegistry(Host::Windows),
        &FreshPreconditions,
        &RebootRunner,
        &FakeElevation { declined: false },
        &FreshReadiness,
        &Cancellation::new(),
        now + 3,
    );
    assert!(result.unwrap_err().to_string().contains("must restart"));

    let task = &store.tasks[&plan.prerequisite_id];
    let attempt = &task.attempts[0];
    assert_eq!(attempt.state, AttemptState::InProgress);
    assert!(attempt.authorization_binding.is_none());
    assert!(attempt.authorization_history.is_some());
    let reboot = attempt.remediation_result.as_ref().unwrap();
    assert_eq!(
        reboot.process_status,
        RemediationProcessStatusV1::RebootRequired
    );
    assert_eq!(reboot.exit_code, Some(3010));
    assert_eq!(
        reboot.verification.as_ref().unwrap().status,
        RemediationVerificationStatusV1::NeedsFreshProbe
    );
    assert!(
        store
            .request_attempt(
                &plan.prerequisite_id,
                uuid::Uuid::new_v4().to_string(),
                1,
                vec![plan.prerequisite_id.clone()],
                "c4d870dd-20a1-44ef-9d3f-73702d5f2222",
                now + 4,
            )
            .is_err()
    );
}
