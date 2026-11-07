use std::{cell::RefCell, collections::VecDeque};

use crate::app::bootstrap_remediation::{
    Cancellation, ProcessOutput, ProcessRunError, TrustedInvocation, TrustedProgram,
};
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

use super::{ElevatedCommandRunner, execute_sequence};

struct ScriptedRunner {
    exit_codes: RefCell<VecDeque<i32>>,
    calls: RefCell<Vec<Vec<String>>>,
}

impl ScriptedRunner {
    fn new(exit_codes: impl IntoIterator<Item = i32>) -> Self {
        Self {
            exit_codes: RefCell::new(exit_codes.into_iter().collect()),
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl ElevatedCommandRunner for ScriptedRunner {
    fn run_elevated(
        &self,
        invocation: TrustedInvocation,
        _cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        assert_eq!(invocation.program(), TrustedProgram::AptGet);
        self.calls
            .borrow_mut()
            .push(invocation.arguments_for_test());
        Ok(ProcessOutput {
            exit_code: self.exit_codes.borrow_mut().pop_front().unwrap(),
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }
}

fn qemu_invocation() -> TrustedInvocation {
    TrustedInvocation::qemu_package_setup(LinuxRootlessRecipeV1::Ubuntu2404Amd64).unwrap()
}

#[test]
fn refreshed_archive_check_gates_fixed_qemu_install() {
    // This exercises the production internal-runner boundary with a scripted elevated runner;
    // package management and privilege escalation remain external and are not invoked.
    let runner = ScriptedRunner::new([0, 0]);
    let cancellation = Cancellation::new();
    let mut verified = false;
    let output = execute_sequence(
        &runner,
        qemu_invocation(),
        LinuxRootlessRecipeV1::Ubuntu2404Amd64,
        &cancellation,
        || {
            verified = true;
            Ok(())
        },
    )
    .unwrap();

    let calls = runner.calls.borrow();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].last().map(String::as_str), Some("update"));
    assert!(calls[1].contains(&"install".into()));
    assert_eq!(
        &calls[1][calls[1].len() - 2..],
        ["qemu-system-x86", "qemu-utils"]
    );
    assert!(verified);
    assert_eq!(output.exit_code, 0);
}

#[test]
fn failed_archive_refresh_stops_before_verification_and_install() {
    let runner = ScriptedRunner::new([1]);
    let cancellation = Cancellation::new();
    let mut verified = false;
    let output = execute_sequence(
        &runner,
        qemu_invocation(),
        LinuxRootlessRecipeV1::Ubuntu2404Amd64,
        &cancellation,
        || {
            verified = true;
            Ok(())
        },
    )
    .unwrap();

    assert_eq!(runner.calls.borrow().len(), 1);
    assert!(!verified);
    assert_eq!(output.exit_code, 1);
}

#[test]
fn rejected_refreshed_candidate_stops_before_package_install() {
    let runner = ScriptedRunner::new([0]);
    let cancellation = Cancellation::new();
    let result = execute_sequence(
        &runner,
        qemu_invocation(),
        LinuxRootlessRecipeV1::Ubuntu2404Amd64,
        &cancellation,
        || anyhow::bail!("synthetic unapproved dependency"),
    );

    assert!(result.is_err());
    assert_eq!(runner.calls.borrow().len(), 1);
    assert_eq!(
        runner.calls.borrow()[0].last().map(String::as_str),
        Some("update")
    );
}
