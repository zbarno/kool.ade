use crate::computer_use::mcp::{
    ComputerUsePersonalProfileViewV1, ComputerUsePersonalVmChoiceV1,
    ComputerUsePersonalVmConsentViewV1,
};
use crate::computer_use::{PersonalVmProfileV1, PersonalVmStorageErrorV1};

use super::{types::ComputerUseMcpControllerV1, unix_time_ms};

const PERSONAL_VM_CONSENT_LIFETIME_MS: u64 = 60_000;
const MAX_PENDING_PERSONAL_VM_CONSENTS: usize = 8;

impl ComputerUseMcpControllerV1 {
    /// Creates an app-owned typed consent request. This method is intentionally
    /// not part of the provider MCP request enum or ordinary session grant path.
    pub fn request_personal_vm_consent_for_current_user(
        &self,
        choice: ComputerUsePersonalVmChoiceV1,
    ) -> Result<String, &'static str> {
        let owner_id = current_owner_id();
        let runtime = self.runtime.as_ref().ok_or("trusted_runtime_unavailable")?;
        let store = runtime
            .personal_profiles
            .as_ref()
            .ok_or("personal_vm_key_service_unavailable")?;
        let profile_id = match &choice {
            ComputerUsePersonalVmChoiceV1::Create { .. } => uuid::Uuid::new_v4().to_string(),
            ComputerUsePersonalVmChoiceV1::Unlock { profile_id } => profile_id.clone(),
        };
        match &choice {
            ComputerUsePersonalVmChoiceV1::Create { retention_days }
                if (1..=365).contains(retention_days) => {}
            ComputerUsePersonalVmChoiceV1::Create { .. } => {
                return Err("personal_vm_retention_out_of_range");
            }
            ComputerUsePersonalVmChoiceV1::Unlock { profile_id } => {
                store
                    .expire_owned_profiles(&owner_id, unix_time_ms())
                    .map_err(|_| "personal_vm_profiles_unavailable")?;
                if !store
                    .list_owned_profiles(&owner_id)
                    .map_err(|_| "personal_vm_profiles_unavailable")?
                    .iter()
                    .any(|profile| profile.profile_id == *profile_id)
                {
                    return Err("personal_vm_profile_not_owned");
                }
            }
        }
        let now = unix_time_ms();
        let session_id = uuid::Uuid::new_v4().to_string();
        let view = ComputerUsePersonalVmConsentViewV1 {
            request_id: uuid::Uuid::new_v4().to_string(),
            choice,
            target: personal_session_target(&owner_id, &profile_id, &session_id),
            expires_at_unix_ms: now.saturating_add(PERSONAL_VM_CONSENT_LIFETIME_MS),
        };
        let mut pending = self
            .pending_personal_vm_consents
            .lock()
            .map_err(|_| "personal_vm_consent_unavailable")?;
        pending.retain(|_, request| request.view.expires_at_unix_ms > now);
        if pending.len() >= MAX_PENDING_PERSONAL_VM_CONSENTS {
            return Err("personal_vm_consent_queue_full");
        }
        pending.insert(
            view.request_id.clone(),
            super::types::PendingPersonalVmConsentV1 {
                owner_id,
                view: view.clone(),
            },
        );
        Ok(view.request_id)
    }

    pub fn personal_vm_consent_requests_for_current_user(
        &self,
    ) -> Vec<ComputerUsePersonalVmConsentViewV1> {
        let owner_id = current_owner_id();
        let now = unix_time_ms();
        let Ok(mut pending) = self.pending_personal_vm_consents.lock() else {
            return Vec::new();
        };
        pending.retain(|_, request| request.view.expires_at_unix_ms > now);
        pending
            .values()
            .filter(|request| request.owner_id == owner_id)
            .map(|request| request.view.clone())
            .collect()
    }

    pub fn decide_personal_vm_consent_for_current_user(
        &self,
        request_id: &str,
        approve: bool,
    ) -> Result<(), &'static str> {
        let owner_id = current_owner_id();
        let mut pending = self
            .pending_personal_vm_consents
            .lock()
            .map_err(|_| "personal_vm_consent_unavailable")?;
        let request = pending
            .get(request_id)
            .ok_or("personal_vm_consent_not_pending")?;
        if request.owner_id != owner_id || request.view.expires_at_unix_ms <= unix_time_ms() {
            pending.remove(request_id);
            return Err("personal_vm_consent_expired_or_wrong_owner");
        }
        if !approve {
            pending.remove(request_id);
            return Ok(());
        }
        if !super::session::startup::PERSONAL_VM_R3_ENABLED {
            return Err("personal_vm_r3_security_gate_disabled");
        }
        let request = pending
            .remove(request_id)
            .ok_or("personal_vm_consent_not_pending")?;
        drop(pending);
        let runtime = self.runtime.as_ref().ok_or("trusted_runtime_unavailable")?;
        super::session::startup::start_personal_session(
            self,
            runtime,
            &request.view.choice,
            &request.view.target,
            &owner_id,
            &std::sync::atomic::AtomicBool::new(false),
        )
    }

    /// Lists profiles for the current local OS account. The owner identity is
    /// derived in-process and is never accepted from an MCP/provider argument.
    pub fn personal_vm_profiles_for_current_user(
        &self,
    ) -> Result<Vec<PersonalVmProfileV1>, PersonalVmStorageErrorV1> {
        let owner_id = current_owner_id();
        let runtime = self
            .runtime
            .as_ref()
            .ok_or(PersonalVmStorageErrorV1::KeyServiceUnavailable)?;
        let store = runtime
            .personal_profiles
            .as_ref()
            .ok_or(PersonalVmStorageErrorV1::KeyServiceUnavailable)?;
        store
            .expire_owned_profiles(&owner_id, unix_time_ms())
            .map_err(|_| PersonalVmStorageErrorV1::Io)?;
        store.list_owned_profiles(&owner_id)
    }

    pub fn personal_vm_profile_views_for_current_user(
        &self,
    ) -> Result<Vec<ComputerUsePersonalProfileViewV1>, PersonalVmStorageErrorV1> {
        let profiles = self.personal_vm_profiles_for_current_user()?;
        let active = self
            .sessions
            .lock()
            .map_err(|_| PersonalVmStorageErrorV1::Io)?;
        Ok(profiles
            .into_iter()
            .map(|profile| {
                let active_session_id = active.iter().find_map(|(session_id, session)| {
                    (session.session.identity.user_id == profile.owner_id
                        && session.personal_profile_id.as_deref() == Some(&profile.profile_id))
                    .then(|| session_id.clone())
                });
                ComputerUsePersonalProfileViewV1 {
                    profile_id: profile.profile_id,
                    created_at_unix_ms: profile.created_at_unix_ms,
                    retention_until_unix_ms: profile.retention_until_unix_ms,
                    active_session_id,
                }
            })
            .collect())
    }

    /// Deletes an owner-selected profile. The profile store enforces ownership,
    /// active-session lock fencing, and key revocation before deleting bytes.
    pub fn delete_personal_vm_profile_for_current_user(
        &self,
        profile_id: &str,
    ) -> Result<(), PersonalVmStorageErrorV1> {
        let owner_id = current_owner_id();
        let runtime = self
            .runtime
            .as_ref()
            .ok_or(PersonalVmStorageErrorV1::KeyServiceUnavailable)?;
        let store = runtime
            .personal_profiles
            .as_ref()
            .ok_or(PersonalVmStorageErrorV1::KeyServiceUnavailable)?;
        store.delete_profile(&owner_id, profile_id)
    }

    /// A profile lock is a normal owner-authorized VM release. The QEMU backend
    /// stops the guest and preserves the encrypted qcow2; dropping the session
    /// then zeroizes the volatile key and releases its exclusive profile lock.
    pub fn lock_personal_vm_session_for_current_user(
        &self,
        session_id: &str,
        profile_id: &str,
    ) -> Result<(), &'static str> {
        let owner_id = current_owner_id();
        let task_id = {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| "session_state_unavailable")?;
            let active = sessions.get(session_id).ok_or("session_not_active")?;
            if active.session.identity.user_id != owner_id
                || active.session.session_class
                    != crate::computer_use::ComputerSessionClassV1::PersonalPersistent
                || active.personal_profile_id.as_deref() != Some(profile_id)
                || active.control_owner
                    != crate::computer_use::mcp::ComputerUseControlOwnerV1::Human
                || !active.private_input
                || active.uncertain
                || active.paused
            {
                return Err("personal_vm_owner_control_required");
            }
            active.session.identity.task_id.clone()
        };
        self.stop_from_task_view(&task_id, &owner_id, session_id)
    }
}

