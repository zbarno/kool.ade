use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    time::Duration,
};

/// Host-side byte stream to one app-owned guest serial port. Its I/O methods
/// stay crate-private so task/provider code cannot bypass the typed protocol.
pub struct ComputerGuestChannelV1 {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) session_id: String,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) lease_generation: u64,
    _stream: Box<dyn GuestChannelIo>,
}

pub(crate) trait GuestChannelIo: Read + Write + Send {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
}

#[cfg(unix)]
impl GuestChannelIo for std::os::unix::net::UnixStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_read_timeout(self, timeout)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        std::os::unix::net::UnixStream::set_write_timeout(self, timeout)
    }
}

/// Token minted only by the trusted guest protocol verifier after it has
/// authenticated a fresh per-boot transcript and matched the active lease.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerGuestReadinessV1 {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) session_id: String,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) lease_generation: u64,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) guest_boot_id: String,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) transcript_sha256: String,
}

impl ComputerGuestReadinessV1 {
    #[allow(dead_code)] // Used by the authenticated guest transport implemented in the dependent issue.
    pub(crate) fn new(
        session_id: String,
        lease_generation: u64,
        guest_boot_id: String,
        transcript_sha256: String,
    ) -> Self {
        Self {
            session_id,
            lease_generation,
            guest_boot_id,
            transcript_sha256,
        }
    }
}

impl ComputerGuestChannelV1 {
    #[cfg(unix)]
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn new(
        session_id: String,
        lease_generation: u64,
        stream: std::os::unix::net::UnixStream,
    ) -> Self {
        Self::from_io(session_id, lease_generation, stream)
    }

    /// Build a channel over a platform-owned authenticated byte stream.
    /// Windows adapters can pass a timeout-enforcing stream wrapper around an
    /// authenticated per-user named pipe without exposing pipe names or raw
    /// I/O to task and provider code.
    #[allow(dead_code)]
    pub(crate) fn from_io(
        session_id: String,
        lease_generation: u64,
        stream: impl GuestChannelIo + 'static,
    ) -> Self {
        Self {
            session_id,
            lease_generation,
            _stream: Box::new(stream),
        }
    }

    #[allow(dead_code)] // The typed guest protocol owns all reads and writes on this channel.
    pub(crate) fn io(&mut self) -> &mut (dyn GuestChannelIo + 'static) {
        self._stream.as_mut()
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn lease_generation(&self) -> u64 {
        self.lease_generation
    }
}
