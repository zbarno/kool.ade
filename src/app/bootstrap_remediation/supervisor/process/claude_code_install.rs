#[cfg(target_os = "linux")]
mod manifest;
#[cfg(target_os = "linux")]
mod stage;

#[cfg(target_os = "linux")]
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::Instant,
};

#[cfg(target_os = "linux")]
use super::{
    NativeCommandRunner, SystemCommandRunner,
    program::{ClaudeReleaseAsset, InternalInvocation, TrustedProgram},
};
use super::{ProcessOutput, ProcessRunError, ProcessRunFailure, program::TrustedInvocation};
#[cfg(target_os = "linux")]
use crate::app::bootstrap_remediation::claude_code_install::ClaudeInstallProgress;
#[cfg(target_os = "linux")]
use crate::app::bootstrap_remediation::claude_code_install::recipe;
use crate::app::bootstrap_remediation::supervisor::Cancellation;

#[cfg(target_os = "linux")]
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
#[cfg(target_os = "linux")]
const MAX_SIGNATURE_BYTES: u64 = 16 * 1024;
#[cfg(target_os = "linux")]
const MAX_KEY_BYTES: u64 = 64 * 1024;

pub(super) fn run(
    invocation: TrustedInvocation,
    cancellation: &Cancellation,
) -> Result<ProcessOutput, ProcessRunError> {
    #[cfg(target_os = "linux")]
    {
        run_linux(invocation, cancellation)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = invocation;
        let _ = cancellation;
        Err(ProcessRunError::not_started(ProcessRunFailure::Unavailable))
    }
}

