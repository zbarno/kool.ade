mod queue;

use crate::computer_use::{ComputerActionActorV1 as Actor, ComputerLifecycleV1 as Lifecycle};

use super::{
    types::{ActiveSession, ComputerUseMcpControllerV1},
    unix_time_ms,
};
use crate::computer_use::mcp::ComputerUseControlOwnerV1 as Owner;
use crate::computer_use::mcp::controller::session_journal::SessionEventKindV1 as JournalEvent;

impl ComputerUseMcpControllerV1 {
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
