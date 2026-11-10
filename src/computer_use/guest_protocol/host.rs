use std::{sync::Arc, time::Duration};
mod actions;
mod authorized;
mod staged_task;

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
    GuestChannelKeyV1, GuestObservationV1, GuestOperationResultV1, GuestProtocolErrorV1,
    GuestReplayDecisionV1, GuestSenderV1,
    auth::{AuthenticatedGuestEnvelopeV1, GuestHandshakeCompleteV1, host_handshake},
    frame::{read_frame, write_frame},
    types::{
        GUEST_ARTIFACT_CAPABILITY_V1, GUEST_ARTIFACT_CHUNK_MAX_BYTES_V1,
        GUEST_CHANNEL_CAPABILITY_V1, GuestEnvelopeContextV1, GuestMessageV1, required_capability,
    },
};
use validation::{readiness_token, request_id_for_action, validate_observation};

pub struct GuestHostTransportV1;

pub struct AuthenticatedGuestChannelV1 {
    channel: Option<ComputerGuestChannelV1>,
    context: GuestAuthenticatedContextV1,
    channel_key: Option<GuestChannelKeyV1>,
    remote_transport: Option<Arc<dyn GuestActionTransportV1>>,
    capabilities: GuestCapabilitySetV1,
    transcript_sha256: String,
    request_id: String,
    action_id: String,
    host_sequence: u64,
    guest_sequence: u64,
    guest_uptime_millis: u64,
    latest_observation: Option<GuestObservationV1>,
}

#[derive(Clone)]
pub(crate) struct GuestAuthenticatedContextV1 {
    pub session_id: String,
    pub worker_id: String,
    pub lease_generation: u64,
    pub expires_at_unix_ms: u64,
    pub boot_id: String,
}

pub(crate) trait GuestActionTransportV1: Send + Sync {
    fn execute_action(
        &self,
        session: &ComputerSessionV1,
        actor: ComputerActionActorV1,
        action: &ComputerActionV1,
        deadline_unix_ms: u64,
        now_unix_ms: u64,
    ) -> Result<GuestActionOutcomeV1, GuestProtocolErrorV1>;

    /// Called only after the controller action journal durably records the
    /// authenticated outcome. Implementations may clear recovery uncertainty.
    fn acknowledge_action_outcome(
        &self,
        _session: &ComputerSessionV1,
        _action_sequence: u64,
    ) -> Result<(), GuestProtocolErrorV1> {
        Ok(())
    }
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
            channel: Some(channel),
            context: GuestAuthenticatedContextV1 {
                session_id: config.session_id.clone(),
                worker_id: config.worker_id.clone(),
                lease_generation: config.lease_generation,
                expires_at_unix_ms: config.expires_at_unix_ms,
                boot_id: config.boot_id.clone(),
            },
            channel_key: Some(completed.channel_key),
            remote_transport: None,
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

