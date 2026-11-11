mod queue;

use crate::computer_use::{ComputerActionActorV1 as Actor, ComputerLifecycleV1 as Lifecycle};

use super::{
    types::{ActiveSession, ComputerUseMcpControllerV1},
    unix_time_ms,
};
use crate::computer_use::mcp::ComputerUseControlOwnerV1 as Owner;
use crate::computer_use::mcp::controller::session_journal::SessionEventKindV1 as JournalEvent;
use crate::computer_use::{ComputerActionApprovalV1, ComputerActionIntentStateV1 as IntentState};
use sha2::Digest;

impl ComputerUseMcpControllerV1 {
    pub fn approve_sensitive_action(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
        intent_id: &str,
    ) -> Result<(), &'static str> {
        self.transition_sensitive_action(
            task_id,
            user_id,
            session_id,
            intent_id,
            IntentState::ApprovedOnce,
        )
    }

    pub fn deny_sensitive_action(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
        intent_id: &str,
    ) -> Result<(), &'static str> {
        self.transition_sensitive_action(
            task_id,
            user_id,
            session_id,
            intent_id,
            IntentState::Denied,
        )
    }

    pub fn verify_sensitive_action_completed(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
        intent_id: &str,
    ) -> Result<(), &'static str> {
        self.transition_sensitive_action(
            task_id,
            user_id,
            session_id,
            intent_id,
            IntentState::VerifiedCompleted,
        )
    }

    pub fn mark_sensitive_action_outcome_unknown(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
        intent_id: &str,
    ) -> Result<(), &'static str> {
        self.transition_sensitive_action(
            task_id,
            user_id,
            session_id,
            intent_id,
            IntentState::OutcomeUnknown,
        )
    }

    fn transition_sensitive_action(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
        intent_id: &str,
        next: IntentState,
    ) -> Result<(), &'static str> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?;
        let active = bound_active(&mut sessions, task_id, user_id, session_id)?;
        let pending = active
            .pending_action_intent
            .as_mut()
            .ok_or("action_intent_not_pending")?;
        if pending.intent.intent_id != intent_id || !pending.state.can_transition_to(next) {
            return Err("action_intent_state_changed");
        }
        let now = unix_time_ms();
        if next == IntentState::ApprovedOnce {
            pending
                .intent
                .validate(now)
                .map_err(|_| "action_intent_expired")?;
        }
        if next == IntentState::ApprovedOnce {
            if active.control_owner != Owner::Agent || active.paused {
                return Err("agent_control_required_for_approval");
            }
            let frame = self
                .display_streams
                .lock()
                .ok()
                .and_then(|streams| streams.get(session_id).cloned())
                .and_then(|snapshot| {
                    snapshot
                        .lock()
                        .ok()
                        .map(|snapshot| (snapshot.frame_revision, snapshot.frame_png.clone()))
                })
                .ok_or("current_screen_unavailable")?;
            let Some(png) = frame.1 else {
                return Err("current_screen_unavailable");
            };
            if format!("sha256:{:x}", sha2::Sha256::digest(&png)) != pending.intent.frame_sha256 {
                let digest = pending
                    .intent
                    .digest()
                    .map_err(|_| "invalid_action_intent")?;
                self.runtime
                    .as_ref()
                    .ok_or("trusted_runtime_unavailable")?
                    .session_journal
                    .append_action_intent_state(
                        &active.session,
                        &active.role_id,
                        intent_id,
                        &digest,
                        IntentState::Cancelled,
                        now,
                    )?;
                pending.state = IntentState::Cancelled;
                pending.approval = None;
                active.pending_action_intent = None;
                self.cache_active_view(active);
                return Err("action_intent_frame_stale");
            }
            let intent_sha256 = pending
                .intent
                .digest()
                .map_err(|_| "invalid_action_intent")?;
            let approval = ComputerActionApprovalV1 {
                schema_version: 1,
                intent_id: pending.intent.intent_id.clone(),
                intent_sha256,
                operator_id: user_id.to_owned(),
                task_id: task_id.to_owned(),
                session_id: session_id.to_owned(),
                lease_generation: pending.intent.lease_generation,
                approval_generation: pending.intent.approval_generation,
                cancellation_generation: pending.intent.cancellation_generation,
                expires_at_unix_ms: pending.intent.expires_at_unix_ms,
                one_time_nonce: pending.intent.one_time_nonce.clone(),
            };
            approval
                .validate_for(&pending.intent, now)
                .map_err(|_| "invalid_action_approval")?;
            pending.approval = Some(approval);
        } else if matches!(
            next,
            IntentState::Denied
                | IntentState::Cancelled
                | IntentState::OutcomeUnknown
                | IntentState::VerifiedCompleted
        ) {
            pending.approval = None;
        }
        let digest = pending
            .intent
            .digest()
            .map_err(|_| "invalid_action_intent")?;
        let runtime = self.runtime.as_ref().ok_or("trusted_runtime_unavailable")?;
        runtime.session_journal.append_action_intent_state(
            &active.session,
            &active.role_id,
            intent_id,
            &digest,
            next,
            now,
        )?;
        pending.state = next;
        if matches!(
            next,
            IntentState::Denied
                | IntentState::VerifiedCompleted
                | IntentState::OutcomeUnknown
                | IntentState::Cancelled
                | IntentState::Expired
        ) {
            active.pending_action_intent = None;
        }
        self.cache_active_view(active);
        Ok(())
    }

    pub fn pause_agent(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
    ) -> Result<(), &'static str> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?;
        let active = bound_active(&mut sessions, task_id, user_id, session_id)?;
        if active.control_owner == Owner::Paused {
            return Ok(());
        }
        if active.control_owner != Owner::Agent {
            return Err("human_control_is_active");
        }
        journal_control(self, active, JournalEvent::AgentPaused)?;
        invalidate_pending_action(self, active)?;
        self.registry
            .revoke(session_id)
            .map_err(|_| "authority_revocation_failed")?;
        active.control_owner = Owner::Paused;
        advance_control_generation(active)?;
        record(active, Owner::Agent, "Agent input paused", false);
        self.cache_active_view(active);
        Ok(())
    }

    pub fn resume_agent(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
    ) -> Result<(), &'static str> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?;
        let active = bound_active(&mut sessions, task_id, user_id, session_id)?;
        if active.control_owner == Owner::Agent {
            return Ok(());
        }
        if active.control_owner != Owner::Paused {
            return Err("human_control_is_active");
        }
        journal_control(self, active, JournalEvent::AgentResumed)?;
        self.replace_actor(active, Actor::Agent, Lifecycle::Delegated)?;
        active.control_owner = Owner::Agent;
        advance_control_generation(active)?;
        active.require_fresh_agent_observation = true;
        record(
            active,
            Owner::Agent,
            "Agent resumed; fresh desktop view required",
            false,
        );
        self.cache_active_view(active);
        Ok(())
    }

    pub fn take_human_control(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
    ) -> Result<(), &'static str> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?;
        let active = bound_active(&mut sessions, task_id, user_id, session_id)?;
        if active.control_owner == Owner::Human {
            return Ok(());
        }
        journal_control(self, active, JournalEvent::HumanTakeover)?;
        invalidate_pending_action(self, active)?;
        self.replace_actor(active, Actor::Human, Lifecycle::HumanControl)?;
        active.control_owner = Owner::Human;
        active.require_fresh_agent_observation = false;
        advance_control_generation(active)?;
        record(active, Owner::Human, "Human took control", false);
        self.cache_active_view(active);
        Ok(())
    }

    pub fn return_agent_control(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
    ) -> Result<(), &'static str> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?;
        let active = bound_active(&mut sessions, task_id, user_id, session_id)?;
        if active.control_owner != Owner::Human {
            return Err("human_control_is_not_active");
        }
        journal_control(self, active, JournalEvent::AgentResumed)?;
        invalidate_pending_action(self, active)?;
        self.replace_actor(active, Actor::Agent, Lifecycle::Delegated)?;
        active.control_owner = Owner::Agent;
        active.private_input = false;
        active.require_fresh_agent_observation = true;
        advance_control_generation(active)?;
        if let Ok(streams) = self.display_streams.lock()
            && let Some(snapshot) = streams.get(session_id)
            && let Ok(mut snapshot) = snapshot.lock()
        {
            snapshot.frame_png = None;
        }
        record(
            active,
            Owner::Agent,
            "Control returned; fresh desktop view required",
            false,
        );
        self.cache_active_view(active);
        Ok(())
    }

    fn replace_actor(
        &self,
        active: &mut ActiveSession,
        actor: Actor,
        lifecycle: Lifecycle,
    ) -> Result<(), &'static str> {
        let runtime = self.runtime.as_ref().ok_or("trusted_runtime_unavailable")?;
        let mut candidate = active.session.clone();
        candidate.lifecycle = lifecycle;
        candidate
            .attest(runtime.controller_key_id.clone(), &runtime.controller_key)
            .map_err(|_| "session_attestation_failed")?;
        self.registry
            .replace_actor(
                candidate.clone(),
                runtime.controller_key_id.clone(),
                runtime.controller_key.verifying_key(),
                actor,
                active.role_id.clone(),
                unix_time_ms(),
            )
            .map_err(|_| "authority_replacement_failed")?;
        active.session = candidate;
        Ok(())
    }
}

