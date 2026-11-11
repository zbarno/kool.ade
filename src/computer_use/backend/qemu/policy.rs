use sha2::{Digest, Sha256};

use crate::computer_use::{
    ComputerBackendErrorV1 as Error, ComputerMachineHandleV1 as Handle, ComputerSessionV1,
    guest_image::GuestImageOverlayV1,
};

use super::super::registry::MachineOwnerRecordV1;
use super::LocalKvmComputerBackend;

impl LocalKvmComputerBackend {
    pub(super) fn check_resource_budget(
        &self,
        session: &ComputerSessionV1,
        overlay: &GuestImageOverlayV1,
    ) -> Result<(), Error> {
        let budget = &session.policy.resource_budget;
        let vcpus = budget.cpu_millis.div_ceil(1_000);
        if vcpus > self.limits.max_vcpus as u32
            || budget.memory_bytes > self.limits.max_memory_bytes
            || budget.disk_bytes > self.limits.max_overlay_bytes
            || budget.time_limit_ms > 24 * 60 * 60 * 1_000
            || overlay.virtual_size_bytes() > budget.disk_bytes
        {
            return Err(Error::ResourceLimit);
        }
        Ok(())
    }
}

pub(super) fn verify_record(
    owner: &MachineOwnerRecordV1,
    session: &ComputerSessionV1,
    handle: &Handle,
) -> Result<(), Error> {
    if owner.schema_version != 1
        || owner.owner_id != session.identity.user_id
        || owner.project_id != session.identity.project_id
        || owner.task_id != session.identity.task_id
        || owner.session_id != handle.session_id
        || owner.machine_id != handle.machine_id
        || owner.lease_id != handle.lease_id
        || owner.lease_generation != handle.lease_generation
        || owner.cancellation_generation != session.cancellation_generation
        || owner.policy_fingerprint != policy_fingerprint(session)?
        || owner.image_digest != session.image_digest
        || owner.image_manifest_digest != session.image_manifest_digest
        || owner.image_signer_key_id != session.image_signer_key_id
    {
        return Err(Error::OwnershipMismatch);
    }
    Ok(())
}

pub(super) fn target_machine_id<'a>(
    session: &'a ComputerSessionV1,
    remote_worker_id: Option<&str>,
) -> Result<&'a str, Error> {
    match &session.target {
        crate::computer_use::ComputerTargetV1::RemoteVmWorker {
            worker_id,
            machine_id,
            ..
        } if remote_worker_id == Some(worker_id.as_str()) => Ok(machine_id),
        crate::computer_use::ComputerTargetV1::RemoteVmWorker { .. } => {
            Err(Error::UnsupportedTarget)
        }
        target if remote_worker_id.is_none() => {
            target.local_machine_id().ok_or(Error::UnsupportedTarget)
        }
        _ => Err(Error::UnsupportedTarget),
    }
}

pub(super) fn policy_fingerprint(session: &ComputerSessionV1) -> Result<String, Error> {
    let encoded = serde_json::to_vec(&session.policy).map_err(|_| Error::InvalidSession)?;
    Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
}
