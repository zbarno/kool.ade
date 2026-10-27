use std::time::Duration;

use crate::app::bootstrap_remediation::{ProcessRunFailure, TrustedProgram};

use super::super::dnf::execute_sequence;
use super::{ScriptedRunner, dnf_artifacts, dnf_candidates};

#[path = "dnf/staged.rs"]
mod staged;

// Production boundary: this fake-runner test covers DNF metadata refresh,
// signed-origin revalidation, and the fixed Fedora install list as one state
// machine. It uses bundled artifacts and a scripted runner only; it does not
// contact Fedora/Docker, run pkexec, or prove a live Fedora VM install.
#[test]
fn dnf_candidate_query_must_match_signed_repo_origins_before_install() {
    let invalid_candidates =
        dnf_candidates().replace("docker-ce|koolade-docker-ce-stable", "docker-ce|fedora");
    let runner = ScriptedRunner::with_stdout([0, 0, 0, 0], 3, &invalid_candidates);
    let error = execute_sequence(
        &runner,
        Duration::from_secs(60),
        crate::app::setup_attention::catalog::LinuxRootlessRecipeV1::Fedora45Amd64,
        dnf_artifacts(),
        || Ok(()),
        &crate::app::bootstrap_remediation::Cancellation::new(),
    )
    .unwrap_err();

    assert!(error.launched);
    assert_eq!(error.failure, ProcessRunFailure::FailedAfterLaunch);
    let calls = runner.calls.borrow();
    assert_eq!(calls.len(), 4);
    assert_eq!(calls[0].0, TrustedProgram::Dd);
    assert_eq!(calls[1].0, TrustedProgram::Dd);
    assert_eq!(calls[2].0, TrustedProgram::Dnf);
    assert_eq!(calls[3].0, TrustedProgram::Dnf);
    assert_eq!(
        calls[2].1[..super::dnf_common_arguments().len()],
        super::dnf_common_arguments()
    );
    assert_eq!(
        calls[3].1[..super::dnf_common_arguments().len()],
        super::dnf_common_arguments()
    );
    assert!(calls[3].1.contains(&"--disablerepo=*".to_owned()));
    assert!(!calls.iter().any(|(program, arguments)| {
        *program == TrustedProgram::Dnf && arguments.contains(&"install".to_owned())
    }));
}

#[test]
fn dnf_cache_refresh_failure_stops_before_candidate_query_and_install() {
    let repository_checks = std::cell::Cell::new(0);
    let runner = ScriptedRunner::new([0, 0, 100]);
    let output = execute_sequence(
        &runner,
        Duration::from_secs(60),
        crate::app::setup_attention::catalog::LinuxRootlessRecipeV1::Fedora45Amd64,
        dnf_artifacts(),
        || {
            repository_checks.set(repository_checks.get() + 1);
            Ok(())
        },
        &crate::app::bootstrap_remediation::Cancellation::new(),
    )
    .unwrap();

    assert_eq!(output.exit_code, 100);
    let calls = runner.calls.borrow();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[2].0, TrustedProgram::Dnf);
    assert!(calls[2].1.contains(&"makecache".to_owned()));
    assert!(calls[2].1.contains(&"--refresh".to_owned()));
    assert_eq!(
        calls[2].1[..super::dnf_common_arguments().len()],
        super::dnf_common_arguments()
    );
    assert_eq!(repository_checks.get(), 1);
}

#[test]
fn conflicting_repository_state_stops_before_dnf_refresh() {
    let runner = ScriptedRunner::new([0, 0]);
    let error = execute_sequence(
        &runner,
        Duration::from_secs(60),
        crate::app::setup_attention::catalog::LinuxRootlessRecipeV1::Fedora45Amd64,
        dnf_artifacts(),
        || anyhow::bail!("synthetic conflicting Fedora repository file"),
        &crate::app::bootstrap_remediation::Cancellation::new(),
    )
    .unwrap_err();

    assert!(error.launched);
    assert_eq!(error.failure, ProcessRunFailure::FailedAfterLaunch);
    let calls = runner.calls.borrow();
    assert_eq!(calls.len(), 2);
    assert!(
        calls
            .iter()
            .all(|(program, _)| *program == TrustedProgram::Dd)
    );
}
