use std::time::{Duration, Instant};

use crate::app::{
    bootstrap_remediation::{
        Cancellation, ProcessOutput, ProcessRunError, ProcessRunFailure, SystemCommandRunner,
        TrustedArgument, TrustedInvocation, TrustedProgram, qemu_package_arguments,
        verify_qemu_package_candidates,
    },
    bootstrap_repair_policy::Host,
};

const APT_STEP_TIMEOUT: Duration = Duration::from_secs(120);

pub(super) fn execute(
    runner: &SystemCommandRunner,
    invocation: TrustedInvocation,
    cancellation: &Cancellation,
) -> Result<ProcessOutput, ProcessRunError> {
    if invocation.program() != TrustedProgram::LinuxQemuPackageSetup
        || invocation.host() != Host::Linux
        || invocation.environment != super::super::program::TrustedEnvironment::CurrentUserSystemd
        || invocation.arguments.len() != 1
    {
        return Err(ProcessRunError::not_started(
            ProcessRunFailure::FailedBeforeLaunch,
        ));
    }
    let recipe = invocation
        .qemu_package_recipe()
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    execute_sequence(runner, invocation, recipe, cancellation, || {
        verify_qemu_package_candidates(recipe)
    })
}

trait ElevatedCommandRunner {
    fn run_elevated(
        &self,
        invocation: TrustedInvocation,
        cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError>;
}

impl ElevatedCommandRunner for SystemCommandRunner {
    fn run_elevated(
        &self,
        invocation: TrustedInvocation,
        cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        self.execute(invocation, cancellation, true)
    }
}

fn execute_sequence(
    runner: &dyn ElevatedCommandRunner,
    invocation: TrustedInvocation,
    recipe: crate::app::setup_attention::catalog::LinuxRootlessRecipeV1,
    cancellation: &Cancellation,
    verify_refreshed_candidates: impl FnOnce() -> anyhow::Result<()>,
) -> Result<ProcessOutput, ProcessRunError> {
    let deadline = Instant::now() + invocation.timeout;

    let update_arguments = TrustedArgument::apt_source_options()
        .into_iter()
        .chain([TrustedArgument::AptUpdate])
        .collect();
    let update = apt_invocation(update_arguments, deadline)?;
    let output = runner.run_elevated(update, cancellation)?;
    if output.exit_code != 0 {
        return Ok(output);
    }

    // Recheck signed official archive origins against the metadata just fetched.
    verify_refreshed_candidates()
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
    let arguments = qemu_package_arguments(recipe)
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
    let install = apt_invocation(arguments, deadline)?;
    runner.run_elevated(install, cancellation)
}

fn apt_invocation(
    arguments: Vec<TrustedArgument>,
    deadline: Instant,
) -> Result<TrustedInvocation, ProcessRunError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(ProcessRunError::after_launch(ProcessRunFailure::TimedOut));
    }
    TrustedInvocation::new_for_current_user_systemd(
        Host::Linux,
        TrustedProgram::AptGet,
        arguments,
        remaining.min(APT_STEP_TIMEOUT),
    )
    .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))
}

#[cfg(test)]
#[path = "qemu_packages_tests.rs"]
mod tests;
