use std::time::{Duration, Instant};

use crate::app::bootstrap_remediation::{
    DnfStageId, ProcessOutput, ProcessRunError, ProcessRunFailure, TrustedArgument,
    TrustedInvocation, TrustedProgram, VerifiedDnfRpm,
};
use crate::app::bootstrap_repair_policy::Host;
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

use super::super::ElevatedCommandRunner;
use super::{stage, verification};

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn execute_staged_sequence(
    runner: &dyn ElevatedCommandRunner,
    recipe: LinuxRootlessRecipeV1,
    stage_id: &DnfStageId,
    deadline: Instant,
    repository_ready: &impl Fn() -> anyhow::Result<()>,
    cancellation: &crate::app::bootstrap_remediation::Cancellation,
) -> Result<ProcessOutput, ProcessRunError> {
    let created = run_step(
        runner,
        TrustedProgram::Mkdir,
        stage::create_directories(stage_id),
        super::step_timeout(deadline)?,
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    if created.exit_code != 0 {
        return Ok(created);
    }
    repository_ready()
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;

    let downloaded = run_step(
        runner,
        TrustedProgram::Dnf,
        stage::download_arguments(recipe, stage_id)
            .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?,
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    if downloaded.exit_code != 0 {
        return Ok(downloaded);
    }

    let listing = run_step(
        runner,
        TrustedProgram::Find,
        stage::find_arguments(stage_id),
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    if listing.exit_code != 0 {
        return Ok(listing);
    }
    if listing.stdout_truncated || listing.stderr_truncated {
        return Err(ProcessRunError::after_launch(
            ProcessRunFailure::FailedAfterLaunch,
        ));
    }
    let packages = stage::parse_package_manifest(&listing.stdout, stage_id)
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;

    let hashes = run_step(
        runner,
        TrustedProgram::Sha256sum,
        stage::hash_arguments(&packages),
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    if hashes.exit_code != 0 {
        return Ok(hashes);
    }
    let hash_bound_packages = stage::parse_sha256_output(&hashes, &packages)
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;

    let initialized = run_step(
        runner,
        TrustedProgram::RpmDb,
        verification::rpmdb_init_arguments(stage_id),
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    if initialized.exit_code != 0 {
        return Ok(initialized);
    }
    let imported = run_step(
        runner,
        TrustedProgram::RpmKeys,
        verification::rpm_import_arguments(stage_id),
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    if imported.exit_code != 0 {
        return Ok(imported);
    }
    let inventory = run_step(
        runner,
        TrustedProgram::RpmKeys,
        verification::rpm_list_arguments(stage_id),
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    verification::validate_key_inventory(&inventory)
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;

    let signatures = run_step(
        runner,
        TrustedProgram::RpmKeys,
        verification::rpm_check_arguments(stage_id, &hash_bound_packages),
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    if signatures.exit_code != 0 {
        return Ok(signatures);
    }
    if signatures.stdout_truncated || signatures.stderr_truncated {
        return Err(ProcessRunError::after_launch(
            ProcessRunFailure::FailedAfterLaunch,
        ));
    }
    let verified_packages = hash_bound_packages
        .iter()
        .cloned()
        .map(VerifiedDnfRpm::after_rpmkeys_success)
        .collect::<Vec<_>>();

    let staged_names_output = run_step(
        runner,
        TrustedProgram::Rpm,
        verification::staged_package_name_arguments(&verified_packages),
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    if staged_names_output.exit_code != 0 {
        return Ok(staged_names_output);
    }
    let staged_names =
        verification::staged_package_names(&staged_names_output, stage_id, verified_packages.len())
            .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;

    let installed_names_output = run_step(
        runner,
        TrustedProgram::Rpm,
        verification::installed_package_name_arguments(),
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    let installed_names = verification::installed_package_names(&installed_names_output)
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
    verification::ensure_no_installed_package_overlap(&staged_names, &installed_names)
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;

    repository_ready()
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
    let rechecked_listing = run_step(
        runner,
        TrustedProgram::Find,
        stage::find_arguments(stage_id),
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    if rechecked_listing.exit_code != 0 {
        return Ok(rechecked_listing);
    }
    if rechecked_listing.stdout_truncated || rechecked_listing.stderr_truncated {
        return Err(ProcessRunError::after_launch(
            ProcessRunFailure::FailedAfterLaunch,
        ));
    }
    let rechecked_packages = stage::parse_package_manifest(&rechecked_listing.stdout, stage_id)
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
    if rechecked_packages != packages {
        return Err(ProcessRunError::after_launch(
            ProcessRunFailure::FailedAfterLaunch,
        ));
    }
    let rechecked_hashes = run_step(
        runner,
        TrustedProgram::Sha256sum,
        stage::hash_arguments(&rechecked_packages),
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    if rechecked_hashes.exit_code != 0 {
        return Ok(rechecked_hashes);
    }
    let rechecked_hash_bound =
        stage::parse_sha256_output(&rechecked_hashes, &rechecked_packages)
            .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
    if rechecked_hash_bound != hash_bound_packages {
        return Err(ProcessRunError::after_launch(
            ProcessRunFailure::FailedAfterLaunch,
        ));
    }
    let inventory = run_step(
        runner,
        TrustedProgram::RpmKeys,
        verification::rpm_list_arguments(stage_id),
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )?;
    verification::validate_key_inventory(&inventory)
        .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;

    run_step(
        runner,
        TrustedProgram::Dnf,
        stage::offline_install_arguments(recipe, stage_id, staged_names, &verified_packages)
            .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?,
        super::step_timeout(deadline)?.min(super::super::APT_TIMEOUT),
        cancellation,
        ProcessRunFailure::FailedAfterLaunch,
    )
}

pub(super) fn cleanup_stage(
    runner: &dyn ElevatedCommandRunner,
    stage_id: &DnfStageId,
) -> Result<(), ProcessRunError> {
    let cleanup = TrustedInvocation::new_for_current_user_systemd(
        Host::Linux,
        TrustedProgram::Rm,
        vec![
            TrustedArgument::RmRecursive,
            TrustedArgument::RmForce,
            TrustedArgument::EndOfOptions,
            TrustedArgument::DnfStageRoot(stage_id.clone()),
        ],
        CLEANUP_TIMEOUT,
    )
    .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
    let cleanup_token = crate::app::bootstrap_remediation::Cancellation::new();
    let output = runner.run_elevated(cleanup, &cleanup_token)?;
    if output.exit_code == 0 {
        Ok(())
    } else {
        Err(ProcessRunError::after_launch(
            ProcessRunFailure::FailedAfterLaunch,
        ))
    }
}

pub(super) fn run_step(
    runner: &dyn ElevatedCommandRunner,
    program: TrustedProgram,
    arguments: Vec<TrustedArgument>,
    timeout: Duration,
    cancellation: &crate::app::bootstrap_remediation::Cancellation,
    invocation_failure: ProcessRunFailure,
) -> Result<ProcessOutput, ProcessRunError> {
    let invocation =
        TrustedInvocation::new_for_current_user_systemd(Host::Linux, program, arguments, timeout)
            .map_err(|_| ProcessRunError::after_launch(invocation_failure))?;
    runner.run_elevated(invocation, cancellation)
}
