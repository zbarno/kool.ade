use std::collections::VecDeque;

use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

use crate::{
    computer_use::{ComputerActionV1, ComputerEventKindV1, ComputerSessionV1, ComputerTargetV1},
    execution::remote::ssh::VerifiedSshWorker,
};

use crate::computer_use::remote_worker::ComputerRemoteWorkerHostConfigV1;

use super::protocol::{
    ComputerRemoteWorkerOperationV1 as Operation, ComputerRemoteWorkerRequestBodyV1 as Body,
    ComputerRemoteWorkerRequestV1,
};

const MAX_RECENT_REQUEST_IDS: usize = 1_024;
const MAX_TRANSPORT_SEQUENCE: u64 = 1_000_000;

/// Identity token only minted from the pinned SSH worker configuration. The
/// fixed Computer Use SSH command still performs strict host-key checking.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedComputerRemoteWorkerIdentityV1 {
    worker_id: String,
    host_key_fingerprint: String,
}

impl VerifiedComputerRemoteWorkerIdentityV1 {
    pub(crate) fn worker_id(&self) -> &str {
        &self.worker_id
    }
    pub(crate) fn host_key_fingerprint(&self) -> &str {
        &self.host_key_fingerprint
    }

    pub(crate) fn from_ssh_worker(worker: &VerifiedSshWorker) -> Self {
        Self {
            worker_id: worker.stable_id().to_owned(),
            host_key_fingerprint: worker.host_key_fingerprint().to_owned(),
        }
    }

    pub(crate) fn from_host_config(config: &ComputerRemoteWorkerHostConfigV1) -> Self {
        Self {
            worker_id: config.worker_id.clone(),
            host_key_fingerprint: config.ssh_host_key_fingerprint.clone(),
        }
    }