    pub(crate) fn from_remote(
        context: GuestAuthenticatedContextV1,
        capabilities: GuestCapabilitySetV1,
        transcript_sha256: String,
        remote_transport: Arc<dyn GuestActionTransportV1>,
    ) -> Self {
        Self {
            channel: None,
            context,
            channel_key: None,
            remote_transport: Some(remote_transport),
            capabilities,
            transcript_sha256,
            request_id: String::new(),
            action_id: String::new(),
            host_sequence: 0,
            guest_sequence: 0,
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

    pub(crate) fn readiness(&self) -> ComputerGuestReadinessV1 {
        ComputerGuestReadinessV1::new(
            self.context.session_id.clone(),
            self.context.lease_generation,
            self.context.boot_id.clone(),
            self.transcript_sha256.clone(),
        )
    }

    #[allow(clippy::too_many_arguments)] // Each chunk, digest, session, and deadline fence is checked independently.
    pub(crate) fn write_artifact_chunk(
        &mut self,
        session: &ComputerSessionV1,
        transfer_id: &str,
        file_name: &str,
        offset: u64,
        total_bytes: u64,
        sha256: &str,
        bytes: &[u8],
        now_unix_ms: u64,
    ) -> Result<(u64, bool), GuestProtocolErrorV1> {
        if self.remote_transport.is_some()
            || session.identity.session_id != self.context.session_id
            || session.lease.generation != self.context.lease_generation
            || session.lease.expires_at_unix_ms <= now_unix_ms
            || !canonical_uuid(transfer_id)
            || !valid_artifact_name(file_name)
            || total_bytes == 0
            || total_bytes > 64 * 1024 * 1024
            || bytes.is_empty()
            || bytes.len() > GUEST_ARTIFACT_CHUNK_MAX_BYTES_V1
            || offset
                .checked_add(bytes.len() as u64)
                .is_none_or(|end| end > total_bytes)
            || !valid_artifact_sha256(sha256)
        {
            return Err(GuestProtocolErrorV1::Malformed);
        }
        let response = self.exchange_artifact_message(
            transfer_id,
            GuestMessageV1::ArtifactWriteChunk {
                transfer_id: transfer_id.to_owned(),
                file_name: file_name.to_owned(),
                offset,
                total_bytes,
                sha256: sha256.to_owned(),
                bytes: bytes.to_vec(),
            },
            session.lease.expires_at_unix_ms,
        )?;
        match response {
            GuestMessageV1::ArtifactWriteAck {
                transfer_id: response_id,
                next_offset,
                complete,
            } if response_id == transfer_id
                && next_offset == offset + bytes.len() as u64
                && complete == (next_offset == total_bytes) =>
            {
                Ok((next_offset, complete))
            }
            _ => Err(GuestProtocolErrorV1::Replay),
        }
    }

    pub(crate) fn read_artifact_chunk(
        &mut self,
        session: &ComputerSessionV1,
        transfer_id: &str,
        file_name: &str,
        offset: u64,
        now_unix_ms: u64,
    ) -> Result<(u64, String, Vec<u8>, bool), GuestProtocolErrorV1> {
        if self.remote_transport.is_some()
            || session.identity.session_id != self.context.session_id
            || session.lease.generation != self.context.lease_generation
            || session.lease.expires_at_unix_ms <= now_unix_ms
            || !canonical_uuid(transfer_id)
            || !valid_artifact_name(file_name)
            || offset > 64 * 1024 * 1024
        {
            return Err(GuestProtocolErrorV1::Malformed);
        }
        let response = self.exchange_artifact_message(
            transfer_id,
            GuestMessageV1::ArtifactReadChunk {
                transfer_id: transfer_id.to_owned(),
                file_name: file_name.to_owned(),
                offset,
                max_bytes: GUEST_ARTIFACT_CHUNK_MAX_BYTES_V1 as u32,
            },
            session.lease.expires_at_unix_ms,
        )?;
        let GuestMessageV1::ArtifactReadResult {
            transfer_id: response_id,
            offset: response_offset,
            total_bytes,
            sha256,
            bytes,
            complete,
        } = response
        else {
            return Err(GuestProtocolErrorV1::Malformed);
        };
        if response_id != transfer_id
            || response_offset != offset
            || total_bytes == 0
            || total_bytes > 64 * 1024 * 1024
            || bytes.is_empty()
            || bytes.len() > GUEST_ARTIFACT_CHUNK_MAX_BYTES_V1
            || offset.saturating_add(bytes.len() as u64) > total_bytes
            || complete != (offset + bytes.len() as u64 == total_bytes)
            || !valid_artifact_sha256(&sha256)
        {
            return Err(GuestProtocolErrorV1::Malformed);
        }
        Ok((total_bytes, sha256, bytes, complete))
    }

    pub(crate) fn abort_artifact_transfer(
        &mut self,
        session: &ComputerSessionV1,
        transfer_id: &str,
        now_unix_ms: u64,
    ) -> Result<(), GuestProtocolErrorV1> {
        if self.remote_transport.is_some()
            || session.identity.session_id != self.context.session_id
            || session.lease.generation != self.context.lease_generation
            || session.lease.expires_at_unix_ms <= now_unix_ms
            || !canonical_uuid(transfer_id)
        {
            return Err(GuestProtocolErrorV1::Malformed);
        }
        match self.exchange_artifact_message(
            transfer_id,
            GuestMessageV1::ArtifactAbort {
                transfer_id: transfer_id.to_owned(),
            },
            session.lease.expires_at_unix_ms,
        )? {
            GuestMessageV1::ArtifactWriteAck {
                transfer_id: response_id,
                next_offset: 0,
                complete: false,
            } if response_id == transfer_id => Ok(()),
            _ => Err(GuestProtocolErrorV1::Replay),
        }
    }

    fn exchange_artifact_message(
        &mut self,
        transfer_id: &str,
        message: GuestMessageV1,
        deadline_unix_ms: u64,
    ) -> Result<GuestMessageV1, GuestProtocolErrorV1> {
        let envelope = self.envelope(
            GuestEnvelopeContextV1 {
                sender: GuestSenderV1::Host,
                sequence: self.host_sequence,
                request_id: transfer_id.to_owned(),
                action_id: transfer_id.to_owned(),
                deadline_unix_ms,
                capability_id: GUEST_ARTIFACT_CAPABILITY_V1.to_owned(),
            },
            message,
        );
        let key = self
            .channel_key
            .as_ref()
            .ok_or(GuestProtocolErrorV1::WrongSession)?;
        let wire = AuthenticatedGuestEnvelopeV1::sign(envelope.clone(), key.expose())?;
        write_frame(
            self.channel
                .as_mut()
                .ok_or(GuestProtocolErrorV1::WrongSession)?
                .io(),
            &wire,
        )?;
        self.host_sequence = self.host_sequence.saturating_add(1);
        loop {
            let incoming: AuthenticatedGuestEnvelopeV1 = read_frame(
                self.channel
                    .as_mut()
                    .ok_or(GuestProtocolErrorV1::WrongSession)?
                    .io(),
            )?;
            incoming.verify(key.expose())?;
            if let GuestMessageV1::Heartbeat {
                boot_id,
                uptime_millis,
            } = &incoming.envelope.message
            {
                if incoming.envelope.sender != GuestSenderV1::Guest
                    || incoming.envelope.sequence != self.guest_sequence
                    || incoming.envelope.session_id != self.context.session_id
                    || incoming.envelope.worker_id != self.context.worker_id
                    || incoming.envelope.lease_generation != self.context.lease_generation
                    || incoming.envelope.capability_id != GUEST_CHANNEL_CAPABILITY_V1
                    || incoming.envelope.request_id != self.request_id
                    || incoming.envelope.action_id != self.action_id
                    || boot_id != &self.context.boot_id
                    || *uptime_millis < self.guest_uptime_millis
                {
                    return Err(GuestProtocolErrorV1::Replay);
                }
                self.guest_uptime_millis = *uptime_millis;
                self.guest_sequence = self.guest_sequence.saturating_add(1);
                continue;
            }
            self.validate_incoming(
                &incoming,
                transfer_id,
                transfer_id,
                GUEST_ARTIFACT_CAPABILITY_V1,
                deadline_unix_ms,
            )?;
            self.guest_sequence = self.guest_sequence.saturating_add(1);
            return Ok(incoming.envelope.message);
        }
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
        if matches!(
            &action.payload,
            crate::computer_use::ComputerActionPayloadV1::LaunchApprovedStagedTaskApp { .. }
        ) {
            return Err(GuestProtocolErrorV1::UnsupportedCapability);
        }
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
            GuestReplayDecisionV1::Replay(outcome) => {
                if let Some(remote) = &self.remote_transport {
                    remote.acknowledge_action_outcome(session, action.action_sequence)?;
                }
                return Ok(outcome);
            }
            GuestReplayDecisionV1::Execute => {}
        }

        if let Some(remote) = &self.remote_transport {
            let outcome = remote
                .execute_action(session, actor, action, deadline_unix_ms, now_unix_ms)
                .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
            if outcome.reply.action_sequence != action.action_sequence
                || outcome.observation.as_ref().is_some_and(|observation| {
                    observation.session_id != session.identity.session_id
                        || observation.validate_metadata().is_err()
                })
            {
                return Err(GuestProtocolErrorV1::OutcomeUnknown);
            }
            if let Some(observation) = &outcome.observation {
                self.latest_observation = Some(observation.clone());
            }
            journal
                .complete(&request_id, action, outcome.clone())
                .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
            remote.acknowledge_action_outcome(session, action.action_sequence)?;
            return Ok(outcome);
        }

        let timeout = Duration::from_millis(deadline_unix_ms.saturating_sub(now_unix_ms));
        self.channel
            .as_mut()
            .ok_or(GuestProtocolErrorV1::WrongSession)?
            .io()
            .set_read_timeout(Some(timeout))
            .map_err(|_| GuestProtocolErrorV1::OutcomeUnknown)?;
        self.channel
            .as_mut()
            .ok_or(GuestProtocolErrorV1::WrongSession)?
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
            self.channel
                .as_mut()
                .ok_or(GuestProtocolErrorV1::WrongSession)?
                .io(),
            &AuthenticatedGuestEnvelopeV1::sign(
                envelope,
                self.channel_key
                    .as_ref()
                    .ok_or(GuestProtocolErrorV1::WrongSession)?
                    .expose(),
            )?,
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
        incoming.verify(
            self.channel_key
                .as_ref()
                .ok_or(GuestProtocolErrorV1::WrongSession)?
                .expose(),
        )?;
        let envelope = &incoming.envelope;
        if envelope.sender != GuestSenderV1::Guest
            || envelope.sequence != self.guest_sequence
            || envelope.session_id != self.context.session_id
            || envelope.worker_id != self.context.worker_id
            || envelope.lease_generation != self.context.lease_generation
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
            session_id: self.context.session_id.clone(),
            worker_id: self.context.worker_id.clone(),
            lease_generation: self.context.lease_generation,
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
            "computer_use.staged_app.v1" => self.capabilities.staged_task_app_launch,
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

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn valid_artifact_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('.')
        && !value.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        && !value.chars().any(char::is_control)
        && !matches!(value, "." | "..")
}

fn valid_artifact_sha256(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests;
