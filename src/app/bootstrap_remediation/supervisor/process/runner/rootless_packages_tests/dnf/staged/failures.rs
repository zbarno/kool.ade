use crate::app::bootstrap_remediation::{ProcessRunFailure, TrustedProgram};

use super::harness::{ResponsePlan, execute, runner};

#[test]
fn download_failure_cleans_stage_without_running_signature_or_install_steps() {
    let runner = runner(ResponsePlan {
        fail_at: Some(5),
        ..ResponsePlan::default()
    });
    let output = execute(&runner, || Ok(())).unwrap();

    assert_eq!(output.exit_code, 100);
    let calls = runner.calls.borrow();
    assert_eq!(calls.last().unwrap().0, TrustedProgram::Rm);
    assert!(!calls.iter().any(|call| call.0 == TrustedProgram::RpmKeys));
    assert!(!calls.iter().any(|call| call.0 == TrustedProgram::Rpm));
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.0 == TrustedProgram::Dnf)
            .count(),
        3
    );
}

#[test]
fn incomplete_or_unexpected_key_inventory_stops_before_signature_check_and_install() {
    for plan in [
        ResponsePlan {
            extra_key: true,
            ..ResponsePlan::default()
        },
        ResponsePlan {
            truncate_at: Some(10),
            ..ResponsePlan::default()
        },
    ] {
        let runner = runner(plan);
        let error = execute(&runner, || Ok(())).unwrap_err();

        assert_eq!(error.failure, ProcessRunFailure::FailedAfterLaunch);
        let calls = runner.calls.borrow();
        assert_eq!(calls.last().unwrap().0, TrustedProgram::Rm);
        assert!(!calls.iter().any(|call| {
            call.0 == TrustedProgram::Dnf
                && call.1.contains(&"--setopt=installonly_limit=0".to_owned())
        }));
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.0 == TrustedProgram::RpmKeys)
                .count(),
            2
        );
    }
}

#[test]
fn signature_failure_or_truncated_success_never_mints_installable_rpms() {
    for plan in [
        ResponsePlan {
            fail_at: Some(11),
            ..ResponsePlan::default()
        },
        ResponsePlan {
            truncate_at: Some(11),
            ..ResponsePlan::default()
        },
    ] {
        let runner = runner(plan);
        let result = execute(&runner, || Ok(()));
        assert!(result.is_err() || result.as_ref().unwrap().exit_code != 0);
        let calls = runner.calls.borrow();
        assert_eq!(calls.last().unwrap().0, TrustedProgram::Rm);
        assert!(!calls.iter().any(|call| {
            call.0 == TrustedProgram::Dnf
                && call.1.contains(&"--setopt=installonly_limit=0".to_owned())
        }));
        assert!(!calls.iter().any(|call| {
            call.0 == TrustedProgram::Rpm && call.1.contains(&"--package".to_owned())
        }));
    }
}

#[test]
fn malformed_or_changed_stage_manifest_or_digest_fails_before_offline_install() {
    for plan in [
        ResponsePlan {
            invalid_manifest: true,
            ..ResponsePlan::default()
        },
        ResponsePlan {
            changed_second_manifest: true,
            ..ResponsePlan::default()
        },
        ResponsePlan {
            changed_second_hash: true,
            ..ResponsePlan::default()
        },
    ] {
        let runner = runner(plan);
        let error = execute(&runner, || Ok(())).unwrap_err();
        assert_eq!(error.failure, ProcessRunFailure::FailedAfterLaunch);
        let calls = runner.calls.borrow();
        assert_eq!(calls.last().unwrap().0, TrustedProgram::Rm);
        assert!(!calls.iter().any(|call| {
            call.0 == TrustedProgram::Dnf
                && call.1.contains(&"--setopt=installonly_limit=0".to_owned())
        }));
    }
}

#[test]
fn installed_name_overlap_fails_closed_before_offline_transaction() {
    let runner = runner(ResponsePlan {
        installed_names: "docker-ce\n",
        ..ResponsePlan::default()
    });
    let error = execute(&runner, || Ok(())).unwrap_err();

    assert_eq!(error.failure, ProcessRunFailure::FailedAfterLaunch);
    let calls = runner.calls.borrow();
    assert_eq!(calls.last().unwrap().0, TrustedProgram::Rm);
    assert!(!calls.iter().any(|call| {
        call.0 == TrustedProgram::Dnf && call.1.contains(&"--setopt=installonly_limit=0".to_owned())
    }));
}