    #[cfg(test)]
    fn fixture(worker_id: &str, host_key_fingerprint: &str) -> Self {
        Self {
            worker_id: worker_id.into(),
            host_key_fingerprint: host_key_fingerprint.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerRemoteWorkerCheckpointV1 {
    pub schema_version: u16,
    pub worker_id: String,
    pub host_key_fingerprint: String,
    pub session_id: String,
    pub lease_id: String,
    pub lease_generation: u64,
    pub approval_generation: u64,
    pub cancellation_generation: u64,
    pub next_transport_sequence: u64,
    pub next_action_sequence: u64,
    pub last_event_sequence: u64,
    pub recent_request_ids: Vec<String>,
    pub pending_action_sequence: Option<u64>,
    pub uncertain_action_sequence: Option<u64>,
    pub cancelled: bool,
    pub released: bool,
}

/// Worker-side authority fence for one signed session. Persist checkpoints in
/// the worker's private durable journal before acknowledging mutating calls.
/// Restart recovery converts in-flight actions to outcome-unknown and never
/// replays them.
pub struct ComputerRemoteWorkerSessionFenceV1 {
    session: ComputerSessionV1,
    worker_id: String,
    host_key_fingerprint: String,
    next_transport_sequence: u64,
    next_action_sequence: u64,
    last_event_sequence: u64,
    recent_request_ids: VecDeque<String>,
    pending_action_sequence: Option<u64>,
    uncertain_action_sequence: Option<u64>,
    cancelled: bool,
    released: bool,
}

impl ComputerRemoteWorkerSessionFenceV1 {
    pub fn session(&self) -> &ComputerSessionV1 {
        &self.session
    }

    pub fn accept_start(
        request: &ComputerRemoteWorkerRequestV1,
        enrollment: &ComputerRemoteWorkerHostConfigV1,
        ssh_identity: &VerifiedComputerRemoteWorkerIdentityV1,
        controller_key_id: &str,
        controller_key: &VerifyingKey,
        now_unix_ms: u64,
    ) -> Result<Self, &'static str> {
        request.validate_shape()?;
        let Body::Start { session } = &request.body else {
            return Err("remote Computer Use session must start with a signed session");
        };
        let configured_key = enrollment.validate()?;
        if enrollment.controller_key_id != controller_key_id
            || configured_key.as_bytes() != controller_key.as_bytes()
        {
            return Err("remote worker controller trust configuration does not match");
        }
        validate_target_binding(session, enrollment, ssh_identity)?;
        session
            .validate_for_host(now_unix_ms, controller_key_id, controller_key)
            .map_err(|_| "remote Computer Use session attestation is invalid")?;
        if session.session_class != crate::computer_use::ComputerSessionClassV1::Disposable {
            return Err("remote persistent Computer Use sessions are not in v1 scope");
        }
        if request.session_id != session.identity.session_id
            || request.lease_id != session.lease.lease_id
            || request.lease_generation != session.lease.generation
            || request.approval_generation != session.approval_generation
            || request.cancellation_generation != session.cancellation_generation
            || request.transport_sequence != 1
        {
            return Err("remote Computer Use start identity is inconsistent");
        }
        let mut recent_request_ids = VecDeque::new();
        recent_request_ids.push_back(request.request_id.clone());
        let fence = Self {
            session: session.clone(),
            worker_id: ssh_identity.worker_id.clone(),
            host_key_fingerprint: ssh_identity.host_key_fingerprint.clone(),
            next_transport_sequence: 2,
            next_action_sequence: session.action_sequence.saturating_add(1),
            last_event_sequence: 0,
            recent_request_ids,
            pending_action_sequence: None,
            uncertain_action_sequence: None,
            cancelled: false,
            released: false,
        };
        Ok(fence)
    }

    pub fn restore(
        session: ComputerSessionV1,
        checkpoint: ComputerRemoteWorkerCheckpointV1,
        enrollment: &ComputerRemoteWorkerHostConfigV1,
        ssh_identity: &VerifiedComputerRemoteWorkerIdentityV1,
        controller_key_id: &str,
        controller_key: &VerifyingKey,
        now_unix_ms: u64,
    ) -> Result<Self, &'static str> {
        session
            .validate_for_host(now_unix_ms, controller_key_id, controller_key)
            .map_err(|_| "remote Computer Use recovery session attestation is invalid")?;
        let configured_key = enrollment.validate()?;
        if enrollment.controller_key_id != controller_key_id
            || configured_key.as_bytes() != controller_key.as_bytes()
        {
            return Err("remote worker controller trust configuration does not match");
        }
        validate_target_binding(&session, enrollment, ssh_identity)?;
        validate_checkpoint(&session, &checkpoint, ssh_identity)?;
        let uncertain_action_sequence = checkpoint
            .uncertain_action_sequence
            .or(checkpoint.pending_action_sequence);
        Ok(Self {
            session,
            worker_id: checkpoint.worker_id,
            host_key_fingerprint: checkpoint.host_key_fingerprint,
            next_transport_sequence: checkpoint.next_transport_sequence,
            next_action_sequence: checkpoint.next_action_sequence,
            last_event_sequence: checkpoint.last_event_sequence,
            recent_request_ids: checkpoint.recent_request_ids.into(),
            pending_action_sequence: None,
            uncertain_action_sequence,
            cancelled: checkpoint.cancelled,
            released: checkpoint.released,
        })
    }

    pub fn accept_request(
        &mut self,
        request: &ComputerRemoteWorkerRequestV1,
        controller_key_id: &str,
        controller_key: &VerifyingKey,
        now_unix_ms: u64,
    ) -> Result<(), &'static str> {
        request.validate_shape()?;
        let reconnect = matches!(
            &request.body,
            Body::Recover { .. } if request.transport_sequence == 0
        );
        if reconnect {
            let Body::Recover {
                replacement_session,
                ..
            } = &request.body
            else {
                unreachable!("reconnect was matched as Recover")
            };
            self.validate_recovery_request_binding(request, replacement_session)?;
        } else {
            self.validate_request_binding(request)?;
        }
        if self.released {
            return Err("remote Computer Use session was released");
        }
        if (!reconnect && request.transport_sequence != self.next_transport_sequence)
            || self.next_transport_sequence > MAX_TRANSPORT_SEQUENCE
        {
            return Err("remote Computer Use transport sequence is stale");
        }
        if self.recent_request_ids.contains(&request.request_id) {
            return Err("remote Computer Use request was replayed");
        }
        match &request.body {
            Body::Probe => return Err("worker probe is outside the session fence"),
            Body::Start { .. } => return Err("remote Computer Use session was already started"),
            Body::Action { action } => self.accept_action(action, now_unix_ms)?,
            Body::Operation { operation } => match operation {
                Operation::GuestAction { handle, action, .. } => {
                    self.validate_handle_binding(handle)?;
                    self.accept_action(action, now_unix_ms)?;
                }
                Operation::Boot { handle }
                | Operation::WaitReady { handle, .. }
                | Operation::Suspend { handle }
                | Operation::Resume { handle }
                | Operation::Stop { handle }
                | Operation::Collect { handle }
                | Operation::Snapshot { handle }
                | Operation::Release { handle }
                | Operation::CaptureDisplay { handle }
                | Operation::SendDisplayInput { handle, .. }
                | Operation::AuthenticateGuest { handle }
                | Operation::MarkGuestReady { handle, .. }
                | Operation::WriteArtifactChunk { handle, .. }
                | Operation::ReadArtifactChunk { handle, .. }
                | Operation::AbortArtifactTransfer { handle, .. } => {
                    self.validate_handle_binding(handle)?;
                    if matches!(operation, Operation::Release { .. }) {
                        self.released = true;
                    }
                    if self.pending_action_sequence.is_some()
                        || self.uncertain_action_sequence.is_some()
                    {
                        return Err("remote Computer Use action outcome is unknown");
                    }
                }
            },
            Body::Cancel {
                replacement_session,
            } => {
                self.validate_replacement_session(
                    replacement_session,
                    controller_key_id,
                    controller_key,
                    now_unix_ms,
                )?;
                if replacement_session.cancellation_generation
                    <= self.session.cancellation_generation
                {
                    return Err("remote Computer Use cancellation fence did not advance");
                }
                self.session = replacement_session.clone();
                self.cancelled = true;
            }
            Body::Recover {
                replacement_session,
                last_event_sequence,
            } => {
                if *last_event_sequence < self.last_event_sequence {
                    return Err("remote Computer Use reconnect stream is stale");
                }
                self.validate_replacement_session(
                    replacement_session,
                    controller_key_id,
                    controller_key,
                    now_unix_ms,
                )?;
                if replacement_session.lease.generation <= self.session.lease.generation
                    || replacement_session.cancellation_generation
                        <= self.session.cancellation_generation
                {
                    return Err("remote Computer Use recovery fence did not advance");
                }
                self.session = replacement_session.clone();
                self.cancelled = false;
                self.last_event_sequence = *last_event_sequence;
                self.next_action_sequence = self.session.action_sequence.saturating_add(1);
            }
        }
        self.recent_request_ids
            .push_back(request.request_id.clone());
        if self.recent_request_ids.len() > MAX_RECENT_REQUEST_IDS {
            self.recent_request_ids.pop_front();
        }
        self.next_transport_sequence = if reconnect {
            1
        } else {
            self.next_transport_sequence
                .checked_add(1)
                .ok_or("remote Computer Use transport sequence exhausted")?
        };
        Ok(())
    }

