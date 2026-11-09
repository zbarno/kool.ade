use std::time::Duration;
mod actions;
mod authorized;

pub use authorized::{
    AuthorizedComputerUseSessionV1, ComputerUseAuthorityRegistryV1, ComputerUsePermissionV1,
};

use crate::computer_use::{
    ComputerActionActorV1, ComputerActionV1, ComputerGuestChannelV1, ComputerGuestReadinessV1,
    ComputerSessionV1,
};

#[path = "host/validation.rs"]
mod validation;
use super::{
    GuestActionJournalV1, GuestActionOutcomeV1, GuestBootConfigV1, GuestCapabilitySetV1,
    GuestChannelKeyV1, GuestMessageV1, GuestObservationV1, GuestOperationResultV1,
    GuestProtocolErrorV1, GuestReplayDecisionV1, GuestSenderV1,
    auth::{AuthenticatedGuestEnvelopeV1, GuestHandshakeCompleteV1, host_handshake},
    frame::write_frame,
    types::{GUEST_CHANNEL_CAPABILITY_V1, GuestEnvelopeContextV1, required_capability},
};
use validation::{readiness_token, request_id_for_action, validate_observation};

pub struct GuestHostTransportV1;

pub struct AuthenticatedGuestChannelV1 {
    channel: ComputerGuestChannelV1,
    config: GuestBootConfigV1,
    channel_key: GuestChannelKeyV1,
    capabilities: GuestCapabilitySetV1,
    transcript_sha256: String,
    request_id: String,
    action_id: String,
    host_sequence: u64,
    guest_sequence: u64,
    guest_uptime_millis: u64,
    latest_observation: Option<GuestObservationV1>,
}

impl GuestHostTransportV1 {
    pub fn authenticate(
        mut channel: ComputerGuestChannelV1,
        config: GuestBootConfigV1,
        session: &ComputerSessionV1,
        now_unix_ms: u64,
    ) -> Result<(AuthenticatedGuestChannelV1, ComputerGuestReadinessV1), GuestProtocolErrorV1> {
        if channel.session_id() != config.session_id
            || channel.lease_generation() != config.lease_generation
        {
            return Err(GuestProtocolErrorV1::WrongSession);
        }
        let completed = host_handshake(channel.io(), &config, session, now_unix_ms)?;
        let readiness = readiness_token(session, &completed);
        Ok((
            AuthenticatedGuestChannelV1::from_handshake(channel, config, completed),
            readiness,
        ))
    }
}

impl AuthenticatedGuestChannelV1 {
    fn from_handshake(
        channel: ComputerGuestChannelV1,
        config: GuestBootConfigV1,
        completed: GuestHandshakeCompleteV1,
    ) -> Self {
        Self {
            channel,
            config,
            channel_key: completed.channel_key,
            capabilities: completed.capabilities,
            transcript_sha256: completed.transcript_sha256,
            request_id: completed.request_id,
            action_id: completed.action_id,
            host_sequence: completed.host_sequence,
            guest_sequence: completed.guest_sequence,
            guest_uptime_millis: 0,
            latest_observation: None,
        }
    }

    pub fn capabilities(&self) -> &GuestCapabilitySetV1 {
        &self.capabilities
    }

    pub fn transcript_sha256(&self) -> &str {
        &self.transcript_sha256
    }

