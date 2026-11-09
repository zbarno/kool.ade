use std::sync::Arc;

use crate::computer_use::{ComputerActionActorV1 as Actor, ComputerLifecycleV1 as Lifecycle};

use super::{
    types::{ActiveSession, ComputerUseMcpControllerV1},
    unix_time_ms,
};
use crate::computer_use::mcp::{
    ComputerUseControlCommandV1 as ControlCommand, ComputerUseControlOwnerV1 as Owner,
};

impl ComputerUseMcpControllerV1 {
    pub fn queue_control_change(
        self: &Arc<Self>,
        task_id: &str,
        user_id: &str,
        session_id: &str,
        command: ControlCommand,
    ) -> Result<(), &'static str> {
        let view = self
            .view_cache
            .lock()
            .map_err(|_| "session_state_unavailable")?
            .get(session_id)
            .cloned()
            .ok_or("session_not_active")?;
        if view.task_id != task_id || view.user_id != user_id {
            return Err("session_binding_mismatch");
        }
        {
            let mut pending = self
                .pending_controls
                .lock()
                .map_err(|_| "session_state_unavailable")?;
            if pending.get(session_id).is_some_and(Option::is_none) {
                return Err("control_change_already_pending");
            }
            pending.insert(session_id.to_owned(), None);
        }
        let weak = Arc::downgrade(self);
        let task = task_id.to_owned();
        let user = user_id.to_owned();
        let session = session_id.to_owned();
        std::thread::spawn(move || {
            let Some(controller) = weak.upgrade() else {
                return;
            };
            let result = match command {
                ControlCommand::PauseAgent => controller.pause_agent(&task, &user, &session),
                ControlCommand::ResumeAgent => controller.resume_agent(&task, &user, &session),
                ControlCommand::TakeControl => {
                    controller.take_human_control(&task, &user, &session)
                }
                ControlCommand::ReturnControl => {
                    controller.return_agent_control(&task, &user, &session)
                }
                ControlCommand::StopComputer => {
                    controller.stop_from_task_view(&task, &user, &session)
                }
                ControlCommand::LaunchApprovedApp(app_id) => {
                    controller.launch_approved_app(&task, &user, &session, &app_id)
                }
            };
            let error = result.err().map(str::to_owned);
            if let Ok(mut pending) = controller.pending_controls.lock() {
                if let Some(error) = &error {
                    pending.insert(session.clone(), Some(error.clone()));
                } else {
                    pending.remove(&session);
                }
            }
            if let Some(error) = error
                && let Ok(mut views) = controller.view_cache.lock()
                && let Some(view) = views.get_mut(&session)
            {
                view.control_error = Some(error);
            }
        });
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
