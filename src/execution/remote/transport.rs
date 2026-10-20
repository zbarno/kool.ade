use super::{protocol, ssh::VerifiedSshWorker};
#[cfg(test)]
use std::sync::atomic::{AtomicI32, Ordering};
use std::{
    io::{self, Read},
    process::{Child, ChildStdin},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

const STDERR_LIMIT: usize = 64 * 1024;

pub(super) trait RemoteRpcTransport: Send {
    fn send(&mut self, envelope: &protocol::EnvelopeV1) -> io::Result<()>;
    fn send_private_file_chunk(
        &mut self,
        _envelope: &protocol::EnvelopeV1,
        _chunk: &crate::execution::remote::private_files::PrivateFileChunkV1,
    ) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private-file chunk transport is unavailable",
        ))
    }
    fn receive(&mut self) -> io::Result<Option<protocol::EnvelopeV1>>;
    #[allow(dead_code)] // The test-only client hook terminates a fakeable transport.
    fn terminate(&mut self) -> io::Result<()>;
    fn diagnostic_stderr(&self) -> String;
    #[cfg(test)]
    fn test_terminator(&self) -> Option<TestTransportTerminator> {
        None
    }
}

#[cfg(test)]
#[derive(Clone)]
pub(super) struct TestTransportTerminator(Arc<AtomicI32>);

#[cfg(test)]
impl TestTransportTerminator {
    pub(super) fn terminate(&self) {
        let pid = self.0.load(Ordering::Acquire);
        #[cfg(unix)]
        if pid > 0 {
            // The E2E disconnect hook must be able to break a blocked receive while the
            // backend client mutex is held by that receive operation.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
        #[cfg(not(unix))]
        let _ = pid;
    }
}

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
    #[cfg(test)]
    test_pid: Arc<AtomicI32>,
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
        #[cfg(test)]
        let test_pid = Arc::new(AtomicI32::new(child.id() as i32));
        Ok(Self {
            child,
            input,
            output: output_rx,
            stderr,
            outbound: protocol::SessionValidator::default(),
            inbound: protocol::SessionValidator::default(),
            #[cfg(test)]
            test_pid,
        })
    }

    pub fn send(&mut self, envelope: &protocol::EnvelopeV1) -> io::Result<()> {
        if envelope.kind == protocol::EnvelopeKindV1::PrivateFileChunk {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "private file chunks require a dedicated authorized transport",
            ));
        }
        self.outbound.accept(envelope)?;
        protocol::write_frame(&mut self.input, envelope)
    }

    pub(crate) fn send_private_file_chunk(
        &mut self,
        envelope: &protocol::EnvelopeV1,
        chunk: &crate::execution::remote::private_files::PrivateFileChunkV1,
    ) -> io::Result<()> {
        self.outbound.accept(envelope)?;
        protocol::write_private_file_chunk_frame(&mut self.input, envelope, chunk)
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
        #[cfg(test)]
        self.test_pid.store(0, Ordering::Release);
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

impl RemoteRpcTransport for SshTransport {
    fn send(&mut self, envelope: &protocol::EnvelopeV1) -> io::Result<()> {
        SshTransport::send(self, envelope)
    }

    fn send_private_file_chunk(
        &mut self,
        envelope: &protocol::EnvelopeV1,
        chunk: &crate::execution::remote::private_files::PrivateFileChunkV1,
    ) -> io::Result<()> {
        SshTransport::send_private_file_chunk(self, envelope, chunk)
    }

    fn receive(&mut self) -> io::Result<Option<protocol::EnvelopeV1>> {
        SshTransport::receive(self)
    }

    fn terminate(&mut self) -> io::Result<()> {
        SshTransport::terminate(self)
    }

    fn diagnostic_stderr(&self) -> String {
        SshTransport::diagnostic_stderr(self)
    }

    #[cfg(test)]
    fn test_terminator(&self) -> Option<TestTransportTerminator> {
        Some(TestTransportTerminator(self.test_pid.clone()))
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

    fn private_chunk() -> protocol::EnvelopeV1 {
        protocol::EnvelopeV1 {
            version: protocol::PROTOCOL_VERSION_V1,
            request_id: "00000000-0000-4000-8000-000000000151".into(),
            execution_id: "00000000-0000-4000-8000-000000000152".into(),
            sequence: 1,
            kind: protocol::EnvelopeKindV1::PrivateFileChunk,
            payload: serde_json::json!({"bytes":[115,121,110,116,104,101,116,105,99]}),
        }
    }

    #[test]
    fn stderr_capture_is_bounded_and_output_reader_does_not_consume_it() {
        let capture = Arc::new(Mutex::new(Vec::new()));
        drain_stderr(Cursor::new(vec![b'x'; STDERR_LIMIT + 8]), capture.clone());
        assert_eq!(capture.lock().unwrap().len(), STDERR_LIMIT);
    }

    #[cfg(unix)]
    #[test]
    fn public_ssh_transport_rejects_private_chunks_before_writing_a_frame() {
        let mut child = std::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let test_pid = Arc::new(AtomicI32::new(child.id() as i32));
        let transport = SshTransport {
            input: child.stdin.take().unwrap(),
            child,
            output: mpsc::channel().1,
            stderr: Arc::new(Mutex::new(Vec::new())),
            outbound: protocol::SessionValidator::default(),
            inbound: protocol::SessionValidator::default(),
            test_pid,
        };
        let mut transport = transport;

        let error = transport.send(&private_chunk()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
}