fn journal_control(
    controller: &ComputerUseMcpControllerV1,
    active: &ActiveSession,
    kind: JournalEvent,
) -> Result<(), &'static str> {
    let runtime = controller
        .runtime
        .as_ref()
        .ok_or("trusted_runtime_unavailable")?;
    runtime
        .session_journal
        .append_session(&active.session, &active.role_id, kind, None, unix_time_ms())
        .map(|_| ())
}

fn invalidate_pending_action(
    controller: &ComputerUseMcpControllerV1,
    active: &mut ActiveSession,
) -> Result<(), &'static str> {
    let Some(pending) = active.pending_action_intent.as_ref() else {
        return Ok(());
    };
    let next = if pending.state == IntentState::Executing {
        IntentState::OutcomeUnknown
    } else if matches!(
        pending.state,
        IntentState::AwaitingApproval | IntentState::ApprovedOnce | IntentState::Prepared
    ) {
        IntentState::Cancelled
    } else {
        active.pending_action_intent = None;
        return Ok(());
    };
    let digest = pending
        .intent
        .digest()
        .map_err(|_| "invalid_action_intent")?;
    controller
        .runtime
        .as_ref()
        .ok_or("trusted_runtime_unavailable")?
        .session_journal
        .append_action_intent_state(
            &active.session,
            &active.role_id,
            &pending.intent.intent_id,
            &digest,
            next,
            unix_time_ms(),
        )?;
    active.pending_action_intent = None;
    Ok(())
}

pub(super) fn bound_active<'a>(
    sessions: &'a mut std::collections::BTreeMap<String, ActiveSession>,
    task_id: &str,
    user_id: &str,
    session_id: &str,
) -> Result<&'a mut ActiveSession, &'static str> {
    let active = sessions.get_mut(session_id).ok_or("session_not_active")?;
    if active.session.identity.task_id != task_id || active.session.identity.user_id != user_id {
        return Err("session_binding_mismatch");
    }
    Ok(active)
}

fn advance_control_generation(active: &mut ActiveSession) -> Result<(), &'static str> {
    active.control_generation = active
        .control_generation
        .checked_add(1)
        .ok_or("control_generation_exhausted")?;
    Ok(())
}

pub(super) fn record(active: &mut ActiveSession, actor: Owner, label: &str, private: bool) {
    if private {
        return;
    }
    active
        .action_log
        .push_back(crate::computer_use::mcp::ComputerUseActionSummaryV1 {
            actor,
            label: label.to_owned(),
            occurred_at_unix_ms: unix_time_ms(),
        });
    while active.action_log.len() > 50 {
        active.action_log.pop_front();
    }
}
