#[cfg(target_os = "linux")]
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "linux")]
use super::program::{InternalInvocation, TrustedProgram};
use super::{ProcessOutput, ProcessRunError, ProcessRunFailure, program::TrustedInvocation};
use crate::app::bootstrap_remediation::supervisor::Cancellation;

#[cfg(target_os = "linux")]
const MAX_CONNECT_SECONDS: u64 = 20;
#[cfg(target_os = "linux")]
const POLL_INTERVAL: Duration = Duration::from_millis(20);

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
    use std::{
        os::unix::fs::{MetadataExt, OpenOptionsExt},
        os::unix::process::CommandExt,
    };

    let Some(InternalInvocation::ClaudeCodeDownload {
        asset,
        destination,
        max_bytes,
    }) = invocation.internal
    else {
        return Err(ProcessRunError::not_started(
            ProcessRunFailure::FailedBeforeLaunch,
        ));
    };
    if invocation.program != TrustedProgram::ClaudeCodeDownload
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
    let parent = destination
        .parent()
        .ok_or_else(|| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    let parent_metadata = std::fs::symlink_metadata(parent)
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    if !parent_metadata.is_dir()
        || parent_metadata.file_type().is_symlink()
        || parent_metadata.uid() != unsafe { libc::geteuid() }
        || parent_metadata.mode() & 0o077 != 0
        || parent.canonicalize().ok().as_deref() != Some(parent)
    {
        return Err(ProcessRunError::not_started(
            ProcessRunFailure::FailedBeforeLaunch,
        ));
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&destination)
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
    let curl = TrustedProgram::ClaudeCodeDownload
        .resolve(invocation.host)
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::Unavailable))?;
    super::platform::validate_program_path(&curl)
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::Unavailable))?;
    let mut command = Command::new(curl);
    command
        .args([
            "--disable",
            "--fail",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--connect-timeout",
        ])
        .arg(MAX_CONNECT_SECONDS.to_string())
        .arg(asset.url())
        .env_clear()
        .env("HOME", parent)
        .env("PATH", "/usr/bin:/bin")
        .current_dir(parent)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(super::platform::install_linux_process_group_guard);
    }
    let mut child = command
        .spawn()
        .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::Unavailable))?;
    let pid = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
    let (read_tx, read_rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let result = copy_bounded(stdout, &mut output, max_bytes);
        let _ = read_tx.send(result);
    });
    let deadline = Instant::now() + invocation.timeout;
    let mut bytes = None;
    let status = loop {
        if cancellation.is_cancelled() {
            super::terminate::terminate(pid, &mut child);
            let _ = reader.join();
            return Err(ProcessRunError::after_launch(ProcessRunFailure::Cancelled));
        }
        if Instant::now() >= deadline {
            super::terminate::terminate(pid, &mut child);
            let _ = reader.join();
            return Err(ProcessRunError::after_launch(ProcessRunFailure::TimedOut));
        }
        match read_rx.try_recv() {
            Ok(Ok(length)) => bytes = Some(length),
            Ok(Err(_)) => {
                super::terminate::terminate(pid, &mut child);
                let _ = reader.join();
                return Err(ProcessRunError::after_launch(
                    ProcessRunFailure::FailedAfterLaunch,
                ));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                super::terminate::terminate(pid, &mut child);
                let _ = reader.join();
                return Err(ProcessRunError::after_launch(
                    ProcessRunFailure::FailedAfterLaunch,
                ));
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                super::terminate::cleanup_descendants(pid);
                break status;
            }
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(_) => {
                super::terminate::terminate(pid, &mut child);
                let _ = reader.join();
                return Err(ProcessRunError::after_launch(
                    ProcessRunFailure::FailedAfterLaunch,
                ));
            }
        }
    };
    if bytes.is_none() {
        bytes = read_rx.recv().ok().and_then(Result::ok);
    }
    if reader.join().is_err() || bytes.is_none() {
        return Err(ProcessRunError::after_launch(
            ProcessRunFailure::FailedAfterLaunch,
        ));
    }
    Ok(ProcessOutput {
        exit_code: status.code().unwrap_or(1),
        stdout: format!("Downloaded {} bounded bytes.", bytes.unwrap_or_default()),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
    })
}

#[cfg(target_os = "linux")]
fn copy_bounded(
    mut source: impl Read,
    destination: &mut File,
    max_bytes: u64,
) -> std::io::Result<u64> {
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            destination.flush()?;
            destination.sync_all()?;
            return Ok(total);
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| std::io::Error::other("download size overflow"))?;
        if total > max_bytes {
            return Err(std::io::Error::other(
                "download exceeded the exact size bound",
            ));
        }
        destination.write_all(&buffer[..read])?;
    }
}
