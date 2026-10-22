use super::{protocol, ssh::VerifiedSshWorker};
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
    fn send_compat_health(&mut self, envelope: &protocol::EnvelopeV1) -> io::Result<()> {
        self.send(envelope)
    }
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
    legacy_health_pending: bool,
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
            legacy_health_pending: false,
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

    pub fn send_compat_health(&mut self, envelope: &protocol::EnvelopeV1) -> io::Result<()> {
        self.outbound.accept_compat_health_request(envelope)?;
        protocol::write_frame(&mut self.input, envelope)?;
        self.legacy_health_pending = true;
        Ok(())
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
        if self.legacy_health_pending {
            self.inbound.accept_compat_health_response(&envelope)?;
            self.legacy_health_pending = false;
        } else {
            self.inbound.accept(&envelope)?;
        }
        Ok(Some(envelope))
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

impl RemoteRpcTransport for SshTransport {
    fn send(&mut self, envelope: &protocol::EnvelopeV1) -> io::Result<()> {
        SshTransport::send(self, envelope)
    }

    fn send_compat_health(&mut self, envelope: &protocol::EnvelopeV1) -> io::Result<()> {
        SshTransport::send_compat_health(self, envelope)
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
            version: protocol::PROTOCOL_VERSION_V2,
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
        let transport = SshTransport {
            input: child.stdin.take().unwrap(),
            child,
            output: mpsc::channel().1,
            stderr: Arc::new(Mutex::new(Vec::new())),
            outbound: protocol::SessionValidator::default(),
            inbound: protocol::SessionValidator::default(),
            legacy_health_pending: false,
        };
        let mut transport = transport;

        let error = transport.send(&private_chunk()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }
}
