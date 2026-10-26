use std::sync::Arc;

use super::{
    NativeCommandRunner, OUTPUT_LIMIT, ProcessOutput, ProcessRunError, ProcessRunFailure,
    READER_JOIN_TIMEOUT, SystemCommandRunner,
    capture::{BoundedCapture, spawn_reader},
    program::TrustedInvocation,
    terminate,
};
use crate::app::bootstrap_remediation::supervisor::Cancellation;

impl NativeCommandRunner for SystemCommandRunner {
    fn run(
        &self,
        invocation: TrustedInvocation,
        cancellation: &Cancellation,
    ) -> Result<ProcessOutput, ProcessRunError> {
        use std::{
            process::{Command, Stdio},
            sync::{
                Mutex,
                atomic::{AtomicBool, Ordering},
                mpsc,
            },
            thread,
            time::{Duration, Instant},
        };

        if !super::platform::platform_execution_supported() {
            return Err(ProcessRunError::not_started(ProcessRunFailure::Unavailable));
        }
        if cancellation.is_cancelled() {
            return Err(ProcessRunError::not_started(ProcessRunFailure::Cancelled));
        }
        let program_path = invocation
            .program
            .resolve(invocation.host)
            .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
        super::platform::validate_program_path(&program_path)
            .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
        let working =
            crate::harness::execution_security::PrivateDirectory::create("koolade-remediation")
                .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
        let mut command = Command::new(&program_path);
        for argument in &invocation.arguments {
            argument
                .validate()
                .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
            command.arg(argument.as_os_string());
        }
        command
            .env_clear()
            .env("HOME", &working.path)
            .env("TMPDIR", &working.path)
            .env("LANG", "C")
            .env("LC_ALL", "C")
            .env("NO_COLOR", "1")
            .current_dir(&working.path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        command
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("XDG_CONFIG_HOME", working.path.join("config"))
            .env("XDG_DATA_HOME", working.path.join("data"))
            .env("XDG_CACHE_HOME", working.path.join("cache"));
        #[cfg(windows)]
        {
            let system_directory = program_path.parent().ok_or_else(|| {
                ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch)
            })?;
            command.env("PATH", system_directory).env(
                "SYSTEMROOT",
                system_directory.parent().unwrap_or(system_directory),
            );
        }
        #[cfg(test)]
        if let Some(mode) = &invocation.test_mode {
            command.env("KOOLADE_REMEDIATION_TEST_MODE", mode);
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            let artifact_fds = invocation.artifact_fds();
            unsafe {
                command.pre_exec(move || {
                    super::platform::make_artifacts_inheritable(&artifact_fds)?;
                    super::platform::install_linux_process_group_guard()
                });
            }
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::process::CommandExt;
            let artifact_fds = invocation.artifact_fds();
            command.process_group(0);
            unsafe {
                command
                    .pre_exec(move || super::platform::make_artifacts_inheritable(&artifact_fds));
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
        }
        let mut child = command
            .spawn()
            .map_err(|_| ProcessRunError::not_started(ProcessRunFailure::FailedBeforeLaunch))?;
        let pid = child.id();
        #[cfg(windows)]
        let process_tree = match crate::harness::discovery::windows::WindowsProcessTree::attach(pid)
        {
            Ok(tree) => Some(tree),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = error;
                return Err(ProcessRunError::after_launch(
                    ProcessRunFailure::FailedAfterLaunch,
                ));
            }
        };
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
        let stdout_capture = Arc::new(Mutex::new(BoundedCapture::new(OUTPUT_LIMIT)));
        let stderr_capture = Arc::new(Mutex::new(BoundedCapture::new(OUTPUT_LIMIT)));
        let (done_tx, done_rx) = mpsc::channel();
        let reader_stop = Arc::new(AtomicBool::new(false));
        let stdout_thread = match spawn_reader(
            stdout,
            stdout_capture.clone(),
            done_tx.clone(),
            reader_stop.clone(),
        ) {
            Ok(thread) => thread,
            Err(_) => {
                terminate::terminate(pid, &mut child);
                #[cfg(windows)]
                drop(process_tree);
                return Err(ProcessRunError::after_launch(
                    ProcessRunFailure::FailedAfterLaunch,
                ));
            }
        };
        let stderr_thread =
            match spawn_reader(stderr, stderr_capture.clone(), done_tx, reader_stop.clone()) {
                Ok(thread) => thread,
                Err(_) => {
                    reader_stop.store(true, Ordering::Release);
                    terminate::terminate(pid, &mut child);
                    #[cfg(windows)]
                    drop(process_tree);
                    return Err(ProcessRunError::after_launch(
                        ProcessRunFailure::FailedAfterLaunch,
                    ));
                }
            };
        let deadline = Instant::now() + invocation.timeout;
        let status = loop {
            if cancellation.is_cancelled() {
                terminate::terminate(pid, &mut child);
                reader_stop.store(true, Ordering::Release);
                #[cfg(windows)]
                drop(process_tree);
                return Err(ProcessRunError::after_launch(ProcessRunFailure::Cancelled));
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    terminate::cleanup_descendants(pid);
                    break status;
                }
                Ok(None) => {}
                Err(error) => {
                    terminate::terminate(pid, &mut child);
                    reader_stop.store(true, Ordering::Release);
                    #[cfg(windows)]
                    drop(process_tree);
                    let _ = error;
                    return Err(ProcessRunError::after_launch(
                        ProcessRunFailure::FailedAfterLaunch,
                    ));
                }
            }
            if Instant::now() >= deadline {
                terminate::terminate(pid, &mut child);
                reader_stop.store(true, Ordering::Release);
                #[cfg(windows)]
                drop(process_tree);
                return Err(ProcessRunError::after_launch(ProcessRunFailure::TimedOut));
            }
            thread::sleep(Duration::from_millis(10));
        };
        #[cfg(windows)]
        drop(process_tree);
        let stdout_done = done_rx.recv_timeout(READER_JOIN_TIMEOUT).is_ok();
        let stderr_done = done_rx.recv_timeout(READER_JOIN_TIMEOUT).is_ok();
        if stdout_done && stderr_done {
            let _ = stdout_thread.join();
            let _ = stderr_thread.join();
        } else {
            reader_stop.store(true, Ordering::Release);
            #[cfg(unix)]
            {
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
            }
            #[cfg(windows)]
            {
                drop(stdout_thread);
                drop(stderr_thread);
            }
        }
        let stdout = stdout_capture
            .lock()
            .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
        let stderr = stderr_capture
            .lock()
            .map_err(|_| ProcessRunError::after_launch(ProcessRunFailure::FailedAfterLaunch))?;
        Ok(ProcessOutput {
            exit_code: status.code().unwrap_or(-1),
            stdout: stdout.redacted(),
            stderr: stderr.redacted(),
            stdout_truncated: stdout.truncated,
            stderr_truncated: stderr.truncated,
        })
    }
}
