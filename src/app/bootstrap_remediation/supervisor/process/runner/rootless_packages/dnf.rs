use std::time::{Duration, Instant};

use crate::app::bootstrap_remediation::{
    DnfStageId, ProcessOutput, ProcessRunError, ProcessRunFailure, TrustedArgument,
    TrustedInvocation, TrustedProgram,
};
use crate::app::bootstrap_repair_policy::Host;
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

mod sequence;
mod stage;
mod verification;

use super::ElevatedCommandRunner;

pub(super) fn execute_sequence(
    runner: &dyn ElevatedCommandRunner,
    timeout: Duration,
    recipe: LinuxRootlessRecipeV1,
    artifacts: crate::app::bootstrap_remediation::PackageSourceArtifacts,
    repository_ready: impl Fn() -> anyhow::Result<()>,
    cancellation: &crate::app::bootstrap_remediation::Cancellation,
) -> Result<ProcessOutput, ProcessRunError> {
    use crate::app::bootstrap_remediation::production::dnf;

    let deadline = Instant::now() + timeout;
    let crate::app::bootstrap_remediation::PackageSourceArtifacts {
        create_keyrings_directory,
        key,
        sources,
    } = artifacts;
    if create_keyrings_directory {
        return Err(ProcessRunError::not_started(
            ProcessRunFailure::FailedBeforeLaunch,
        ));
    }
    if let Some(key) = key {
        let output =
            create_repository_file(runner, key, true, step_timeout(deadline)?, cancellation)?;
        if output.exit_code != 0 {
            return Ok(output);
        }
    }
    if let Some(repo) = sources {
        let output =
            create_repository_file(runner, repo, false, step_timeout(deadline)?, cancellation)?;
        if output.exit_code != 0 {
            return Ok(output);
        }
    }
    repository_ready()
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;

    let output = sequence::run_step(
        runner,
        TrustedProgram::Dnf,
        dnf::make_cache_arguments(recipe)
            .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?,
        step_timeout(deadline)?.min(super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedBeforeLaunch,
    )?;
    if output.exit_code != 0 {
        return Ok(output);
    }
    repository_ready()
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;

    let output = sequence::run_step(
        runner,
        TrustedProgram::Dnf,
        dnf::candidate_arguments(recipe)
            .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?,
        step_timeout(deadline)?.min(super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    if output.exit_code != 0 {
        return Ok(output);
    }
    if output.stdout_truncated || output.stderr_truncated {
        return Err(ProcessRunError::after_launch(
            ProcessRunFailure::FailedAfterLaunch,
        ));
    }
    dnf::validate_candidate_output(recipe, &output.stdout)
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
    repository_ready()
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;

    let stage = DnfStageId::new();
    let result = sequence::execute_staged_sequence(
        runner,
        recipe,
        &stage,
        deadline,
        &repository_ready,
        cancellation,
    );
    let cleanup = sequence::cleanup_stage(runner, &stage);
    match (result, cleanup) {
        (Ok(output), Ok(())) => Ok(output),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
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
            TrustedArgument::DockerDnfKeyDdInput(artifact),
            TrustedArgument::DdDockerDnfKeyOutput,
        )
    } else {
        (
            TrustedArgument::DockerDnfRepoDdInput(artifact),
            TrustedArgument::DdDockerDnfRepoOutput,
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
        timeout.min(super::FILE_INSTALL_TIMEOUT),
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
