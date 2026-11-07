use super::*;
use crate::computer_use::guest_protocol::{GuestActionReplyV1, frame::read_frame};

impl AuthenticatedGuestChannelV1 {
    pub(super) fn read_action_result(
        &mut self,
        action: &ComputerActionV1,
        request_id: &str,
        capability_id: &str,
        deadline_unix_ms: u64,
    ) -> Result<GuestActionOutcomeV1, GuestProtocolErrorV1> {
        let reply = loop {
            let incoming: AuthenticatedGuestEnvelopeV1 = read_frame(self.channel.io())?;
            incoming.verify(self.channel_key.expose())?;
            match &incoming.envelope.message {
                GuestMessageV1::Heartbeat {
                    boot_id,
                    uptime_millis,
                } => {
                    if boot_id != &self.config.boot_id
                        || incoming.envelope.sender != GuestSenderV1::Guest
                        || incoming.envelope.sequence != self.guest_sequence
                        || incoming.envelope.session_id != self.config.session_id
                        || incoming.envelope.worker_id != self.config.worker_id
                        || incoming.envelope.lease_generation != self.config.lease_generation
                        || incoming.envelope.deadline_unix_ms != self.config.expires_at_unix_ms
                        || incoming.envelope.capability_id != GUEST_CHANNEL_CAPABILITY_V1
                        || incoming.envelope.request_id != self.request_id
                        || incoming.envelope.action_id != self.action_id
                        || *uptime_millis < self.guest_uptime_millis
                    {
                        return Err(GuestProtocolErrorV1::Replay);
                    }
                    self.guest_uptime_millis = *uptime_millis;
                    self.guest_sequence = self.guest_sequence.saturating_add(1);
                }
                GuestMessageV1::ActionReply {
                    action_sequence,
                    result,
                } if *action_sequence == action.action_sequence => {
                    self.validate_incoming(
                        &incoming,
                        request_id,
                        &action.idempotency_key,
                        capability_id,
                        deadline_unix_ms,
                    )?;
                    self.guest_sequence = self.guest_sequence.saturating_add(1);
                    break result.clone();
                }
                GuestMessageV1::GuestExit { reason } => {
                    self.validate_incoming(
                        &incoming,
                        request_id,
                        &action.idempotency_key,
                        capability_id,
                        deadline_unix_ms,
                    )?;
                    self.guest_sequence = self.guest_sequence.saturating_add(1);
                    return Err(GuestProtocolErrorV1::GuestExited(*reason));
                }
                _ => return Err(GuestProtocolErrorV1::Malformed),
            }
        };
        if reply != GuestOperationResultV1::Completed {
            return Ok(GuestActionOutcomeV1 {
                reply: GuestActionReplyV1 {
                    action_sequence: action.action_sequence,
                    result: reply,
                },
                observation: None,
            });
        }
        let requires_observation = matches!(
            &action.payload,
            crate::computer_use::ComputerActionPayloadV1::ObserveScreen
                | crate::computer_use::ComputerActionPayloadV1::MouseClick { .. }
                | crate::computer_use::ComputerActionPayloadV1::KeyPress { .. }
                | crate::computer_use::ComputerActionPayloadV1::TypeText { .. }
                | crate::computer_use::ComputerActionPayloadV1::LaunchApp { .. }
        );
        if !requires_observation {
            return Err(GuestProtocolErrorV1::Malformed);
        }
        let incoming: AuthenticatedGuestEnvelopeV1 = read_frame(self.channel.io())?;
        self.validate_incoming(
            &incoming,
            request_id,
            &action.idempotency_key,
            "computer_use.screen.v1",
            deadline_unix_ms,
        )?;
        let GuestMessageV1::Observation { observation } = &incoming.envelope.message else {
            return Err(GuestProtocolErrorV1::Malformed);
        };
        self.guest_sequence = self.guest_sequence.saturating_add(1);
        let observation = validate_observation(observation)?;
        Ok(GuestActionOutcomeV1 {
            reply: GuestActionReplyV1 {
                action_sequence: action.action_sequence,
                result: GuestOperationResultV1::Completed,
            },
            observation: Some(observation),
        })
    }
}