#[cfg(target_os = "linux")]
fn run_linux(
    invocation: TrustedInvocation,
    cancellation: &Cancellation,
) -> Result<ProcessOutput, ProcessRunError> {
    let Some(progress) = invocation.progress.clone() else {
        return Err(ProcessRunError::not_started(
            ProcessRunFailure::FailedBeforeLaunch,
        ));
    };
    let Some(InternalInvocation::ClaudeCodeInstall { target }) = invocation.internal else {
        return Err(ProcessRunError::not_started(
            ProcessRunFailure::FailedBeforeLaunch,
        ));
    };
    if invocation.program != TrustedProgram::ClaudeCodeInstall
        || invocation.host != crate::app::bootstrap_repair_policy::Host::Linux
        || cancellation.is_cancelled()
    {
        return Err(ProcessRunError::not_started(
            if cancellation.is_cancelled() {
                ProcessRunFailure::Cancelled
            } else {
                ProcessRunFailure::FailedBeforeLaunch
            },
        ));
    }
    let result = execute_install(&target, invocation.timeout, cancellation, &progress);
    match result {
        Ok(()) => Ok(ProcessOutput {
            exit_code: 0,
            stdout: format!("Installed Claude Code {} for this user.", recipe::RELEASE),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        }),
        Err(InstallFailure::Cancelled) => {
            Err(ProcessRunError::after_launch(ProcessRunFailure::Cancelled))
        }
        Err(InstallFailure::TimedOut) => {
            Err(ProcessRunError::after_launch(ProcessRunFailure::TimedOut))
        }
        Err(InstallFailure::Failed) => Ok(ProcessOutput {
            exit_code: 1,
            stdout: String::new(),
            stderr: "The pinned Claude Code release could not be verified or installed.".into(),
            stdout_truncated: false,
            stderr_truncated: false,
        }),
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InstallFailure {
    Cancelled,
    TimedOut,
    Failed,
}

#[cfg(target_os = "linux")]
fn execute_install(
    target: &Path,
    timeout: std::time::Duration,
    cancellation: &Cancellation,
    progress: &std::sync::mpsc::Sender<ClaudeInstallProgress>,
) -> Result<(), InstallFailure> {
    recipe::require_supported_host().map_err(|_| InstallFailure::Failed)?;
    if recipe::validate_target(&target.to_string_lossy()).is_err() {
        return Err(InstallFailure::Failed);
    }
    let started = Instant::now();
    let staging = tempfile::Builder::new()
        .prefix("koolade-claude-release-")
        .tempdir()
        .map_err(|_| InstallFailure::Failed)?;
    stage::validate_private_staging(staging.path()).map_err(|_| InstallFailure::Failed)?;
    report_progress(progress, ClaudeInstallProgress::Download);
    let key_path = download(
        ClaudeReleaseAsset::PublicKey,
        MAX_KEY_BYTES,
        staging.path(),
        started,
        timeout,
        cancellation,
    )?;
    let manifest_path = download(
        ClaudeReleaseAsset::Manifest,
        MAX_MANIFEST_BYTES,
        staging.path(),
        started,
        timeout,
        cancellation,
    )?;
    let signature_path = download(
        ClaudeReleaseAsset::Signature,
        MAX_SIGNATURE_BYTES,
        staging.path(),
        started,
        timeout,
        cancellation,
    )?;
    let binary_path = download(
        ClaudeReleaseAsset::LinuxX64Binary,
        recipe::BINARY_SIZE,
        staging.path(),
        started,
        timeout,
        cancellation,
    )?;
    let manifest = read_bounded(&manifest_path, MAX_MANIFEST_BYTES)?;
    let signature = read_bounded(&signature_path, MAX_SIGNATURE_BYTES)?;
    let key = read_bounded(&key_path, MAX_KEY_BYTES)?;
    report_progress(progress, ClaudeInstallProgress::TrustVerification);
    manifest::verify_signed_manifest(&manifest, &signature, &key)
        .map_err(|_| InstallFailure::Failed)?;
    manifest::verify_binary(&binary_path).map_err(|_| InstallFailure::Failed)?;
    if cancellation.is_cancelled() {
        return Err(InstallFailure::Cancelled);
    }
    if started.elapsed() >= timeout {
        return Err(InstallFailure::TimedOut);
    }
    report_progress(progress, ClaudeInstallProgress::CurrentUserInstall);
    stage::install_verified_binary(&binary_path, target, recipe::BINARY_SHA256)
        .map_err(|_| InstallFailure::Failed)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn report_progress(
    progress: &std::sync::mpsc::Sender<ClaudeInstallProgress>,
    phase: ClaudeInstallProgress,
) {
    let _ = progress.send(phase);
}

#[cfg(target_os = "linux")]
fn download(
    asset: ClaudeReleaseAsset,
    max_bytes: u64,
    staging: &Path,
    started: Instant,
    timeout: std::time::Duration,
    cancellation: &Cancellation,
) -> Result<PathBuf, InstallFailure> {
    if cancellation.is_cancelled() {
        return Err(InstallFailure::Cancelled);
    }
    let remaining = timeout
        .checked_sub(started.elapsed())
        .ok_or(InstallFailure::TimedOut)?;
    if remaining.is_zero() {
        return Err(InstallFailure::TimedOut);
    }
    let path = staging.join(match asset {
        ClaudeReleaseAsset::PublicKey => "anthropic-key.asc",
        ClaudeReleaseAsset::Manifest => "manifest.json",
        ClaudeReleaseAsset::Signature => "manifest.json.sig",
        ClaudeReleaseAsset::LinuxX64Binary => "claude-linux-x64",
    });
    let invocation = TrustedInvocation::claude_download(asset, path.clone(), max_bytes, remaining)
        .map_err(|_| InstallFailure::Failed)?;
    let output = SystemCommandRunner
        .run(invocation, cancellation)
        .map_err(|error| match error.failure {
            ProcessRunFailure::Cancelled => InstallFailure::Cancelled,
            ProcessRunFailure::TimedOut => InstallFailure::TimedOut,
            _ => InstallFailure::Failed,
        })?;
    if output.exit_code != 0 {
        return Err(InstallFailure::Failed);
    }
    let metadata = std::fs::symlink_metadata(&path).map_err(|_| InstallFailure::Failed)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > max_bytes
    {
        return Err(InstallFailure::Failed);
    }
    Ok(path)
}

#[cfg(target_os = "linux")]
fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>, InstallFailure> {
    let file = File::open(path).map_err(|_| InstallFailure::Failed)?;
    let metadata = file.metadata().map_err(|_| InstallFailure::Failed)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(InstallFailure::Failed);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| InstallFailure::Failed)?;
    if bytes.len() as u64 > maximum {
        return Err(InstallFailure::Failed);
    }
    Ok(bytes)
}
