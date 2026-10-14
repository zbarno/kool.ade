use super::{protocol, ssh::VerifiedSshWorker};
use std::{
    io::{self, Read},
    process::{Child, ChildStdin},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

const STDERR_LIMIT: usize = 64 * 1024;
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(16 * 60);

/// One authenticated SSH stdio session. SSH stderr is drained independently
/// from protocol stdout and retained only as a bounded, redacted diagnostic.
pub struct SshTransport {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<io::Result<Option<protocol::EnvelopeV1>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
    outbound: protocol::SessionValidator,
    inbound: protocol::SessionValidator,
}

impl SshTransport {
    pub fn connect(worker: &VerifiedSshWorker) -> io::Result<Self> {
        let mut child = worker.spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("SSH protocol stdin is unavailable"))?;
        let mut output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("SSH protocol stdout is unavailable"))?;
        let (output_tx, output_rx) = mpsc::channel();
        thread::spawn(move || {
            loop {
                let frame = protocol::read_frame(&mut output);
                let finished = !matches!(frame, Ok(Some(_)));
                if output_tx.send(frame).is_err() || finished {
                    break;
                }
            }
        });
        let stderr_pipe = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("SSH diagnostic stderr is unavailable"))?;
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let capture = stderr.clone();
        thread::spawn(move || drain_stderr(stderr_pipe, capture));
        Ok(Self {
            child,
            input,
            output: output_rx,
            stderr,
            outbound: protocol::SessionValidator::default(),
            inbound: protocol::SessionValidator::default(),
        })
    }

    pub fn send(&mut self, envelope: &protocol::EnvelopeV1) -> io::Result<()> {
        self.outbound.accept(envelope)?;
        protocol::write_frame(&mut self.input, envelope)
    }

    pub fn receive(&mut self) -> io::Result<Option<protocol::EnvelopeV1>> {
        let frame = match self.output.recv_timeout(RESPONSE_TIMEOUT) {
            Ok(frame) => frame?,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = self.terminate();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "remote worker response exceeded its deadline",
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(None),
        };
        let Some(envelope) = frame else {
            return Ok(None);
        };
        self.inbound.accept(&envelope)?;
        Ok(Some(envelope))
    }

    pub fn try_wait(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    pub fn terminate(&mut self) -> io::Result<()> {
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
        }
        let _ = self.child.wait()?;
        Ok(())
    }

    pub fn diagnostic_stderr(&self) -> String {
        let bytes = self
            .stderr
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        crate::error::redact_secrets(&text)
    }
}

impl Drop for SshTransport {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

fn drain_stderr(mut stream: impl Read, capture: Arc<Mutex<Vec<u8>>>) {
    let mut chunk = [0_u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                if let Ok(mut bytes) = capture.lock() {
                    let remaining = STDERR_LIMIT.saturating_sub(bytes.len());
                    bytes.extend_from_slice(&chunk[..count.min(remaining)]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn stderr_capture_is_bounded_and_output_reader_does_not_consume_it() {
        let capture = Arc::new(Mutex::new(Vec::new()));
        drain_stderr(Cursor::new(vec![b'x'; STDERR_LIMIT + 8]), capture.clone());
        assert_eq!(capture.lock().unwrap().len(), STDERR_LIMIT);
    }
}
