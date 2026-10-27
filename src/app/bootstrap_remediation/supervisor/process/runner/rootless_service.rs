use std::time::{Duration, Instant};

use crate::app::{
    bootstrap_remediation::supervisor::{
        Cancellation, NativeCommandRunner, ProcessOutput, ProcessRunError, ProcessRunFailure,
        SystemCommandRunner, TrustedArgument, TrustedInvocation, TrustedProgram,
    },
    bootstrap_repair_policy::Host,
};

use super::rootless_service_unit::{
    ensure_current_user_rootless_unit, validate_existing_current_user_rootless_unit,
};

const UNIT_NAME: &str = "koolade-rootless-docker.service";

pub(super) fn execute(
    runner: &SystemCommandRunner,
    invocation: TrustedInvocation,
    cancellation: &Cancellation,
) -> Result<ProcessOutput, ProcessRunError> {
    if invocation.host != Host::Linux
        || invocation.environment != super::super::program::TrustedEnvironment::CurrentUserSystemd
        || !invocation.arguments.is_empty()
    {
        return Err(ProcessRunError::not_started(
            ProcessRunFailure::FailedBeforeLaunch,
        ));
    }
    let environment = super::user_environment::current_user_systemd()
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    execute_sequence(runner, invocation.timeout, &environment.home, cancellation)
}

fn execute_sequence(
    runner: &dyn NativeCommandRunner,
    timeout: Duration,
    home: &std::path::Path,
    cancellation: &Cancellation,
) -> Result<ProcessOutput, ProcessRunError> {
    let deadline = Instant::now() + timeout;
    let mut launched = false;

    checked_call(
        runner,
        TrustedProgram::Rootlesskit,
        vec![TrustedArgument::RootlesskitTrueExecutable],
        deadline,
        cancellation,
        &mut launched,
    )?;

    let load_state = checked_call(
        runner,
        TrustedProgram::Systemctl,
        systemctl_args(vec![
            TrustedArgument::SystemctlShow,
            TrustedArgument::SystemctlPropertyLoadState,
            TrustedArgument::SystemctlValue,
            TrustedArgument::LinuxRootlessServiceUnit,
        ]),
        deadline,
        cancellation,
        &mut launched,
    )?;
    match load_state.stdout.trim() {
        "not-found" => {
            ensure_current_user_rootless_unit(home).map_err(|_| failure(launched))?;
            if cancellation.is_cancelled() {
                return Err(ProcessRunError::after_launch(ProcessRunFailure::Cancelled));
            }
            checked_call(
                runner,
                TrustedProgram::Systemctl,
                systemctl_args(vec![TrustedArgument::SystemctlDaemonReload]),
                deadline,
                cancellation,
                &mut launched,
            )?;
            let loaded = checked_call(
                runner,
                TrustedProgram::Systemctl,
                systemctl_args(vec![
                    TrustedArgument::SystemctlShow,
                    TrustedArgument::SystemctlPropertyLoadState,
                    TrustedArgument::SystemctlValue,
                    TrustedArgument::LinuxRootlessServiceUnit,
                ]),
                deadline,
                cancellation,
                &mut launched,
            )?;
            if loaded.stdout.trim() != "loaded" {
                return Err(failure(launched));
            }
        }
        "loaded" => {
            if !validate_existing_current_user_rootless_unit(home).map_err(|_| failure(launched))? {
                return Err(failure(launched));
            }
        }
        _ => return Err(failure(launched)),
    }

    let active = checked_call(
        runner,
        TrustedProgram::Systemctl,
        systemctl_args(vec![
            TrustedArgument::SystemctlShow,
            TrustedArgument::SystemctlPropertyActiveState,
            TrustedArgument::SystemctlValue,
            TrustedArgument::LinuxRootlessServiceUnit,
        ]),
        deadline,
        cancellation,
        &mut launched,
    )?;
    let already_active = match active.stdout.trim() {
        "active" => true,
        "inactive" | "failed" => false,
        _ => return Err(failure(launched)),
    };

    checked_call(
        runner,
        TrustedProgram::Systemctl,
        enable_arguments(already_active),
        deadline,
        cancellation,
        &mut launched,
    )?;

    let ready = checked_call(
        runner,
        TrustedProgram::Systemctl,
        systemctl_args(vec![
            TrustedArgument::SystemctlShow,
            TrustedArgument::SystemctlPropertyActiveState,
            TrustedArgument::SystemctlValue,
            TrustedArgument::LinuxRootlessServiceUnit,
        ]),
        deadline,
        cancellation,
        &mut launched,
    )?;
    if ready.stdout.trim() != "active" {
        return Err(failure(launched));
    }

    Ok(ProcessOutput {
        exit_code: 0,
        stdout: format!("{UNIT_NAME} enabled and active\n"),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
    })
}

fn checked_call(
    runner: &dyn NativeCommandRunner,
    program: TrustedProgram,
    arguments: Vec<TrustedArgument>,
    deadline: Instant,
    cancellation: &Cancellation,
    launched: &mut bool,
) -> Result<ProcessOutput, ProcessRunError> {
    if cancellation.is_cancelled() {
        return Err(if *launched {
            ProcessRunError::after_launch(ProcessRunFailure::Cancelled)
        } else {
            ProcessRunError::not_started(ProcessRunFailure::Cancelled)
        });
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(if *launched {
            ProcessRunError::after_launch(ProcessRunFailure::TimedOut)
        } else {
            ProcessRunError::not_started(ProcessRunFailure::TimedOut)
        });
    }
    let invocation = TrustedInvocation::new_for_current_user_systemd(
        Host::Linux,
        program,
        arguments,
        remaining.min(Duration::from_secs(120)),
    )
    .map_err(|_| failure(*launched))?;
    let output = match runner.run(invocation, cancellation) {
        Ok(output) => {
            *launched = true;
            output
        }
        Err(error) => {
            *launched |= error.launched;
            return Err(error);
        }
    };
    if output.exit_code != 0 || output.stdout_truncated || output.stderr_truncated {
        return Err(failure(*launched));
    }
    Ok(output)
}

fn systemctl_args(mut arguments: Vec<TrustedArgument>) -> Vec<TrustedArgument> {
    arguments.insert(0, TrustedArgument::SystemctlUser);
    arguments
}

fn enable_arguments(already_active: bool) -> Vec<TrustedArgument> {
    let mut arguments = systemctl_args(vec![TrustedArgument::SystemctlEnable]);
    if !already_active {
        arguments.push(TrustedArgument::SystemctlNow);
    }
    arguments.push(TrustedArgument::LinuxRootlessServiceUnit);
    arguments
}

fn failure(launched: bool) -> ProcessRunError {
    if launched {
        ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch)
    } else {
        ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch)
    }
}

#[cfg(test)]
#[path = "rootless_service/tests.rs"]
mod tests;
