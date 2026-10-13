use std::{
    io::Read,
    process::{Child, ExitStatus},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use super::Capture;

pub(super) fn spawn_reader(
    mut reader: impl Read + Send + 'static,
    capture: Arc<Mutex<Capture>>,
    done: mpsc::Sender<()>,
) {
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    if let Ok(mut capture) = capture.lock() {
                        capture.append(&buffer[..read]);
                    }
                }
            }
        }
        let _ = done.send(());
    });
}

pub(super) fn wait_bounded(
    child: &mut Child,
    pid: u32,
    deadline: Instant,
    cancellation: Option<&std::sync::atomic::AtomicBool>,
) -> anyhow::Result<ExitStatus> {
    loop {
        if is_cancelled(cancellation) {
            terminate_group(pid);
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("probe was cancelled");
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                terminate_group(pid);
                return Ok(status);
            }
            Ok(None) => {}
            Err(error) => {
                terminate_group(pid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        }
        if Instant::now() >= deadline {
            terminate_group(pid);
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("probe process exceeded its deadline");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn is_cancelled(cancellation: Option<&std::sync::atomic::AtomicBool>) -> bool {
    cancellation.is_some_and(|token| token.load(std::sync::atomic::Ordering::Acquire))
}

#[cfg(unix)]
pub(super) fn terminate_group(pid: u32) {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    if let Ok(pid) = i32::try_from(pid) {
        unsafe {
            kill(-pid, 9);
        }
    }
}

#[cfg(not(unix))]
pub(super) fn terminate_group(_pid: u32) {}
