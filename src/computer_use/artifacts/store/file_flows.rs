use super::filesystem::{path_contains_private_component, read_owned_task_file};
use super::{ComputerArtifactBridgeErrorV1, ComputerArtifactBridgeV1};
use crate::computer_use::ComputerIdentityV1;
use crate::computer_use::apps::{StagedTaskApplicationV1, StagedTaskArtifactContentV1};
use crate::computer_use::artifacts::{
    ComputerArtifactFlowV1 as Flow, ComputerArtifactManifestV1 as Manifest,
    ComputerArtifactReceiptV1 as Receipt, ComputerTransferGrantV1 as Grant,
    validation::{self, sha256, validate_content},
};
use std::path::Path;

impl ComputerArtifactBridgeV1 {
    pub(super) fn authorize_remote_guest_send(
        &self,
        artifact_id: &str,
        identity: &ComputerIdentityV1,
        current_task_revision: &str,
        lease_generation: u64,
        approval_generation: u64,
        now_unix_ms: u64,
    ) -> Result<(), ComputerArtifactBridgeErrorV1> {
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
            Ok(())
        })
    }

    #[allow(clippy::too_many_arguments)] // Each transfer authority fence is independently checked.
    pub(super) fn authorize_remote_guest_receive(
        &self,
        grant_id: &str,
        identity: &ComputerIdentityV1,
        current_task_revision: &str,
        lease_generation: u64,
        approval_generation: u64,
        display_name: &str,
        now_unix_ms: u64,
    ) -> Result<u64, ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let grant = self.active_grant(grant_id, identity, now_unix_ms)?;
            self.ensure_unused(grant_id)?;
            let authorized_name = Path::new(&grant.relative_path)
                .file_name()
                .and_then(|part| part.to_str());
            if !matches!(
                grant.flow,
                Flow::GuestArtifactToTask | Flow::UserDownloadFromGuest
            ) || grant.task_revision != current_task_revision
                || grant.lease_generation != lease_generation
                || grant.approval_generation != approval_generation
                || !validation::valid_display_name(display_name)
                || authorized_name != Some(display_name)
                || path_contains_private_component(&grant.relative_path)
            {
                return Err(ComputerArtifactBridgeErrorV1::StaleRevision);
            }
            Ok(grant.max_bytes)
        })
    }

    #[cfg(any(target_os = "linux", test))]
    /// Persist one exact file from a controller-verified QA build manifest.
    /// The in-memory bytes must match both the one-use grant and its digest.
    #[allow(clippy::too_many_arguments)] // Each grant fence is checked independently.
    pub(crate) fn stage_approved_task_build_bytes(
        &self,
        grant_id: &str,
        identity: &ComputerIdentityV1,
        current_task_revision: &str,
        relative_path: &str,
        expected_sha256: &str,
        lease_generation: u64,
        approval_generation: u64,
        approval_receipt_sha256: &str,
        bytes: &[u8],
        now_unix_ms: u64,
    ) -> Result<Receipt, ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let grant = self.active_grant(grant_id, identity, now_unix_ms)?;
            self.ensure_unused(grant_id)?;
            if grant.flow != Flow::TaskArtifactToGuest
                || !grant.user_approved
                || grant.approval_receipt_sha256.as_deref() != Some(approval_receipt_sha256)
                || grant.task_revision != current_task_revision
                || grant.relative_path != relative_path
                || grant.lease_generation != lease_generation
                || grant.approval_generation != approval_generation
                || bytes.len() as u64 != grant.max_bytes
                || sha256(bytes) != expected_sha256
            {
                return Err(ComputerArtifactBridgeErrorV1::StaleRevision);
            }
            let receipt = self.store_content(&grant, relative_path, bytes, now_unix_ms)?;
            self.mark_used(grant_id, &receipt.artifact_id)?;
            Ok(receipt)
        })
    }

    /// Atomically load a sorted staged-app receipt set only while every
    /// one-use task-to-guest grant remains bound to this identity/revision and
    /// the active lease and approval generations.
    pub fn load_staged_task_application_for_guest(
        &self,
        application: &StagedTaskApplicationV1,
        identity: &ComputerIdentityV1,
        current_task_revision: &str,
        lease_generation: u64,
        approval_generation: u64,
        now_unix_ms: u64,
    ) -> Result<Vec<StagedTaskArtifactContentV1>, ComputerArtifactBridgeErrorV1> {
        application
            .validate_at(now_unix_ms)
            .map_err(|_| ComputerArtifactBridgeErrorV1::Invalid)?;
        if application.task_revision != current_task_revision {
            return Err(ComputerArtifactBridgeErrorV1::StaleRevision);
        }
        self.with_lock(|| {
            let mut contents = Vec::with_capacity(application.artifacts.len());
            for artifact in &application.artifacts {
                let manifest = self.load_manifest(&artifact.receipt.artifact_id)?;
                let grant =
                    self.active_grant(&artifact.transfer_grant_id, identity, now_unix_ms)?;
                if manifest.grant_id != artifact.transfer_grant_id
                    || manifest.flow != Flow::TaskArtifactToGuest
                    || !grant.user_approved
                    || grant.approval_receipt_sha256.as_deref()
                        != Some(application.build_approval_sha256.as_str())
                    || manifest.approval_receipt_sha256.as_deref()
                        != Some(application.build_approval_sha256.as_str())
                    || !manifest_matches_grant(&manifest, &grant)
                    || manifest.identity != *identity
                    || manifest.task_revision != current_task_revision
                    || manifest.lease_generation != lease_generation
                    || manifest.approval_generation != approval_generation
                    || manifest.relative_path != artifact.relative_path
                    || manifest.artifact_id != artifact.receipt.artifact_id
                    || manifest.display_name != artifact.receipt.display_name
                    || manifest.size_bytes != artifact.receipt.size_bytes
                    || manifest.sha256 != artifact.receipt.sha256
                    || manifest.media_type != artifact.receipt.media_type
                    || manifest.expires_at_unix_ms != artifact.receipt.expires_at_unix_ms
                    || grant.lease_generation != lease_generation
                    || grant.approval_generation != approval_generation
                {
                    return Err(ComputerArtifactBridgeErrorV1::StaleRevision);
                }
                let bytes = self.load_bytes(&manifest)?;
                contents.push(StagedTaskArtifactContentV1 {
                    artifact: artifact.clone(),
                    bytes,
                });
            }
            application
                .validate_contents(&contents)
                .map_err(|_| ComputerArtifactBridgeErrorV1::Invalid)?;
            Ok(contents)
        })
    }

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
        && manifest.approval_receipt_sha256 == grant.approval_receipt_sha256
}
