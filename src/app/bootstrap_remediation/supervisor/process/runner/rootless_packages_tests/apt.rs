use std::time::Duration;

use crate::app::bootstrap_remediation::{ProcessRunFailure, TrustedProgram};

use super::super::execute_sequence;
use super::{APT_SOURCE_OPTIONS, ScriptedRunner, artifacts};

// Production boundary: these scripted tests cover APT refresh, candidate
// revalidation, and the fixed install list. They do not contact repositories,
// invoke pkexec, or validate a live Ubuntu VM. Production validation needs a
// disposable Ubuntu VM, network access to Docker's repository, and elevation.

#[test]
fn apt_update_failure_stops_before_candidate_check_and_package_install() {
    let runner = ScriptedRunner::new([0, 0, 0, 100]);
    let output = execute_sequence(
        &runner,
        Duration::from_secs(60),
        crate::app::setup_attention::catalog::LinuxRootlessRecipeV1::Ubuntu2404Amd64,
        artifacts(),
        || panic!("candidate validation must not run after apt update fails"),
        &crate::app::bootstrap_remediation::Cancellation::new(),
    )
    .unwrap();

    assert_eq!(output.exit_code, 100);
    let calls = runner.calls.borrow();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0].0, TrustedProgram::Mkdir);
    assert_eq!(calls[1].0, TrustedProgram::Dd);
    assert_eq!(calls[2].0, TrustedProgram::Dd);
    let expected_update = APT_SOURCE_OPTIONS
        .into_iter()
        .chain(["update"])
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(calls[3], (TrustedProgram::AptGet, expected_update));
}

#[test]
fn candidate_revalidation_failure_stops_before_package_install() {
    let runner = ScriptedRunner::new([0, 0, 0, 0]);
    let error = execute_sequence(
        &runner,
        Duration::from_secs(60),
        crate::app::setup_attention::catalog::LinuxRootlessRecipeV1::Ubuntu2404Amd64,
        artifacts(),
        || anyhow::bail!("synthetic candidate origin mismatch"),
        &crate::app::bootstrap_remediation::Cancellation::new(),
    )
    .unwrap_err();

    assert!(error.launched);
    assert_eq!(error.failure, ProcessRunFailure::FailedAfterLaunch);
    assert_eq!(runner.calls.borrow().len(), 4);
}

#[test]
fn package_install_uses_only_the_fixed_approved_package_list() {
    let runner = ScriptedRunner::new([0, 0, 0, 0, 0]);
    execute_sequence(
        &runner,
        Duration::from_secs(60),
        crate::app::setup_attention::catalog::LinuxRootlessRecipeV1::Ubuntu2404Amd64,
        artifacts(),
        || Ok(()),
        &crate::app::bootstrap_remediation::Cancellation::new(),
    )
    .unwrap();

    let calls = runner.calls.borrow();
    assert_eq!(calls.len(), 5);
    assert_eq!(calls[4].0, TrustedProgram::AptGet);
    let expected_install = APT_SOURCE_OPTIONS
        .into_iter()
        .chain([
            "install",
            "--assume-yes",
            "--no-upgrade",
            "--no-remove",
            "--no-install-recommends",
            "docker-ce",
            "docker-ce-cli",
            "containerd.io",
            "docker-ce-rootless-extras",
            "uidmap",
            "dbus-user-session",
        ])
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(calls[4].1, expected_install);
    assert!(!calls[1].1[0].is_empty());
    assert_eq!(calls[1].1[1], "of=/etc/apt/keyrings/docker.asc");
    assert_eq!(calls[1].1[2], "oflag=excl");
    assert_eq!(
        calls[2].1[1],
        "of=/etc/apt/sources.list.d/koolade-docker.sources"
    );
}
