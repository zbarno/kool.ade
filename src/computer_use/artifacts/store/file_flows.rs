use super::filesystem::{path_contains_private_component, read_owned_task_file};
use super::{ComputerArtifactBridgeErrorV1, ComputerArtifactBridgeV1};
use crate::computer_use::ComputerIdentityV1;
use crate::computer_use::artifacts::{
    ComputerArtifactFlowV1 as Flow, ComputerArtifactManifestV1 as Manifest,
    ComputerArtifactReceiptV1 as Receipt, ComputerTransferGrantV1 as Grant,
    validation::{self, sha256, validate_content},
};
use std::path::Path;

impl ComputerArtifactBridgeV1 {
    /// Copy one explicitly granted task file into private staging for a guest.
    /// Source root and revision come from trusted task-controller state.
    pub fn stage_task_file(
        &self,
        grant_id: &str,
        task_root: &Path,
        current_task_revision: &str,
        identity: &ComputerIdentityV1,
        now_unix_ms: u64,
    ) -> Result<Receipt, ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let grant = self.active_grant(grant_id, identity, now_unix_ms)?;
            self.ensure_unused(grant_id)?;
            if grant.flow != Flow::TaskArtifactToGuest
                || grant.task_revision != current_task_revision
            {
                return Err(ComputerArtifactBridgeErrorV1::StaleRevision);
            }
            let source = read_owned_task_file(task_root, &grant.relative_path, grant.max_bytes)?;
            let receipt = self.store_content(&grant, &grant.relative_path, &source, now_unix_ms)?;
            self.mark_used(grant_id, &receipt.artifact_id)?;
            Ok(receipt)
        })
    }

    /// Receive one bounded guest artifact into local staging. The authenticated
    /// guest transport must call this only after session/lease verification.
    #[allow(clippy::too_many_arguments)] // Each fence and digest is independently checked.
    pub fn receive_guest_file(
        &self,
        grant_id: &str,
        identity: &ComputerIdentityV1,
        current_task_revision: &str,
        lease_generation: u64,
        approval_generation: u64,
        display_name: &str,
        bytes: &[u8],
        claimed_sha256: &str,
        now_unix_ms: u64,
    ) -> Result<Receipt, ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let grant = self.active_grant(grant_id, identity, now_unix_ms)?;
            self.ensure_unused(grant_id)?;
            if !matches!(
                grant.flow,
                Flow::GuestArtifactToTask | Flow::UserDownloadFromGuest
            ) || grant.task_revision != current_task_revision
                || grant.lease_generation != lease_generation
                || grant.approval_generation != approval_generation
            {
                return Err(ComputerArtifactBridgeErrorV1::StaleRevision);
            }
            let authorized_name = Path::new(&grant.relative_path)
                .file_name()
                .and_then(|part| part.to_str());
            if !validation::valid_display_name(display_name)
                || authorized_name != Some(display_name)
                || path_contains_private_component(&grant.relative_path)
            {
                return Err(ComputerArtifactBridgeErrorV1::Invalid);
            }
            let media_type = validate_content(display_name, bytes)
                .map_err(|_| ComputerArtifactBridgeErrorV1::Invalid)?;
            if bytes.len() as u64 > grant.max_bytes || sha256(bytes) != claimed_sha256 {
                return Err(ComputerArtifactBridgeErrorV1::Invalid);
            }
            let receipt = self.store_content_with_media(
                &grant,
                display_name,
                bytes,
                &media_type,
                now_unix_ms,
            )?;
            self.mark_used(grant_id, &receipt.artifact_id)?;
            Ok(receipt)
        })
    }

    /// Load a task file for a guest only when its exact task revision, session,
    /// lease, and approval grant are still active.
    pub fn load_for_guest(
        &self,
        artifact_id: &str,
        identity: &ComputerIdentityV1,
        current_task_revision: &str,
        lease_generation: u64,
        approval_generation: u64,
        now_unix_ms: u64,
    ) -> Result<(Manifest, Vec<u8>), ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let manifest = self.load_manifest(artifact_id)?;
            let grant = self.active_grant(&manifest.grant_id, identity, now_unix_ms)?;
            if manifest.flow != Flow::TaskArtifactToGuest
                || !manifest_matches_grant(&manifest, &grant)
                || manifest.task_revision != current_task_revision
                || manifest.lease_generation != lease_generation
                || manifest.approval_generation != approval_generation
            {
                return Err(ComputerArtifactBridgeErrorV1::StaleRevision);
            }
            let bytes = self.load_bytes(&manifest)?;
            Ok((manifest, bytes))
        })
    }

    /// Return staged bytes only to the task controller after rechecking the
    /// original task revision and active authority generations.
    pub fn load_for_task_apply(
        &self,
        artifact_id: &str,
        identity: &ComputerIdentityV1,
        current_task_revision: &str,
        lease_generation: u64,
        approval_generation: u64,
        now_unix_ms: u64,
    ) -> Result<(Manifest, Vec<u8>), ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let manifest = self.load_manifest(artifact_id)?;
            let grant = self.active_grant(&manifest.grant_id, identity, now_unix_ms)?;
            if manifest.flow != Flow::GuestArtifactToTask
                || !manifest_matches_grant(&manifest, &grant)
                || &manifest.identity != identity
                || manifest.task_revision != current_task_revision
                || manifest.lease_generation != lease_generation
                || manifest.approval_generation != approval_generation
                || grant.task_revision != current_task_revision
            {
                return Err(ComputerArtifactBridgeErrorV1::StaleRevision);
            }
            let bytes = self.load_bytes(&manifest)?;
            Ok((manifest, bytes))
        })
    }
}

pub(super) fn manifest_matches_grant(manifest: &Manifest, grant: &Grant) -> bool {
    manifest.grant_id == grant.grant_id
        && manifest.identity == grant.identity
        && manifest.flow == grant.flow
        && manifest.task_revision == grant.task_revision
        && manifest.relative_path == grant.relative_path
        && manifest.size_bytes <= grant.max_bytes
        && manifest.expires_at_unix_ms == grant.expires_at_unix_ms
        && manifest.lease_generation == grant.lease_generation
        && manifest.approval_generation == grant.approval_generation
}
