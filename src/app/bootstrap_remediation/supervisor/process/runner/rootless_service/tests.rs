use std::{cell::RefCell, collections::VecDeque, os::unix::fs::PermissionsExt, time::Duration};

use crate::app::bootstrap_remediation::supervisor::{
    Cancellation, NativeCommandRunner, ProcessOutput, ProcessRunError, ProcessRunFailure,
    TrustedInvocation, TrustedProgram,
};

use super::execute_sequence;

struct ScriptedRunner {
    outputs: RefCell<VecDeque<ProcessOutput>>,
    calls: RefCell<Vec<(TrustedProgram, Vec<String>)>>,
}

impl ScriptedRunner {
    fn new(outputs: impl IntoIterator<Item = &'static str>) -> Self {
        Self::from_outputs(outputs.into_iter().map(output).collect())
    }

    fn from_outputs(outputs: Vec<ProcessOutput>) -> Self {
        Self {
            outputs: RefCell::new(outputs.into()),
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl NativeCommandRunner for ScriptedRunner {
    fn run(
        &self,
        invocation: TrustedInvocation,
        _cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        self.calls.borrow_mut().push((
            invocation.program_for_test(),
            invocation.arguments_for_test(),
        ));
        self.outputs
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))
    }
}

fn output(stdout: &str) -> ProcessOutput {
    ProcessOutput {
        exit_code: 0,
        stdout: stdout.into(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
    }
}

fn expected_systemctl(tail: &[&str]) -> (TrustedProgram, Vec<String>) {
    let mut arguments = vec!["--user".into()];
    arguments.extend(tail.iter().map(|value| (*value).to_owned()));
    (TrustedProgram::Systemctl, arguments)
}

fn private_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    home
}

#[test]
fn active_app_unit_is_enabled_without_restart() {
    let runner = ScriptedRunner::new(["", "not-found", "", "loaded", "active", "", "active"]);
    let home = private_home();
    execute_sequence(
        &runner,
        Duration::from_secs(30),
        home.path(),
        &Cancellation::new(),
    )
    .unwrap();

    let calls = runner.calls.borrow();
    assert_eq!(calls.len(), 7);
    assert_eq!(calls[0].0, TrustedProgram::Rootlesskit);
    assert_eq!(calls[0].1, vec!["/usr/bin/true".to_owned()]);
    assert_eq!(
        calls[1],
        expected_systemctl(&[
            "show",
            "--property=LoadState",
            "--value",
            "koolade-rootless-docker.service",
        ])
    );
    assert_eq!(calls[2], expected_systemctl(&["daemon-reload"]));
    assert_eq!(
        calls[5],
        expected_systemctl(&["enable", "koolade-rootless-docker.service",])
    );
    assert!(
        home.path()
            .join(".config/systemd/user/koolade-rootless-docker.service")
            .is_file()
    );
}

#[test]
fn inactive_app_unit_is_enabled_and_started_once() {
    let runner = ScriptedRunner::new(["", "not-found", "", "loaded", "inactive", "", "active"]);
    let home = private_home();
    execute_sequence(
        &runner,
        Duration::from_secs(30),
        home.path(),
        &Cancellation::new(),
    )
    .unwrap();

    assert_eq!(
        runner.calls.borrow()[5],
        expected_systemctl(&["enable", "--now", "koolade-rootless-docker.service",])
    );
}

#[test]
fn failed_start_preserves_exact_app_unit_for_a_safe_retry() {
    let runner = ScriptedRunner::from_outputs(vec![
        output(""),
        output("not-found"),
        output(""),
        output("loaded"),
        output("failed"),
        ProcessOutput {
            exit_code: 1,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        },
    ]);
    let home = private_home();
    let error = execute_sequence(
        &runner,
        Duration::from_secs(30),
        home.path(),
        &Cancellation::new(),
    )
    .unwrap_err();

    assert!(error.launched);
    assert_eq!(runner.calls.borrow().len(), 6);
    let unit = home
        .path()
        .join(".config/systemd/user/koolade-rootless-docker.service");
    assert!(unit.is_file());

    let retry = ScriptedRunner::new(["", "loaded", "failed", "", "active"]);
    execute_sequence(
        &retry,
        Duration::from_secs(30),
        home.path(),
        &Cancellation::new(),
    )
    .unwrap();
    let calls = retry.calls.borrow();
    assert_eq!(calls.len(), 5);
    assert_eq!(
        calls[3],
        expected_systemctl(&["enable", "--now", "koolade-rootless-docker.service",])
    );
    assert!(unit.is_file());
}
