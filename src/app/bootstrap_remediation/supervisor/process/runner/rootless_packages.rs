use std::time::{Duration, Instant};

mod dnf;

use crate::app::{
    bootstrap_remediation::{
        ProcessOutput, ProcessRunError, ProcessRunFailure, SystemCommandRunner, TrustedArgument,
        TrustedInvocation, TrustedProgram, package_install_arguments, package_source_artifacts,
        production::dnf::validate_repository_files, production_package_source_ready,
    },
    bootstrap_repair_policy::Host,
};

const FILE_INSTALL_TIMEOUT: Duration = Duration::from_secs(20);
const APT_TIMEOUT: Duration = Duration::from_secs(120);

pub(super) fn execute(
    runner: &SystemCommandRunner,
    invocation: TrustedInvocation,
    cancellation: &crate::app::bootstrap_remediation::Cancellation,
) -> Result<ProcessOutput, ProcessRunError> {
    if invocation.program() != TrustedProgram::LinuxRootlessPackageSetup
        || invocation.host() != Host::Linux
        || invocation.environment != super::super::program::TrustedEnvironment::CurrentUserSystemd
        || invocation.arguments.len() != 1
    {
        return Err(ProcessRunError::not_started(
            ProcessRunFailure::FailedBeforeLaunch,
        ));
    }
    let recipe = invocation
        .rootless_package_recipe()
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    let artifacts = package_source_artifacts(recipe)
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    if recipe == crate::app::setup_attention::catalog::LinuxRootlessRecipeV1::Fedora45Amd64 {
        dnf::execute_sequence(
            runner,
            invocation.timeout,
            recipe,
            artifacts,
            || validate_repository_files(recipe),
            cancellation,
        )
    } else {
        execute_sequence(
            runner,
            invocation.timeout,
            recipe,
            artifacts,
            || production_package_source_ready(recipe),
            cancellation,
        )
    }
}

pub(super) trait ElevatedCommandRunner {
    fn run_elevated(
        &self,
        invocation: TrustedInvocation,
        cancellation: &crate::app::bootstrap_remediation::Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError>;
}

impl ElevatedCommandRunner for SystemCommandRunner {
    fn run_elevated(
        &self,
        invocation: TrustedInvocation,
        cancellation: &crate::app::bootstrap_remediation::Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        self.execute(invocation, cancellation, true)
    }
}

fn execute_sequence(
    runner: &dyn ElevatedCommandRunner,
    timeout: Duration,
    recipe: crate::app::setup_attention::catalog::LinuxRootlessRecipeV1,
    artifacts: crate::app::bootstrap_remediation::PackageSourceArtifacts,
    source_ready: impl FnOnce() -> anyhow::Result<()>,
    cancellation: &crate::app::bootstrap_remediation::Cancellation,
) -> Result<ProcessOutput, ProcessRunError> {
    let deadline = Instant::now() + timeout;
    let crate::app::bootstrap_remediation::PackageSourceArtifacts {
        create_keyrings_directory,
        key,
        sources,
    } = artifacts;

    if create_keyrings_directory {
        let mkdir = TrustedInvocation::new_for_current_user_systemd(
            Host::Linux,
            TrustedProgram::Mkdir,
            vec![
                TrustedArgument::MkdirMode0755,
                TrustedArgument::MkdirDockerAptKeyringsDirectory,
            ],
            step_timeout(deadline)?.min(FILE_INSTALL_TIMEOUT),
        )
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
        let output = runner.run_elevated(mkdir, cancellation)?;
        if output.exit_code != 0 {
            return Ok(output);
        }
    }

    if let Some(key) = key {
        let output =
            create_repository_file(runner, key, true, step_timeout(deadline)?, cancellation)?;
        if output.exit_code != 0 {
            return Ok(output);
        }
    }
    if let Some(sources) = sources {
        let output = create_repository_file(
            runner,
            sources,
            false,
            step_timeout(deadline)?,
            cancellation,
        )?;
        if output.exit_code != 0 {
            return Ok(output);
        }
    }

    let update_arguments = TrustedArgument::apt_source_options()
        .into_iter()
        .chain([TrustedArgument::AptUpdate])
        .collect();
    let update = TrustedInvocation::new_for_current_user_systemd(
        Host::Linux,
        TrustedProgram::AptGet,
        update_arguments,
        step_timeout(deadline)?.min(APT_TIMEOUT),
    )
    .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    let output = runner.run_elevated(update, cancellation)?;
    if output.exit_code != 0 {
        return Ok(output);
    }

    source_ready()
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
    let install = TrustedInvocation::new_for_current_user_systemd(
        Host::Linux,
        TrustedProgram::AptGet,
        package_install_arguments(recipe)
            .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?,
        step_timeout(deadline)?.min(APT_TIMEOUT),
    )
    .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
    runner.run_elevated(install, cancellation)
}

fn create_repository_file(
    runner: &dyn ElevatedCommandRunner,
    artifact: crate::app::bootstrap_remediation::VerifiedArtifact,
    is_key: bool,
    timeout: Duration,
    cancellation: &crate::app::bootstrap_remediation::Cancellation,
) -> Result<ProcessOutput, ProcessRunError> {
    let (artifact, destination) = if is_key {
        (
            TrustedArgument::DockerAptKeyDdInput(artifact),
            TrustedArgument::DdDockerAptKeyOutput,
        )
    } else {
        (
            TrustedArgument::DockerAptSourcesDdInput(artifact),
            TrustedArgument::DdDockerAptSourcesOutput,
        )
    };
    let invocation = TrustedInvocation::new_for_current_user_systemd(
        Host::Linux,
        TrustedProgram::Dd,
        vec![
            artifact,
            destination,
            TrustedArgument::DdOutputExclusive,
            TrustedArgument::DdSync,
            TrustedArgument::DdStatusNone,
        ],
        timeout.min(FILE_INSTALL_TIMEOUT),
    )
    .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    runner.run_elevated(invocation, cancellation)
}

fn step_timeout(deadline: Instant) -> Result<Duration, ProcessRunError> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(ProcessRunError::after_launch(ProcessRunFailure::TimedOut));
    }
    Ok(remaining)
}

#[cfg(test)]
#[path = "rootless_packages_tests.rs"]
mod tests;