    fn accept_action(
        &mut self,
        action: &ComputerActionV1,
        now_unix_ms: u64,
    ) -> Result<(), &'static str> {
        if self.cancelled {
            return Err("remote Computer Use session was cancelled");
        }
        if self.pending_action_sequence.is_some() || self.uncertain_action_sequence.is_some() {
            return Err("remote Computer Use action outcome is unknown");
        }
        if action.action_sequence != self.next_action_sequence
            || action.session_id != self.session.identity.session_id
            || action.lease_generation != self.session.lease.generation
            || action.approval_generation != self.session.approval_generation
            || action.cancellation_generation != self.session.cancellation_generation
        {
            return Err("remote Computer Use action fence is stale");
        }
        action.validate_as(action.actor, &self.session, now_unix_ms)?;
        self.pending_action_sequence = Some(action.action_sequence);
        self.next_action_sequence = self
            .next_action_sequence
            .checked_add(1)
            .ok_or("remote Computer Use action sequence exhausted")?;
        Ok(())
    }

    fn validate_replacement_session(
        &self,
        replacement: &ComputerSessionV1,
        controller_key_id: &str,
        controller_key: &VerifyingKey,
        now_unix_ms: u64,
    ) -> Result<(), &'static str> {
        replacement
            .validate_for_host(now_unix_ms, controller_key_id, controller_key)
            .map_err(|_| "remote Computer Use replacement session is invalid")?;
        if replacement.identity != self.session.identity
            || replacement.target != self.session.target
            || replacement.policy != self.session.policy
            || replacement.image_digest != self.session.image_digest
            || replacement.image_manifest_digest != self.session.image_manifest_digest
            || replacement.image_signer_key_id != self.session.image_signer_key_id
        {
            return Err("remote Computer Use replacement changed session authority");
        }
        replacement
            .lease
            .validate_renewal_from(&self.session.lease)?;
        Ok(())
    }

    fn validate_request_binding(
        &self,
        request: &ComputerRemoteWorkerRequestV1,
    ) -> Result<(), &'static str> {
        if request.worker_id != self.worker_id
            || request.ssh_host_key_fingerprint != self.host_key_fingerprint
            || request.session_id != self.session.identity.session_id
            || request.lease_id != self.session.lease.lease_id
            || request.lease_generation != self.session.lease.generation
            || request.approval_generation != self.session.approval_generation
            || request.cancellation_generation != self.session.cancellation_generation
        {
            return Err("remote Computer Use request identity is stale");
        }
        Ok(())
    }

    fn validate_recovery_request_binding(
        &self,
        request: &ComputerRemoteWorkerRequestV1,
        replacement: &ComputerSessionV1,
    ) -> Result<(), &'static str> {
        if request.worker_id != self.worker_id
            || request.ssh_host_key_fingerprint != self.host_key_fingerprint
            || request.session_id != replacement.identity.session_id
            || request.lease_id != replacement.lease.lease_id
            || request.lease_generation != replacement.lease.generation
            || request.approval_generation != replacement.approval_generation
            || request.cancellation_generation != replacement.cancellation_generation
        {
            return Err("remote Computer Use recovery request identity is stale");
        }
        Ok(())
    }

    fn validate_handle_binding(
        &self,
        handle: &crate::computer_use::ComputerMachineHandleV1,
    ) -> Result<(), &'static str> {
        if handle.session_id() != self.session.identity.session_id
            || handle.lease_generation() != self.session.lease.generation
            || handle.machine_id() != self.session.target.target_machine_id()
        {
            return Err("remote Computer Use machine handle is stale");
        }
        Ok(())
    }

    pub fn mark_transport_lost(&mut self) -> Option<u64> {
        let sequence = self.pending_action_sequence.take()?;
        self.uncertain_action_sequence = Some(sequence);
        Some(sequence)
    }

    pub fn record_action_outcome(
        &mut self,
        event: &crate::computer_use::ComputerEventV1,
    ) -> Result<(), &'static str> {
        event.validate()?;
        let Some(sequence) = self
            .pending_action_sequence
            .or(self.uncertain_action_sequence)
        else {
            return Err("remote Computer Use has no action awaiting an outcome");
        };
        if event.session_id != self.session.identity.session_id
            || event.lease_generation != self.session.lease.generation
            || event.cancellation_generation != self.session.cancellation_generation
            || event.sequence <= self.last_event_sequence
        {
            return Err("remote Computer Use action outcome is stale");
        }
        match &event.kind {
            ComputerEventKindV1::ActionCompleted {
                action_sequence, ..
            } if *action_sequence == sequence => {}
            _ => {
                return Err("remote Computer Use action outcome does not match the pending action");
            }
        }
        self.pending_action_sequence = None;
        self.uncertain_action_sequence = None;
        self.last_event_sequence = event.sequence;
        Ok(())
    }

    pub fn record_guest_action_outcome(
        &mut self,
        outcome: &crate::computer_use::guest_protocol::GuestActionOutcomeV1,
    ) -> Result<(), &'static str> {
        let Some(sequence) = self.pending_action_sequence else {
            return Err("remote Computer Use has no guest action awaiting an outcome");
        };
        if outcome.reply.action_sequence != sequence
            || outcome.observation.as_ref().is_some_and(|observation| {
                observation.session_id != self.session.identity.session_id
                    || observation.validate_metadata().is_err()
            })
        {
            return Err("remote Computer Use guest action outcome is mismatched");
        }
        self.pending_action_sequence = None;
        self.uncertain_action_sequence = None;
        self.last_event_sequence = self.last_event_sequence.saturating_add(1);
        Ok(())
    }

    pub fn checkpoint(&self) -> ComputerRemoteWorkerCheckpointV1 {
        ComputerRemoteWorkerCheckpointV1 {
            schema_version: 1,
            worker_id: self.worker_id.clone(),
            host_key_fingerprint: self.host_key_fingerprint.clone(),
            session_id: self.session.identity.session_id.clone(),
            lease_id: self.session.lease.lease_id.clone(),
            lease_generation: self.session.lease.generation,
            approval_generation: self.session.approval_generation,
            cancellation_generation: self.session.cancellation_generation,
            next_transport_sequence: self.next_transport_sequence,
            next_action_sequence: self.next_action_sequence,
            last_event_sequence: self.last_event_sequence,
            recent_request_ids: self.recent_request_ids.iter().cloned().collect(),
            pending_action_sequence: self.pending_action_sequence,
            uncertain_action_sequence: self.uncertain_action_sequence,
            cancelled: self.cancelled,
            released: self.released,
        }
    }
}