fn current_owner_id() -> String {
    let uid = unsafe { libc::geteuid() };
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("koolade-linux-effective-uid-v1:{uid}").as_bytes(),
    )
    .to_string()
}

pub(super) fn personal_session_target(
    owner_id: &str,
    profile_id: &str,
    session_id: &str,
) -> crate::computer_use::mcp::ComputerUsePersonalVmSessionTargetV1 {
    crate::computer_use::mcp::ComputerUsePersonalVmSessionTargetV1 {
        identity: crate::computer_use::ComputerIdentityV1 {
            project_id: uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_OID,
                format!("koolade-personal-vm-project-v1:{owner_id}").as_bytes(),
            )
            .to_string(),
            task_id: profile_id.to_owned(),
            user_id: owner_id.to_owned(),
            session_id: session_id.to_owned(),
        },
        profile_id: profile_id.to_owned(),
        session_class: crate::computer_use::ComputerSessionClassV1::PersonalPersistent,
        network_profile: crate::computer_use::ComputerNetworkProfileV1::Disabled,
        initial_control_owner: crate::computer_use::mcp::ComputerUseControlOwnerV1::Human,
        private_input_required: true,
    }
}

#[cfg(test)]
mod tests {
    use super::personal_session_target;
    use crate::computer_use::mcp::ComputerUseControlOwnerV1;
    use crate::computer_use::{ComputerNetworkProfileV1, ComputerSessionClassV1};

    #[test]
    fn personal_session_target_is_profile_bound_offline_and_human_owned() {
        let owner = "00000000-0000-4000-8000-000000000001";
        let profile = "00000000-0000-4000-8000-000000000002";
        let session = "00000000-0000-4000-8000-000000000003";
        let target = personal_session_target(owner, profile, session);

        assert_eq!(target.identity.user_id, owner);
        assert_eq!(target.identity.task_id, profile);
        assert_eq!(target.identity.session_id, session);
        assert_eq!(target.profile_id, profile);
        assert_eq!(
            target.session_class,
            ComputerSessionClassV1::PersonalPersistent
        );
        assert_eq!(target.network_profile, ComputerNetworkProfileV1::Disabled);
        assert_eq!(
            target.initial_control_owner,
            ComputerUseControlOwnerV1::Human
        );
        assert!(target.private_input_required);
    }
}
