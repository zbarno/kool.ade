use std::io::{self, Read, Write};

use crate::execution::remote::ssh::SshWorkerConfig;

use super::{
    ComputerRemoteWorkerRequestV1, ComputerRemoteWorkerResponseV1,
    VerifiedComputerRemoteWorkerIdentityV1, decode_remote_computer_worker_frame_v1,
    encode_remote_computer_worker_frame_v1,
};

/// Pinned-SSH request stream for one worker. Only the fixed Computer Use
/// command can be started, and requests/results are sequence fenced.
pub struct ComputerRemoteWorkerClientV1 {
    child: Option<std::process::Child>,
    stream: Option<Box<dyn RemoteWorkerIoV1>>,
    identity: VerifiedComputerRemoteWorkerIdentityV1,
    next_transport_sequence: u64,
}

trait RemoteWorkerIoV1: Read + Write + Send {}
impl<T: Read + Write + Send> RemoteWorkerIoV1 for T {}

struct ProcessIo {
    input: std::process::ChildStdin,
    output: std::process::ChildStdout,
}

impl Read for ProcessIo {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.output.read(buffer)
    }
}

impl Write for ProcessIo {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.input.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.input.flush()
    }
}

impl Drop for ComputerRemoteWorkerClientV1 {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut()
            && child.try_wait().ok().flatten().is_none()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl ComputerRemoteWorkerClientV1 {
    pub fn connect(
        config: &SshWorkerConfig,
        expected_worker_id: &str,
        expected_host_key_fingerprint: &str,
        remote_config_path: &std::path::Path,
    ) -> io::Result<Self> {
        Self::connect_with_sequence(
            config,
            expected_worker_id,
            expected_host_key_fingerprint,
            remote_config_path,
            1,
        )
    }

    /// Open a fresh pinned SSH channel for a signed, generation-advancing
    /// recovery request. Sequence zero is reserved for that one request; the
    /// worker resets the new transport stream to sequence one after accepting
    /// the recovery fence.
    pub fn connect_for_recovery(
        config: &SshWorkerConfig,
        expected_worker_id: &str,
        expected_host_key_fingerprint: &str,
        remote_config_path: &std::path::Path,
    ) -> io::Result<Self> {
        Self::connect_with_sequence(
            config,
            expected_worker_id,
            expected_host_key_fingerprint,
            remote_config_path,
            0,
        )
    }

    fn connect_with_sequence(
        config: &SshWorkerConfig,
        expected_worker_id: &str,
        expected_host_key_fingerprint: &str,
        remote_config_path: &std::path::Path,
        first_sequence: u64,
    ) -> io::Result<Self> {
        let worker = config
            .verify_computer_use_known_host(expected_worker_id, expected_host_key_fingerprint)
            .map_err(|_| invalid_data("SSH worker identity verification failed"))?;
        let mut child = worker.spawn_computer_use(remote_config_path)?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("worker stdin unavailable"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("worker stdout unavailable"))?;
        Ok(Self {
            child: Some(child),
            stream: Some(Box::new(ProcessIo { input, output })),
            identity: VerifiedComputerRemoteWorkerIdentityV1::from_ssh_worker(&worker),
            next_transport_sequence: first_sequence,
        })
    }

    #[cfg(test)]
    pub(crate) fn from_test_stream(
        stream: impl Read + Write + Send + 'static,
        identity: VerifiedComputerRemoteWorkerIdentityV1,
        first_sequence: u64,
    ) -> Self {
        Self {
            child: None,
            stream: Some(Box::new(stream)),
            identity,
            next_transport_sequence: first_sequence,
        }
    }

    pub fn identity(&self) -> &VerifiedComputerRemoteWorkerIdentityV1 {
        &self.identity
    }

    pub fn next_transport_sequence(&self) -> u64 {
        self.next_transport_sequence
    }

    pub fn request(
        &mut self,
        request: &ComputerRemoteWorkerRequestV1,
    ) -> io::Result<ComputerRemoteWorkerResponseV1> {
        request.validate_shape().map_err(invalid_data)?;
        if request.worker_id != self.identity.worker_id()
            || request.ssh_host_key_fingerprint != self.identity.host_key_fingerprint()
            || request.transport_sequence != self.next_transport_sequence
        {
            return Err(invalid_data(
                "request is outside the verified worker sequence",
            ));
        }
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "worker stream is closed"))?;
        stream.write_all(&encode_remote_computer_worker_frame_v1(request)?)?;
        stream.flush()?;
        let response = read_response(stream.as_mut())?;
        response.validate().map_err(invalid_data)?;
        if response.worker_id != request.worker_id
            || response.session_id != request.session_id
            || response.transport_sequence != request.transport_sequence
            || response.protocol_version != request.protocol_version
        {
            return Err(invalid_data("response does not match the request"));
        }
        self.next_transport_sequence = self
            .next_transport_sequence
            .checked_add(1)
            .ok_or_else(|| invalid_data("transport sequence exhausted"))?;
        Ok(response)
    }

    pub fn close(mut self) -> io::Result<std::process::ExitStatus> {
        drop(self.stream.take());
        self.child
            .take()
            .ok_or_else(|| io::Error::other("in-memory worker stream has no child process"))?
            .wait()
    }
}

fn read_response<R: Read + ?Sized>(input: &mut R) -> io::Result<ComputerRemoteWorkerResponseV1> {
    let mut prefix = [0; 4];
    input.read_exact(&mut prefix)?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > super::MAX_REMOTE_COMPUTER_WORKER_FRAME_BYTES_V1 {
        return Err(invalid_data("worker response exceeds the frame limit"));
    }
    let mut frame = Vec::with_capacity(length + 4);
    frame.extend_from_slice(&prefix);
    frame.resize(length + 4, 0);
    input.read_exact(&mut frame[4..])?;
    decode_remote_computer_worker_frame_v1(&frame)
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