fn validate_target_binding(
    session: &ComputerSessionV1,
    enrollment: &ComputerRemoteWorkerHostConfigV1,
    ssh_identity: &VerifiedComputerRemoteWorkerIdentityV1,
) -> Result<(), &'static str> {
    let ComputerTargetV1::RemoteVmWorker {
        worker_id, trust, ..
    } = &session.target
    else {
        return Err("remote worker cannot accept a local VM session");
    };
    if !enrollment.enabled
        || enrollment.worker_id != *worker_id
        || enrollment.worker_id != ssh_identity.worker_id
        || enrollment.ssh_host_key_fingerprint != trust.ssh_host_key_fingerprint
        || enrollment.ssh_host_key_fingerprint != ssh_identity.host_key_fingerprint
        || enrollment.machine_id != session.target.target_machine_id()
    {
        return Err("remote Computer Use target does not match pinned enrollment");
    }
    if enrollment.target()? != session.target {
        return Err("remote Computer Use target trust does not match worker config");
    }
    trust.validate()?;
    Ok(())
}

fn validate_checkpoint(
    session: &ComputerSessionV1,
    checkpoint: &ComputerRemoteWorkerCheckpointV1,
    ssh_identity: &VerifiedComputerRemoteWorkerIdentityV1,
) -> Result<(), &'static str> {
    let target_id = session.target.worker_id();
    if checkpoint.schema_version != 1
        || checkpoint.worker_id != target_id
        || checkpoint.worker_id != ssh_identity.worker_id
        || checkpoint.host_key_fingerprint != ssh_identity.host_key_fingerprint
        || checkpoint.session_id != session.identity.session_id
        || checkpoint.lease_id != session.lease.lease_id
        || checkpoint.lease_generation != session.lease.generation
        || checkpoint.approval_generation != session.approval_generation
        || checkpoint.cancellation_generation != session.cancellation_generation
        || checkpoint.next_transport_sequence == 0
        || checkpoint.next_transport_sequence > MAX_TRANSPORT_SEQUENCE
        || checkpoint.next_action_sequence == 0
        || checkpoint.recent_request_ids.len() > MAX_RECENT_REQUEST_IDS
        || checkpoint
            .recent_request_ids
            .iter()
            .any(|id| !canonical_uuid(id))
        || checkpoint.pending_action_sequence.is_some()
            && checkpoint.uncertain_action_sequence.is_some()
    {
        return Err("remote Computer Use checkpoint is invalid");
    }
    Ok(())
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

#[cfg(test)]
#[path = "fence/tests.rs"]
mod tests;
