use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::computer_use::{
    ComputerActionActorV1, ComputerActionV1, ComputerIdentityV1, ComputerOperationV1,
    ComputerSessionClassV1, ComputerSessionV1,
};

use super::{
    AuthenticatedGuestChannelV1, GuestActionJournalV1, GuestActionOutcomeV1, GuestProtocolErrorV1,
};

/// App-owned authorization view used by provider bridges. The session and its
/// trusted verification key are supplied by the controller; tool arguments
/// cannot create or widen this authority.
pub struct AuthorizedComputerUseSessionV1 {
    session: ComputerSessionV1,
    controller_key_id: String,
    controller_key: ed25519_dalek::VerifyingKey,
    actor: ComputerActionActorV1,
    role_id: String,
    revoked: RwLock<bool>,
}

/// Bounded, non-secret projection for the application permission view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerUsePermissionV1 {
    pub identity: ComputerIdentityV1,
    pub actor: ComputerActionActorV1,
    pub role_id: String,
    pub machine_id: String,
    pub session_class: ComputerSessionClassV1,
    pub allowed_operations: Vec<ComputerOperationV1>,
    pub network_profile: crate::computer_use::ComputerNetworkProfileV1,
    pub approved_destination: Option<String>,
    pub resource_budget: crate::computer_use::ComputerResourceBudgetV1,
    pub expires_at_unix_ms: u64,
    pub revoked: bool,
    pub expired: bool,
}

impl AuthorizedComputerUseSessionV1 {
    fn new(
        session: ComputerSessionV1,
        controller_key_id: String,
        controller_key: ed25519_dalek::VerifyingKey,
        actor: ComputerActionActorV1,
        role_id: String,
        now_unix_ms: u64,
    ) -> Result<Self, GuestProtocolErrorV1> {
        session
            .validate_for_host(now_unix_ms, &controller_key_id, &controller_key)
            .map_err(|_| GuestProtocolErrorV1::AuthenticationFailed)?;
        if !owns_control(actor, session.lifecycle) {
            return Err(GuestProtocolErrorV1::WrongSession);
        }
        if !crate::computer_use::mcp::is_supported_computer_use_role_v1(&role_id) {
            return Err(GuestProtocolErrorV1::WrongSession);
        }
        Ok(Self {
            session,
            controller_key_id,
            controller_key,
            actor,
            role_id,
            revoked: RwLock::new(false),
        })
    }

    pub fn session(&self) -> &ComputerSessionV1 {
        &self.session
    }

    /// Controller-side revocation takes effect before the next tool call.
    fn revoke(&self) -> Result<(), GuestProtocolErrorV1> {
        let mut revoked = self
            .revoked
            .write()
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        *revoked = true;
        Ok(())
    }

    fn permission(
        &self,
        now_unix_ms: u64,
    ) -> Result<ComputerUsePermissionV1, GuestProtocolErrorV1> {
        let revoked = self
            .revoked
            .read()
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        let machine_id = match &self.session.target {
            crate::computer_use::ComputerTargetV1::LocalKvm { machine_id } => machine_id.clone(),
        };
        Ok(ComputerUsePermissionV1 {
            identity: self.session.identity.clone(),
            actor: self.actor,
            role_id: self.role_id.clone(),
            machine_id,
            session_class: self.session.session_class,
            allowed_operations: self
                .session
                .policy
                .authorized_operations
                .iter()
                .copied()
                .collect(),
            network_profile: self.session.policy.network_profile,
            approved_destination: None,
            resource_budget: self.session.policy.resource_budget.clone(),
            expires_at_unix_ms: self.session.lease.expires_at_unix_ms,
            revoked: *revoked,
            expired: now_unix_ms >= self.session.lease.expires_at_unix_ms
                || now_unix_ms
                    >= self
                        .session
                        .session_started_at_unix_ms
                        .saturating_add(self.session.policy.resource_budget.time_limit_ms),
        })
    }

    pub fn validate_call(&self, now_unix_ms: u64) -> Result<(), GuestProtocolErrorV1> {
        let revoked = self
            .revoked
            .read()
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        if *revoked {
            return Err(GuestProtocolErrorV1::StaleLease);
        }
        self.session
            .validate_for_host(now_unix_ms, &self.controller_key_id, &self.controller_key)
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        if !owns_control(self.actor, self.session.lifecycle) {
            return Err(GuestProtocolErrorV1::WrongSession);
        }
        Ok(())
    }

