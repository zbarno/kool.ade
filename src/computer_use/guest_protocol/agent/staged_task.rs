use crate::computer_use::{
    ComputerActionPayloadV1,
    apps::guest_helper::StagedTaskClientV1,
    guest_protocol::{
        AuthenticatedGuestEnvelopeV1, GuestBootConfigV1, GuestMessageV1, GuestProtocolErrorV1,
        GuestSenderV1,
        auth::GuestHandshakeCompleteV1,
        frame::{read_frame, write_frame},
    },
};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use std::{
    io::{Read, Write},
    os::fd::AsRawFd,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_CHUNK_BYTES: usize = 128 * 1024;

pub(super) fn receive_transfer(
    port: &mut (impl Read + Write + AsRawFd),
    config: &GuestBootConfigV1,
    handshake: &GuestHandshakeCompleteV1,
    start: &crate::computer_use::guest_protocol::GuestEnvelopeV1,
    action: &crate::computer_use::ComputerActionV1,
    mut host_sequence: u64,
    mut guest_sequence: u64,
) -> Result<(u64, u64), GuestProtocolErrorV1> {
    let ComputerActionPayloadV1::LaunchApprovedStagedTaskApp { application } = &action.payload
    else {
        return Err(GuestProtocolErrorV1::Malformed);
    };
    let mut accepted = application.validate_at(unix_time_ms()).is_ok()
        && config.allowed_apps.contains(&application.app_id);
    let mut stage = accepted
        .then(|| StagedTaskClientV1::begin(action).ok())
        .flatten();
    accepted &= stage.is_some();
    let mut offsets = if accepted {
        vec![0_u64; application.artifacts.len()]
    } else {
        Vec::new()
    };
    loop {
        let incoming: AuthenticatedGuestEnvelopeV1 = read_frame(port)?;
        incoming.verify(handshake.channel_key.expose())?;
        let envelope = &incoming.envelope;
        if envelope.sender != GuestSenderV1::Host
            || envelope.protocol_version
                != crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1
            || envelope.session_id != config.session_id
            || envelope.worker_id != config.worker_id
            || envelope.lease_generation != config.lease_generation
            || envelope.sequence != host_sequence
            || envelope.request_id != start.request_id
            || envelope.action_id != start.action_id
            || envelope.capability_id != start.capability_id
            || envelope.deadline_unix_ms != start.deadline_unix_ms
        {
            return Err(GuestProtocolErrorV1::Replay);
        }
        host_sequence = host_sequence.saturating_add(1);
        match &envelope.message {
            GuestMessageV1::StageTaskArtifactChunk {
                artifact_index,
                offset,
                bytes_base64,
            } => {
                if bytes_base64.len() > MAX_CHUNK_BYTES.div_ceil(3) * 4 {
                    return Err(GuestProtocolErrorV1::Oversized);
                }
                let bytes = BASE64
                    .decode(bytes_base64)
                    .map_err(|_| GuestProtocolErrorV1::Malformed)?;
                let byte_count = bytes.len() as u64;
                if bytes.is_empty() || bytes.len() > MAX_CHUNK_BYTES {
                    return Err(GuestProtocolErrorV1::Malformed);
                }
                if accepted {
                    let index = *artifact_index as usize;
                    let valid = application.artifacts.get(index).is_some_and(|expected| {
                        offsets[index] == *offset
                            && offset.saturating_add(bytes.len() as u64)
                                <= expected.receipt.size_bytes
                    });
                    if !valid
                        || stage.as_mut().is_none_or(|client| {
                            client.write_chunk(*artifact_index, *offset, bytes).is_err()
                        })
                    {
                        accepted = false;
                        stage.take();
                        offsets.clear();
                    } else {
                        offsets[index] = offset.saturating_add(byte_count);
                    }
                }
            }
            GuestMessageV1::StageTaskApplicationFinish => {
                let all_bytes_received = offsets.len() == application.artifacts.len()
                    && offsets
                        .iter()
                        .zip(&application.artifacts)
                        .all(|(offset, artifact)| *offset == artifact.receipt.size_bytes);
                accepted &= all_bytes_received;
                if accepted {
                    accepted = stage.take().is_some_and(|client| client.finish().is_ok());
                }
                let sequence =
                    send_reply(port, config, handshake, start, guest_sequence, accepted)?;
                guest_sequence = sequence;
                return Ok((host_sequence, guest_sequence));
            }
            _ => return Err(GuestProtocolErrorV1::Malformed),
        }
    }
}

fn send_reply(
    port: &mut impl Write,
    config: &GuestBootConfigV1,
    handshake: &GuestHandshakeCompleteV1,
    start: &crate::computer_use::guest_protocol::GuestEnvelopeV1,
    sequence: u64,
    accepted: bool,
) -> Result<u64, GuestProtocolErrorV1> {
    let reply = crate::computer_use::guest_protocol::GuestEnvelopeV1 {
        protocol_version: crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
        sender: GuestSenderV1::Guest,
        session_id: config.session_id.clone(),
        worker_id: config.worker_id.clone(),
        lease_generation: config.lease_generation,
        sequence,
        request_id: start.request_id.clone(),
        action_id: start.action_id.clone(),
        deadline_unix_ms: start.deadline_unix_ms,
        capability_id: start.capability_id.clone(),
        message: GuestMessageV1::StageTaskTransferReply { accepted },
    };
    write_frame(
        port,
        &AuthenticatedGuestEnvelopeV1::sign(reply, handshake.channel_key.expose())?,
    )?;
    Ok(sequence.saturating_add(1))
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
