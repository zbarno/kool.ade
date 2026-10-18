use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io::{self, Read, Write},
};
use zeroize::Zeroizing;

mod private_chunks;
mod sensitive;
pub(crate) use private_chunks::{
    WorkerInboundFrameV1, read_worker_frame, write_private_file_chunk_frame,
};

pub const PROTOCOL_VERSION_V1: u16 = 1;
/// Reserved execution identity for the key-bootstrap handshake on each SSH session.
pub const SYSTEM_IMAGE_SESSION_ID_V1: &str = "00000000-0000-4000-8000-000000000001";
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_SESSION_MESSAGES: usize = 16_384;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeKindV1 {
    Probe,
    ProbeResult,
    Heartbeat,
    PrepareWorkspace,
    BeginWorkspaceTransfer,
    WorkspaceTransferChunk,
    FinalizeWorkspaceTransfer,
    BeginCacheTransfer,
    CacheTransferChunk,
    FinalizeCacheTransfer,
    InspectSystemPackages,
    BeginSystemImageTransfer,
    SystemImageTransferChunk,
    FinalizeSystemImageTransfer,
    RelayDependency,
    DependencyRequest,
    DependencyDecision,
    AuthorizePrivateFileGrant,
    AbortPrivateFileStage,
    /// Secret-bearing frame that must never enter a durable RPC journal.
    PrivateFileChunk,
    PrepareWorkspaceResult,
    ReadWorkspaceResultChunk,
    Start,
    Exec,
    Subscribe,
    Event,
    Cancel,
    CollectArtifacts,
    ReleaseWorkspace,
    Release,
    Health,
    Acknowledge,
    Error,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeV1 {
    pub version: u16,
    pub request_id: String,
    pub execution_id: String,
    pub sequence: u64,
    pub kind: EnvelopeKindV1,
    pub payload: serde_json::Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnvelopeV1<'a> {
    version: u16,
    request_id: String,
    execution_id: String,
    sequence: u64,
    kind: EnvelopeKindV1,
    #[serde(borrow)]
    payload: &'a serde_json::value::RawValue,
}

impl std::fmt::Debug for EnvelopeV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("EnvelopeV1");
        debug
            .field("version", &self.version)
            .field("request_id", &self.request_id)
            .field("execution_id", &self.execution_id)
            .field("sequence", &self.sequence)
            .field("kind", &self.kind);
        if self.kind == EnvelopeKindV1::PrivateFileChunk {
            debug.field("payload", &"<redacted>");
        } else {
            debug.field("payload", &self.payload);
        }
        debug.finish()
    }
}

impl EnvelopeV1 {
    /// Clear decoded private bytes before the request leaves the transport
    /// stack. Private chunks are encoded as Base64 strings.
    pub(crate) fn clear_sensitive_payload(&mut self) {
        if self.kind != EnvelopeKindV1::PrivateFileChunk {
            return;
        }
        clear_sensitive_payload_value(&mut self.payload);
    }
}

/// Erase secret-bearing JSON values when a private chunk is denied before it
/// reaches the dedicated, grant-authorized transport path.
pub(crate) fn clear_sensitive_payload_value(payload: &mut serde_json::Value) {
    sensitive::clear_payload(payload);
}

/// Rejects replayed request IDs and sequence gaps within one transport session.
/// The first sequence may be non-one because a reconnect can resume an exact
/// journaled request; subsequent frames must be contiguous.
#[derive(Default)]
pub struct SessionValidator {
    request_ids: HashSet<String>,
    last_sequence: HashMap<String, u64>,
}

impl SessionValidator {
    pub fn accept(&mut self, envelope: &EnvelopeV1) -> io::Result<()> {
        envelope.validate()?;
        if self.request_ids.len() >= MAX_SESSION_MESSAGES {
            return Err(invalid_data("remote worker session message limit exceeded"));
        }
        if !self.request_ids.insert(envelope.request_id.clone()) {
            return Err(invalid_data("remote worker request ID was replayed"));
        }
        if let Some(last) = self.last_sequence.get(&envelope.execution_id)
            && envelope.sequence != last.saturating_add(1)
        {
            self.request_ids.remove(&envelope.request_id);
            return Err(invalid_data(
                "remote worker message sequence is out of order",
            ));
        }
        self.last_sequence
            .insert(envelope.execution_id.clone(), envelope.sequence);
        Ok(())
    }
}

impl EnvelopeV1 {
    pub fn validate(&self) -> io::Result<()> {
        if self.version != PROTOCOL_VERSION_V1
            || self.sequence == 0
            || uuid::Uuid::parse_str(&self.request_id)
                .map_or(true, |id| id.hyphenated().to_string() != self.request_id)
            || uuid::Uuid::parse_str(&self.execution_id)
                .map_or(true, |id| id.hyphenated().to_string() != self.execution_id)
        {
            return Err(invalid_data("remote worker envelope identity is invalid"));
        }
        if self.request_id.len() > 64 || self.execution_id.len() > 64 {
            return Err(invalid_data("remote worker envelope identity is too long"));
        }
        Ok(())
    }
}

/// Writes one UTF-8 JSON envelope with a big-endian length prefix.
pub fn write_frame(mut output: impl Write, envelope: &EnvelopeV1) -> io::Result<()> {
    envelope.validate()?;
    if envelope.kind == EnvelopeKindV1::PrivateFileChunk {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private file chunks require a dedicated authorized transport",
        ));
    }
    let bytes = Zeroizing::new(
        serde_json::to_vec(envelope)
            .map_err(|_| invalid_data("remote worker envelope could not be encoded"))?,
    );
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
        return Err(invalid_data("remote worker frame exceeds the size limit"));
    }
    let length = u32::try_from(bytes.len())
        .map_err(|_| invalid_data("remote worker frame length is invalid"))?;
    output.write_all(&length.to_be_bytes())?;
    output.write_all(&bytes)?;
    output.flush()
}

/// Reads one bounded frame. EOF before the prefix is clean; partial frames fail.
pub fn read_frame(input: &mut impl Read) -> io::Result<Option<EnvelopeV1>> {
    match read_worker_frame(input)? {
        None => Ok(None),
        Some(WorkerInboundFrameV1::Envelope(envelope)) => Ok(Some(envelope)),
        Some(WorkerInboundFrameV1::PrivateFileChunk { mut envelope, .. }) => {
            envelope.clear_sensitive_payload();
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "private file chunks require the worker's typed chunk handler",
            ))
        }
    }
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests;
