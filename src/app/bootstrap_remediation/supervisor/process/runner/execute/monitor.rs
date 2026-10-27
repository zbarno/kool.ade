use std::{
    process::Child,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use super::super::super::{
    OUTPUT_LIMIT, ProcessOutput, ProcessRunError, ProcessRunFailure, READER_JOIN_TIMEOUT, capture,
    terminate,
};
use super::scope::{self, ElevatedLaunch};
use crate::app::bootstrap_remediation::supervisor::{Cancellation, TrustedInvocation};

use capture::{BoundedCapture, spawn_reader};

pub(super) fn finish(
    mut child: Child,
    invocation: &TrustedInvocation,
    cancellation: &Cancellation,
    elevated_launch: Option<&ElevatedLaunch>,
) -> Result<ProcessOutput, ProcessRunError> {
    let pid = child.id();
    #[cfg(windows)]
    let process_tree = match crate::harness::discovery::windows::WindowsProcessTree::attach(pid) {
        Ok(tree) => Some(tree),
        Err(_) => {
            stop_elevated(elevated_launch);
            let _ = child.kill();
            let _ = child.wait();
            return Err(ProcessRunError::after_launch(
                ProcessRunFailure::FailedAfterLaunch,
            ));
        }
    };

    let Some(stdout) = child.stdout.take() else {
        stop_elevated(elevated_launch);
        terminate::terminate(pid, &mut child);
        return Err(ProcessRunError::after_launch(
            ProcessRunFailure::FailedAfterLaunch,
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        stop_elevated(elevated_launch);
        terminate::terminate(pid, &mut child);
        return Err(ProcessRunError::after_launch(
            ProcessRunFailure::FailedAfterLaunch,
        ));
    };
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
            stop_elevated(elevated_launch);
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
                stop_elevated(elevated_launch);
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
            let stopped = stop_elevated(elevated_launch);
            terminate::terminate(pid, &mut child);
            reader_stop.store(true, Ordering::Release);
            #[cfg(windows)]
            drop(process_tree);
            return Err(ProcessRunError::after_launch(if stopped {
                ProcessRunFailure::Cancelled
            } else {
                ProcessRunFailure::FailedAfterLaunch
            }));
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                terminate::cleanup_descendants(pid);
                break status;
            }
            Ok(None) => {}
            Err(_) => {
                stop_elevated(elevated_launch);
                terminate::terminate(pid, &mut child);
                reader_stop.store(true, Ordering::Release);
                #[cfg(windows)]
                drop(process_tree);
                return Err(ProcessRunError::after_launch(
                    ProcessRunFailure::FailedAfterLaunch,
                ));
            }
        }
        if Instant::now() >= deadline {
            let stopped = stop_elevated(elevated_launch);
            terminate::terminate(pid, &mut child);
            reader_stop.store(true, Ordering::Release);
            #[cfg(windows)]
            drop(process_tree);
            return Err(ProcessRunError::after_launch(if stopped {
                ProcessRunFailure::TimedOut
            } else {
                ProcessRunFailure::FailedAfterLaunch
            }));
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

fn stop_elevated(launch: Option<&ElevatedLaunch>) -> bool {
    launch.is_none_or(|launch| scope::stop_unit(&launch.unit_name).is_ok())
}