    /// Revalidates controller signature, lease, time budget, lifecycle, and
    /// actor authority for every action, including calls after a reconnect.
    pub fn execute(
        &self,
        channel: &mut AuthenticatedGuestChannelV1,
        journal: &GuestActionJournalV1,
        action: &ComputerActionV1,
        deadline_unix_ms: u64,
        now_unix_ms: u64,
    ) -> Result<GuestActionOutcomeV1, GuestProtocolErrorV1> {
        // Keep the read guard through dispatch so revocation waits for any
        // already-authorized action and fences every later one.
        let revoked = self
            .revoked
            .read()
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        if *revoked {
            return Err(GuestProtocolErrorV1::StaleLease);
        }
        self.validate_session_and_actor(now_unix_ms)?;
        let machine_id = match &self.session.target {
            crate::computer_use::ComputerTargetV1::LocalKvm { machine_id } => machine_id,
        };
        if channel.config.session_id != self.session.identity.session_id
            || channel.config.worker_id != *machine_id
            || channel.config.lease_generation != self.session.lease.generation
        {
            return Err(GuestProtocolErrorV1::WrongSession);
        }
        let result = channel.execute_action(
            &self.session,
            self.actor,
            action,
            deadline_unix_ms,
            now_unix_ms,
            journal,
        );
        drop(revoked);
        result
    }

    /// Execute the Human-only task-build route after the controller has loaded
    /// every exact P15 receipt under the same task/session/lease/approval.
    #[allow(clippy::too_many_arguments)] // Each independent authority fence is checked here.
    pub fn execute_staged_task_application(
        &self,
        channel: &mut AuthenticatedGuestChannelV1,
        journal: &GuestActionJournalV1,
        artifact_bridge: &crate::computer_use::artifacts::ComputerArtifactBridgeV1,
        action: &ComputerActionV1,
        current_task_revision: &str,
        deadline_unix_ms: u64,
        now_unix_ms: u64,
    ) -> Result<GuestActionOutcomeV1, GuestProtocolErrorV1> {
        let revoked = self
            .revoked
            .read()
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        if *revoked || self.actor != ComputerActionActorV1::Human {
            return Err(GuestProtocolErrorV1::StaleLease);
        }
        self.validate_session_and_actor(now_unix_ms)?;
        let machine_id = match &self.session.target {
            crate::computer_use::ComputerTargetV1::LocalKvm { machine_id } => machine_id,
        };
        if channel.config.session_id != self.session.identity.session_id
            || channel.config.worker_id != *machine_id
            || channel.config.lease_generation != self.session.lease.generation
        {
            return Err(GuestProtocolErrorV1::WrongSession);
        }
        let crate::computer_use::ComputerActionPayloadV1::LaunchApprovedStagedTaskApp {
            application,
        } = &action.payload
        else {
            return Err(GuestProtocolErrorV1::Malformed);
        };
        let contents = artifact_bridge
            .load_staged_task_application_for_guest(
                application,
                &self.session.identity,
                current_task_revision,
                self.session.lease.generation,
                self.session.approval_generation,
                now_unix_ms,
            )
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        channel.execute_staged_task_application(
            &self.session,
            self.actor,
            action,
            &contents,
            deadline_unix_ms,
            now_unix_ms,
            journal,
        )
    }

    fn validate_session_and_actor(&self, now_unix_ms: u64) -> Result<(), GuestProtocolErrorV1> {
        self.session
            .validate_for_host(now_unix_ms, &self.controller_key_id, &self.controller_key)
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        if !owns_control(self.actor, self.session.lifecycle) {
            return Err(GuestProtocolErrorV1::WrongSession);
        }
        Ok(())
    }
}

/// Process-local app authority registry. MCP subprocesses must send each call
/// back to this owner; they must never reconstruct authority from serialized
/// provider arguments. Revocation remains effective across provider reconnects
/// for the lifetime of the owning application turn.
#[derive(Default)]
pub struct ComputerUseAuthorityRegistryV1 {
    sessions: RwLock<HashMap<String, Arc<AuthorizedComputerUseSessionV1>>>,
}

