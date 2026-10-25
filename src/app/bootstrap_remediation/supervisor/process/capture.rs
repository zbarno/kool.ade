#[cfg(unix)]
use std::time::Duration;
use std::{
    io::Read,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
};

pub(super) struct BoundedCapture {
    limit: usize,
    bytes: Vec<u8>,
    pub(super) truncated: bool,
}

impl BoundedCapture {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            limit,
            bytes: Vec::new(),
            truncated: false,
        }
    }

    pub(super) fn append(&mut self, chunk: &[u8]) {
        let remaining = self.limit.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        self.truncated |= chunk.len() > remaining;
    }

    pub(super) fn redacted(&self) -> String {
        let redacted = crate::error::redact_secrets(&String::from_utf8_lossy(&self.bytes));
        crate::app::bootstrap_remediation::redact_keyed_values(&redacted)
    }
}

#[cfg(unix)]
pub(super) fn spawn_reader(
    reader: impl Read + Send + std::os::fd::AsRawFd + 'static,
    capture: Arc<Mutex<BoundedCapture>>,
    done: mpsc::Sender<()>,
    stop: Arc<AtomicBool>,
) -> anyhow::Result<JoinHandle<()>> {
    set_nonblocking(&reader)?;
    Ok(spawn_reader_inner(reader, capture, done, stop))
}

#[cfg(not(unix))]
pub(super) fn spawn_reader(
    reader: impl Read + Send + 'static,
    capture: Arc<Mutex<BoundedCapture>>,
    done: mpsc::Sender<()>,
    stop: Arc<AtomicBool>,
) -> anyhow::Result<JoinHandle<()>> {
    Ok(spawn_reader_inner(reader, capture, done, stop))
}

fn spawn_reader_inner(
    mut reader: impl Read + Send + 'static,
    capture: Arc<Mutex<BoundedCapture>>,
    done: mpsc::Sender<()>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    #[cfg(not(unix))]
    let _ = &stop;
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            #[cfg(unix)]
            if stop.load(Ordering::Acquire) {
                break;
            }
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if let Ok(mut capture) = capture.lock() {
                        capture.append(&buffer[..read]);
                    }
                }
                #[cfg(unix)]
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
        let _ = done.send(());
    })
}

#[cfg(unix)]
fn set_nonblocking(reader: &impl std::os::fd::AsRawFd) -> anyhow::Result<()> {
    let descriptor = reader.as_raw_fd();
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    anyhow::ensure!(flags >= 0, "could not inspect remediation output pipe");
    anyhow::ensure!(
        unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags | libc::O_NONBLOCK) } == 0,
        "could not bound remediation output pipe reads"
    );
    Ok(())
}
