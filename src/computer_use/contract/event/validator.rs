use super::{ComputerEventKindV1, ComputerEventV1};
use crate::computer_use::{
    ComputerActionActorV1, ComputerLifecycleV1, ComputerSessionV1, ComputerStatusV1,
};
use ed25519_dalek::VerifyingKey;
use std::collections::BTreeSet;

/// Stateful controller-side protocol validator. Persist each accepted event
/// before acknowledgment and rebuild state by replaying the trusted journal.
pub struct ComputerEventValidator {
    session_id: String,
    lifecycle: ComputerLifecycleV1,
    session: ComputerSessionV1,
    last_sequence: u64,
    last_action_sequence: u64,
    approval_generation: u64,
    cancellation_generation: u64,
    lease_generation: u64,
    last_timestamp_unix_ms: u64,
    pending_action: Option<u64>,
    pending_lease_revocation: bool,
    idempotency_keys: BTreeSet<String>,
}

impl ComputerEventValidator {
    pub fn new(
        session: &ComputerSessionV1,
        now_unix_ms: u64,
        expected_key_id: &str,
        verifying_key: &VerifyingKey,
    ) -> Result<Self, &'static str> {
        session.validate_for_host(now_unix_ms, expected_key_id, verifying_key)?;
        Ok(Self {
            session_id: session.identity.session_id.clone(),
            lifecycle: session.lifecycle,
            session: session.clone(),
            last_sequence: 0,
            last_action_sequence: session.action_sequence,
            approval_generation: session.approval_generation,
            cancellation_generation: session.cancellation_generation,
            lease_generation: session.lease.generation,
            last_timestamp_unix_ms: 0,
            pending_action: None,
            pending_lease_revocation: false,
            idempotency_keys: BTreeSet::new(),
        })
    }

    pub fn accept(
        &mut self,
        event: &ComputerEventV1,
        now_unix_ms: u64,
    ) -> Result<(), &'static str> {
        self.session.lease.validate(now_unix_ms)?;
        event.validate()?;
        if event.occurred_at_unix_ms > now_unix_ms {
            return Err("event timestamp is in the future");
        }
        if event.session_id != self.session_id {
            return Err("event session identity changed");
        }
        let expected_sequence = self
            .last_sequence
            .checked_add(1)
            .ok_or("event sequence is exhausted")?;
        if event.sequence != expected_sequence {
            return Err("event sequence is replayed or discontinuous");
        }
        if event.occurred_at_unix_ms < self.last_timestamp_unix_ms {
            return Err("event timestamp moved backwards");
        }
        if event.approval_generation != self.approval_generation
            || event.cancellation_generation != self.cancellation_generation
        {
            return Err("event authority generation does not match controller state");
        }
        if event.lease_generation != self.lease_generation {
            return Err("event lease generation does not match controller state");
        }

        match &event.kind {
            ComputerEventKindV1::StateChanged { from, to } => {
                if self.pending_action.is_some() {
                    return Err("cannot change lifecycle while an action is unresolved");
                }
                if *from != self.lifecycle {
                    return Err("event lifecycle does not match controller state");
                }
                self.lifecycle = *to;
            }
            ComputerEventKindV1::ActionAccepted { action } => {
                action.validate(&self.session, now_unix_ms)?;
                if action.session_id != self.session_id
                    || action.lease_generation != event.lease_generation
                    || action.approval_generation != event.approval_generation
                    || action.cancellation_generation != event.cancellation_generation
                {
                    return Err("action authority does not match event authority");
                }
                if action.action_sequence != self.last_action_sequence.saturating_add(1)
                    || self.pending_action.is_some()
                {
                    return Err("action sequence is replayed or another action is unresolved");
                }
                let actor_state_matches = matches!(
                    (action.actor, self.lifecycle),
                    (ComputerActionActorV1::Agent, ComputerLifecycleV1::Delegated)
                        | (
                            ComputerActionActorV1::Human,
                            ComputerLifecycleV1::HumanControl
                        )
                );
                if !actor_state_matches {
                    return Err("action actor does not own computer control");
                }
                if !self.idempotency_keys.insert(action.idempotency_key.clone()) {
                    return Err("action idempotency key was reused");
                }
                self.last_action_sequence = action.action_sequence;
                self.pending_action = Some(action.action_sequence);
            }
            ComputerEventKindV1::ActionCompleted {
                action_sequence,
                status,
                failure,
            } => {
                if self.pending_action != Some(*action_sequence) {
                    return Err("completion has no matching pending action");
                }
                match (status, failure) {
                    (ComputerStatusV1::Succeeded, None) => {}
                    (
                        ComputerStatusV1::Denied
                        | ComputerStatusV1::Failed
                        | ComputerStatusV1::Interrupted,
                        Some(code),
                    ) if *code
                        != crate::computer_use::ComputerFailureCodeV1::UncertainSideEffect => {}
                    (
                        ComputerStatusV1::Uncertain,
                        Some(crate::computer_use::ComputerFailureCodeV1::UncertainSideEffect),
                    ) => {}
                    _ => return Err("action completion status and failure disagree"),
                }
                match status {
                    ComputerStatusV1::Uncertain | ComputerStatusV1::Interrupted
                        if !crate::computer_use::can_transition(
                            self.lifecycle,
                            ComputerLifecycleV1::Interrupted,
                        ) =>
                    {
                        return Err("action interruption has no valid lifecycle transition");
                    }
                    ComputerStatusV1::Failed
                        if !crate::computer_use::can_transition(
                            self.lifecycle,
                            ComputerLifecycleV1::Failed,
                        ) =>
                    {
                        return Err("action failure has no valid lifecycle transition");
                    }
                    _ => {}
                }
                self.pending_action = None;
                match status {
                    ComputerStatusV1::Uncertain | ComputerStatusV1::Interrupted => {
                        self.lifecycle = ComputerLifecycleV1::Interrupted;
                    }
                    ComputerStatusV1::Failed => self.lifecycle = ComputerLifecycleV1::Failed,
                    _ => {}
                }
            }
            ComputerEventKindV1::Observation { observation } => {
                if observation.session_id != self.session_id
                    || observation.action_sequence > self.last_action_sequence
                {
                    return Err("observation refers to an unknown action or session");
                }
            }
            ComputerEventKindV1::LeaseRevoked {
                cancellation_generation,
            } => {
                if !self.pending_lease_revocation
                    || *cancellation_generation != event.cancellation_generation
                {
                    return Err("lease revocation does not match cancellation fence");
                }
                self.pending_lease_revocation = false;
            }
        }
        self.last_sequence = event.sequence;
        self.approval_generation = event.approval_generation;
        self.cancellation_generation = event.cancellation_generation;
        self.last_timestamp_unix_ms = event.occurred_at_unix_ms;
        Ok(())
    }

    /// Called only after the controller commits a new grant or cancellation.
    pub fn advance_authority(
        &mut self,
        session: &ComputerSessionV1,
        now_unix_ms: u64,
        expected_key_id: &str,
        verifying_key: &VerifyingKey,
    ) -> Result<(), &'static str> {
        session.validate_for_host(now_unix_ms, expected_key_id, verifying_key)?;
        if session.identity != self.session.identity
            || session.target != self.session.target
            || session.image_digest != self.session.image_digest
            || session.action_sequence != self.last_action_sequence
            || session.approval_generation < self.approval_generation
            || session.cancellation_generation < self.cancellation_generation
            || session.lease.generation < self.lease_generation
            || (session.approval_generation == self.approval_generation
                && session.cancellation_generation == self.cancellation_generation
                && session.lease.generation == self.lease_generation)
        {
            return Err("controller authority generation did not advance");
        }
        if session.lease.generation != self.lease_generation {
            session.lease.validate_renewal_from(&self.session.lease)?;
        } else if session.lease.lease_id != self.session.lease.lease_id
            || session.lease.issued_at_unix_ms != self.session.lease.issued_at_unix_ms
            || session.lease.expires_at_unix_ms != self.session.lease.expires_at_unix_ms
        {
            return Err("lease fields changed without advancing the lease fence");
        }
        if session.approval_generation != self.approval_generation && self.pending_action.is_some()
        {
            return Err("cannot replace approval while an action is unresolved");
        }
        if session.cancellation_generation > self.cancellation_generation {
            if session.lifecycle != ComputerLifecycleV1::Interrupted
                || !crate::computer_use::can_transition(
                    self.lifecycle,
                    ComputerLifecycleV1::Interrupted,
                )
            {
                return Err("cancellation must interrupt the session");
            }
            self.pending_action = None;
            self.pending_lease_revocation = true;
        } else if !crate::computer_use::can_transition(self.lifecycle, session.lifecycle)
            && self.lifecycle != session.lifecycle
        {
            return Err("controller session lifecycle transition is forbidden");
        }
        self.lifecycle = session.lifecycle;
        self.approval_generation = session.approval_generation;
        self.cancellation_generation = session.cancellation_generation;
        self.lease_generation = session.lease.generation;
        self.session = session.clone();
        Ok(())
    }
}