    /// The journal write is durable before an action reaches the guest. After
    /// an ambiguous timeout or reconnect, a pending action returns
    /// `OutcomeUnknown` and is never sent a second time.
    pub(crate) fn execute_action(
        &mut self,
        session: &ComputerSessionV1,
        actor: ComputerActionActorV1,
        action: &ComputerActionV1,
        deadline_unix_ms: u64,
        now_unix_ms: u64,
        journal: &GuestActionJournalV1,
    ) -> Result<GuestActionOutcomeV1, GuestProtocolErrorV1> {
        action
            .validate_as(actor, session, now_unix_ms)
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        self.validate_observation_reference(action)?;
        let capability_id = required_capability(action);
        if !self.supports(capability_id) {
            return Err(GuestProtocolErrorV1::UnsupportedCapability);
        }
        if let crate::computer_use::ComputerActionPayloadV1::LaunchApp { app_id } = &action.payload
            && !self.capabilities.app_registry.contains(app_id)
        {
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

        let envelope = self.envelope(
            GuestEnvelopeContextV1 {
                sender: GuestSenderV1::Host,
                sequence: self.host_sequence,
                request_id: request_id.clone(),
                action_id: action.idempotency_key.clone(),
                deadline_unix_ms,
                capability_id: capability_id.to_owned(),
            },
            GuestMessageV1::ActionRequest {
                action: action.clone(),
            },
        );
        write_frame(
            self.channel.io(),
            &AuthenticatedGuestEnvelopeV1::sign(envelope, self.channel_key.expose())?,
        )
        .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
        self.host_sequence = self.host_sequence.saturating_add(1);
        let result = self.read_action_result(action, &request_id, capability_id, deadline_unix_ms);
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error @ GuestProtocolErrorV1::GuestExited(_)) => return Err(error),
            Err(_) => return Err(GuestProtocolErrorV1::OutcomeUnknown),
        };
        journal
            .complete(&request_id, action, outcome.clone())
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
        Ok(outcome)
    }

    fn validate_incoming(
        &self,
        incoming: &AuthenticatedGuestEnvelopeV1,
        request_id: &str,
        action_id: &str,
        capability_id: &str,
        deadline_unix_ms: u64,
    ) -> Result<(), GuestProtocolErrorV1> {
        incoming.verify(self.channel_key.expose())?;
        let envelope = &incoming.envelope;
        if envelope.sender != GuestSenderV1::Guest
            || envelope.sequence != self.guest_sequence
            || envelope.session_id != self.config.session_id
            || envelope.worker_id != self.config.worker_id
            || envelope.lease_generation != self.config.lease_generation
            || envelope.request_id != request_id
            || envelope.action_id != action_id
            || envelope.capability_id != capability_id
            || envelope.deadline_unix_ms != deadline_unix_ms
        {
            return Err(GuestProtocolErrorV1::Replay);
        }
        Ok(())
    }

    fn envelope(
        &self,
        context: GuestEnvelopeContextV1,
        message: GuestMessageV1,
    ) -> crate::computer_use::guest_protocol::GuestEnvelopeV1 {
        crate::computer_use::guest_protocol::GuestEnvelopeV1 {
            protocol_version: super::types::GUEST_PROTOCOL_VERSION_V1,
            sender: context.sender,
            session_id: self.config.session_id.clone(),
            worker_id: self.config.worker_id.clone(),
            lease_generation: self.config.lease_generation,
            sequence: context.sequence,
            request_id: context.request_id,
            action_id: context.action_id,
            deadline_unix_ms: context.deadline_unix_ms,
            capability_id: context.capability_id,
            message,
        }
    }

    fn supports(&self, capability_id: &str) -> bool {
        match capability_id {
            "computer_use.screen.v1" => self.capabilities.screen_capture,
            "computer_use.pointer.v1" => self.capabilities.pointer_input,
            "computer_use.keyboard.v1" => self.capabilities.keyboard_input,
            "computer_use.accessibility.v1" => self.capabilities.accessibility_tree,
            "computer_use.app_registry.v1" => !self.capabilities.app_registry.is_empty(),
            "computer_use.state.v1" => self.capabilities.screen_capture,
            _ => false,
        }
    }

    fn validate_observation_reference(
        &self,
        action: &ComputerActionV1,
    ) -> Result<(), GuestProtocolErrorV1> {
        use crate::computer_use::ComputerActionPayloadV1 as P;
        let observation = match &self.latest_observation {
            Some(observation) => observation,
            None if matches!(
                &action.payload,
                P::MouseClick { .. }
                    | P::MouseDoubleClick { .. }
                    | P::MouseMove { .. }
                    | P::MouseDrag { .. }
                    | P::MouseScroll { .. }
                    | P::FocusWindow { .. }
                    | P::KeyPress { .. }
                    | P::KeyChord { .. }
                    | P::TypeText { .. }
            ) =>
            {
                return Err(GuestProtocolErrorV1::StaleObservation);
            }
            None => return Ok(()),
        };
        let matches_window = |window_id: &str, revision: u64| {
            revision == observation.observation_revision
                && observation
                    .windows
                    .iter()
                    .any(|window| window.window_id == window_id)
        };
        match &action.payload {
            P::MouseClick { target, .. }
            | P::MouseDoubleClick { target, .. }
            | P::MouseMove { target, .. }
            | P::MouseDrag { target, .. }
            | P::MouseScroll { target, .. }
                if !observation.matches_frame_reference(target) =>
            {
                Err(GuestProtocolErrorV1::StaleObservation)
            }
            P::FocusWindow {
                window_id,
                observation_revision,
            }
            | P::KeyPress {
                window_id,
                observation_revision,
                ..
            }
            | P::KeyChord {
                window_id,
                observation_revision,
                ..
            }
            | P::TypeText {
                window_id,
                observation_revision,
                ..
            } if !matches_window(window_id, *observation_revision) => {
                Err(GuestProtocolErrorV1::StaleObservation)
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
