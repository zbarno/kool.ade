use crate::computer_use::guest_protocol::{
    AuthenticatedGuestEnvelopeV1, GuestActionOutcomeV1, GuestBootConfigV1, GuestEnvelopeContextV1,
    GuestEnvelopeV1, GuestMessageV1, GuestProtocolErrorV1, GuestSenderV1,
    auth::GuestHandshakeCompleteV1, frame::write_frame,
};
use std::io::Write;

pub(super) fn write_outcome(
    port: &mut impl Write,
    config: &GuestBootConfigV1,
    handshake: &GuestHandshakeCompleteV1,
    mut sequence: u64,
    request: &GuestEnvelopeV1,
    outcome: &GuestActionOutcomeV1,
) -> Result<u64, GuestProtocolErrorV1> {
    let reply = envelope(
        config,
        GuestEnvelopeContextV1 {
            sender: GuestSenderV1::Guest,
            sequence,
            request_id: request.request_id.clone(),
            action_id: request.action_id.clone(),
            deadline_unix_ms: request.deadline_unix_ms,
            capability_id: request.capability_id.clone(),
        },
        GuestMessageV1::ActionReply {
            action_sequence: outcome.reply.action_sequence,
            result: outcome.reply.result.clone(),
        },
    );
    write_frame(
        port,
        &AuthenticatedGuestEnvelopeV1::sign(reply, handshake.channel_key.expose())?,
    )?;
    sequence = sequence.saturating_add(1);
    if let Some(observation) = &outcome.observation {
        let frame = envelope(
            config,
            GuestEnvelopeContextV1 {
                sender: GuestSenderV1::Guest,
                sequence,
                request_id: request.request_id.clone(),
                action_id: request.action_id.clone(),
                deadline_unix_ms: request.deadline_unix_ms,
                capability_id: "computer_use.screen.v1".to_owned(),
            },
            GuestMessageV1::Observation {
                observation: observation.clone(),
            },
        );
        write_frame(
            port,
            &AuthenticatedGuestEnvelopeV1::sign(frame, handshake.channel_key.expose())?,
        )?;
        sequence = sequence.saturating_add(1);
    }
    Ok(sequence)
}

pub(super) fn envelope(
    config: &GuestBootConfigV1,
    context: GuestEnvelopeContextV1,
    message: GuestMessageV1,
) -> GuestEnvelopeV1 {
    GuestEnvelopeV1 {
        protocol_version: crate::computer_use::guest_protocol::types::GUEST_PROTOCOL_VERSION_V1,
        sender: context.sender,
        session_id: config.session_id.clone(),
        worker_id: config.worker_id.clone(),
        lease_generation: config.lease_generation,
        sequence: context.sequence,
        request_id: context.request_id,
        action_id: context.action_id,
        deadline_unix_ms: context.deadline_unix_ms,
        capability_id: context.capability_id,
        message,
    }
}