impl ComputerUseAuthorityRegistryV1 {
    pub fn contains_session(&self, session_id: &str) -> Result<bool, GuestProtocolErrorV1> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        Ok(sessions.contains_key(session_id))
    }

    pub fn register(
        &self,
        session: ComputerSessionV1,
        controller_key_id: String,
        controller_key: ed25519_dalek::VerifyingKey,
        actor: ComputerActionActorV1,
        role_id: String,
        now_unix_ms: u64,
    ) -> Result<(), GuestProtocolErrorV1> {
        let session_id = session.identity.session_id.clone();
        let authority = Arc::new(AuthorizedComputerUseSessionV1::new(
            session,
            controller_key_id,
            controller_key,
            actor,
            role_id,
            now_unix_ms,
        )?);
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        if sessions.contains_key(&session_id) {
            return Err(GuestProtocolErrorV1::Replay);
        }
        sessions.insert(session_id, authority);
        Ok(())
    }

    /// Atomically replace one active actor's authority after fencing it.
    /// The old authority is revoked while the registry write lock excludes
    /// new lookups; revoke waits for any action already inside its read guard.
    pub fn replace_actor(
        &self,
        session: ComputerSessionV1,
        controller_key_id: String,
        controller_key: ed25519_dalek::VerifyingKey,
        actor: ComputerActionActorV1,
        role_id: String,
        now_unix_ms: u64,
    ) -> Result<(), GuestProtocolErrorV1> {
        let session_id = session.identity.session_id.clone();
        let replacement = Arc::new(AuthorizedComputerUseSessionV1::new(
            session,
            controller_key_id,
            controller_key,
            actor,
            role_id,
            now_unix_ms,
        )?);
        let mut sessions = self
            .sessions
            .write()
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        let previous = sessions
            .get(&session_id)
            .ok_or(GuestProtocolErrorV1::WrongSession)?;
        if previous.session.identity != replacement.session.identity {
            return Err(GuestProtocolErrorV1::WrongSession);
        }
        previous.revoke()?;
        sessions.insert(session_id, replacement);
        Ok(())
    }

    pub fn authorize(
        &self,
        caller: &ComputerIdentityV1,
        actor: ComputerActionActorV1,
        role_id: &str,
        now_unix_ms: u64,
    ) -> Result<Arc<AuthorizedComputerUseSessionV1>, GuestProtocolErrorV1> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        let authority = sessions
            .get(&caller.session_id)
            .cloned()
            .ok_or(GuestProtocolErrorV1::WrongSession)?;
        if &authority.session.identity != caller
            || authority.actor != actor
            || authority.role_id != role_id
        {
            return Err(GuestProtocolErrorV1::WrongSession);
        }
        authority.validate_call(now_unix_ms)?;
        Ok(authority)
    }

    pub fn revoke(&self, session_id: &str) -> Result<(), GuestProtocolErrorV1> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        let authority = sessions
            .get(session_id)
            .ok_or(GuestProtocolErrorV1::WrongSession)?;
        authority.revoke()
    }

    /// List the controller's current grants for a user-facing permission view.
    /// This exposes identity and policy metadata only; guest observations and
    /// action evidence remain behind the task-scoped private evidence store.
    pub fn permissions(
        &self,
        now_unix_ms: u64,
    ) -> Result<Vec<ComputerUsePermissionV1>, GuestProtocolErrorV1> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
        let mut permissions = sessions
            .values()
            .map(|authority| authority.permission(now_unix_ms))
            .collect::<Result<Vec<_>, _>>()?;
        permissions.sort_by(|left, right| left.identity.session_id.cmp(&right.identity.session_id));
        Ok(permissions)
    }
}

fn owns_control(
    actor: ComputerActionActorV1,
    lifecycle: crate::computer_use::ComputerLifecycleV1,
) -> bool {
    matches!(
        (actor, lifecycle),
        (
            ComputerActionActorV1::Agent,
            crate::computer_use::ComputerLifecycleV1::Delegated
        ) | (
            ComputerActionActorV1::Human,
            crate::computer_use::ComputerLifecycleV1::HumanControl
        )
    )
}
