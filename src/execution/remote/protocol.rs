use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    io::{self, Read, Write},
};

pub const PROTOCOL_VERSION_V1: u16 = 1;
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
    RelayDependency,
    DependencyRequest,
    DependencyDecision,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeV1 {
    pub version: u16,
    pub request_id: String,
    pub execution_id: String,
    pub sequence: u64,
    pub kind: EnvelopeKindV1,
    pub payload: serde_json::Value,
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
    let bytes = serde_json::to_vec(envelope)
        .map_err(|_| invalid_data("remote worker envelope could not be encoded"))?;
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
    let mut prefix = [0_u8; 4];
    let mut read = 0;
    while read < prefix.len() {
        match input.read(&mut prefix[read..])? {
            0 if read == 0 => return Ok(None),
            0 => return Err(invalid_data("remote worker frame prefix is truncated")),
            count => read += count,
        }
    }
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(invalid_data("remote worker frame exceeds the size limit"));
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    let envelope: EnvelopeV1 = serde_json::from_slice(&bytes)
        .map_err(|_| invalid_data("remote worker frame contains invalid JSON"))?;
    envelope.validate()?;
    Ok(Some(envelope))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn envelope() -> EnvelopeV1 {
        EnvelopeV1 {
            version: PROTOCOL_VERSION_V1,
            request_id: "00000000-0000-4000-8000-000000000001".into(),
            execution_id: "00000000-0000-4000-8000-000000000002".into(),
            sequence: 1,
            kind: EnvelopeKindV1::Probe,
            payload: serde_json::json!({"target": "local"}),
        }
    }

    #[test]
    fn round_trip_uses_bounded_length_prefixed_json() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &envelope()).unwrap();
        let mut input = Cursor::new(bytes);
        assert_eq!(read_frame(&mut input).unwrap(), Some(envelope()));
        assert_eq!(read_frame(&mut input).unwrap(), None);
    }

    #[test]
    fn oversized_and_truncated_frames_are_rejected_before_decode() {
        let mut oversized = Cursor::new(((MAX_FRAME_BYTES as u32) + 1).to_be_bytes());
        assert_eq!(
            read_frame(&mut oversized).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        let mut truncated = Cursor::new([0, 0, 0, 8, b'{', b'}']);
        assert_eq!(
            read_frame(&mut truncated).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn rejects_unknown_fields_and_incompatible_versions() {
        let mut bytes = Vec::new();
        let mut bad = envelope();
        bad.version = 2;
        assert_eq!(
            write_frame(&mut bytes, &bad).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        let json = br#"{"version":1,"request_id":"00000000-0000-4000-8000-000000000001","execution_id":"00000000-0000-4000-8000-000000000002","sequence":1,"kind":"probe","payload":{},"extra":true}"#;
        let mut framed = (json.len() as u32).to_be_bytes().to_vec();
        framed.extend_from_slice(json);
        assert_eq!(
            read_frame(&mut Cursor::new(framed)).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn session_rejects_duplicate_requests_and_sequence_gaps() {
        let mut validator = SessionValidator::default();
        let first = envelope();
        validator.accept(&first).unwrap();
        assert_eq!(
            validator.accept(&first).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );

        let mut next = envelope();
        next.request_id = "00000000-0000-4000-8000-000000000003".into();
        next.sequence = 3;
        assert_eq!(
            validator.accept(&next).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        next.sequence = 2;
        validator.accept(&next).unwrap();
    }

    #[test]
    fn new_transport_session_can_resume_at_a_persisted_sequence() {
        let mut validator = SessionValidator::default();
        let mut resumed = envelope();
        resumed.sequence = 17;
        validator.accept(&resumed).unwrap();
        resumed.request_id = "00000000-0000-4000-8000-000000000003".into();
        resumed.sequence = 18;
        validator.accept(&resumed).unwrap();
        resumed.sequence = 20;
        resumed.request_id = "00000000-0000-4000-8000-000000000004".into();
        assert_eq!(
            validator.accept(&resumed).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
