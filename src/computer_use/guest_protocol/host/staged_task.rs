use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use std::time::Duration;

use crate::computer_use::{
    ComputerActionActorV1, ComputerActionPayloadV1, ComputerActionV1,
    apps::StagedTaskArtifactContentV1,
};

use super::super::{
    AuthenticatedGuestChannelV1, GuestActionJournalV1, GuestActionOutcomeV1, GuestActionReplyV1,
    GuestMessageV1, GuestOperationResultV1, GuestProtocolErrorV1, GuestReplayDecisionV1,
    GuestSenderV1,
    auth::AuthenticatedGuestEnvelopeV1,
    frame::{read_frame, write_frame},
    types::{GuestEnvelopeContextV1, required_capability},
};
use super::validation::request_id_for_action;

const STAGE_CHUNK_BYTES: usize = 128 * 1024;

impl AuthenticatedGuestChannelV1 {
    /// Transfer exact P15 receipt bytes over the authenticated virtio channel,
    /// then launch through the separately fenced guest helper operation.
    #[allow(clippy::too_many_arguments)] // Transport, authority, payload, and fences remain explicit.
    pub(crate) fn execute_staged_task_application(
        &mut self,
        session: &crate::computer_use::ComputerSessionV1,
        actor: ComputerActionActorV1,
        action: &ComputerActionV1,
        contents: &[StagedTaskArtifactContentV1],
        deadline_unix_ms: u64,
        now_unix_ms: u64,
        journal: &GuestActionJournalV1,
    ) -> Result<GuestActionOutcomeV1, GuestProtocolErrorV1> {
        action
            .validate_as(actor, session, now_unix_ms)
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        let ComputerActionPayloadV1::LaunchApprovedStagedTaskApp { application } = &action.payload
        else {
            return Err(GuestProtocolErrorV1::Malformed);
        };
        if actor != ComputerActionActorV1::Human
            || !self.capabilities.staged_task_app_launch
            || !application.validate_contents(contents).is_ok()
            || application.guest_image_digest != session.image_digest
        {
            return Err(GuestProtocolErrorV1::UnsupportedCapability);
        }
        let capability_id = required_capability(action);
        if !self.supports(capability_id) {
            return Err(GuestProtocolErrorV1::UnsupportedCapability);
        }
        if deadline_unix_ms <= now_unix_ms
            || deadline_unix_ms > session.lease.expires_at_unix_ms
            || deadline_unix_ms
                > session
                    .session_started_at_unix_ms
                    .saturating_add(session.policy.resource_budget.time_limit_ms)
            || deadline_unix_ms.saturating_sub(now_unix_ms) > 60_000
        {
            return Err(GuestProtocolErrorV1::DeadlineExpired);
        }
        let request_id = request_id_for_action(session, action);
        match journal
            .begin(&request_id, action)
            .map_err(GuestProtocolErrorV1::from)?
        {
            GuestReplayDecisionV1::Replay(outcome) => return Ok(outcome),
            GuestReplayDecisionV1::Execute => {}
        }
        let timeout = Duration::from_millis(deadline_unix_ms.saturating_sub(now_unix_ms));
        self.channel
            .io()
            .set_read_timeout(Some(timeout))
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
        self.channel
            .io()
            .set_write_timeout(Some(timeout))
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;

        self.write_stage_message(
            &request_id,
            action,
            deadline_unix_ms,
            GuestMessageV1::StageTaskApplicationStart {
                action: action.clone(),
            },
        )?;
        for (artifact_index, content) in contents.iter().enumerate() {
            for (chunk_index, bytes) in content.bytes.chunks(STAGE_CHUNK_BYTES).enumerate() {
                let offset = (chunk_index * STAGE_CHUNK_BYTES) as u64;
                self.write_stage_message(
                    &request_id,
                    action,
                    deadline_unix_ms,
                    GuestMessageV1::StageTaskArtifactChunk {
                        artifact_index: artifact_index as u16,
                        offset,
                        bytes_base64: BASE64.encode(bytes),
                    },
                )?;
            }
        }
        self.write_stage_message(
            &request_id,
            action,
            deadline_unix_ms,
            GuestMessageV1::StageTaskApplicationFinish,
        )?;
        let transfer_reply: AuthenticatedGuestEnvelopeV1 =
            read_frame(self.channel.io()).map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
        self.validate_incoming(
            &transfer_reply,
            &request_id,
            &action.idempotency_key,
            capability_id,
            deadline_unix_ms,
        )?;
        self.guest_sequence = self.guest_sequence.saturating_add(1);
        if !matches!(
            transfer_reply.envelope.message,
            GuestMessageV1::StageTaskTransferReply { accepted: true }
        ) {
            let outcome = rejected_outcome(action);
            journal
                .complete(&request_id, action, outcome.clone())
                .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
            return Ok(outcome);
        }
        self.write_stage_message(
            &request_id,
            action,
            deadline_unix_ms,
            GuestMessageV1::ActionRequest {
                action: action.clone(),
            },
        )?;
        let outcome = self
            .read_action_result(action, &request_id, capability_id, deadline_unix_ms)
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
        journal
            .complete(&request_id, action, outcome.clone())
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
        Ok(outcome)
    }

    fn write_stage_message(
        &mut self,
        request_id: &str,
        action: &ComputerActionV1,
        deadline_unix_ms: u64,
        message: GuestMessageV1,
    ) -> Result<(), GuestProtocolErrorV1> {
        let envelope = self.envelope(
            GuestEnvelopeContextV1 {
                sender: GuestSenderV1::Host,
                sequence: self.host_sequence,
                request_id: request_id.to_owned(),
                action_id: action.idempotency_key.clone(),
                deadline_unix_ms,
                capability_id: required_capability(action).to_owned(),
            },
            message,
        );
        write_frame(
            self.channel.io(),
            &AuthenticatedGuestEnvelopeV1::sign(envelope, self.channel_key.expose())?,
        )
        .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
        self.host_sequence = self.host_sequence.saturating_add(1);
        Ok(())
    }
}

fn rejected_outcome(action: &ComputerActionV1) -> GuestActionOutcomeV1 {
    GuestActionOutcomeV1 {
        reply: GuestActionReplyV1 {
            action_sequence: action.action_sequence,
            result: GuestOperationResultV1::Rejected,
        },
        observation: None,
    }
}
