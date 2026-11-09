use std::path::Path;

use super::file_flows::manifest_matches_grant;
use super::filesystem::*;
use super::{ARTIFACTS, ComputerArtifactBridgeErrorV1, ComputerArtifactBridgeV1};
use crate::artifacts::planning_store::PlanningStore;
use crate::computer_use::ComputerIdentityV1;
use crate::computer_use::artifacts::{
    COMPUTER_CLIPBOARD_MAX_BYTES_V1, ComputerArtifactFlowV1 as Flow,
    ComputerArtifactManifestV1 as Manifest, ComputerClipboardDirectionV1 as ClipboardDirection,
    ComputerClipboardGrantV1 as ClipboardGrant, ComputerTransferGrantRequestV1 as GrantRequest,
    ComputerTransferGrantV1 as Grant, contract::new_clipboard_grant,
};

impl ComputerArtifactBridgeV1 {
    pub fn new(store: PlanningStore) -> Self {
        Self { store }
    }

    /// Issue a narrowly scoped, expiring grant. This app-side API must only be
    /// called after the task controller or explicit user approval has decided.
    pub fn issue_grant(
        &self,
        request: GrantRequest,
    ) -> Result<Grant, ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            if request.flow == Flow::UserApprovedFileToPersonalGuest {
                return Err(ComputerArtifactBridgeErrorV1::Denied);
            }
            let grant =
                Grant::from_request(request).map_err(|_| ComputerArtifactBridgeErrorV1::Invalid)?;
            grant
                .validate(grant.issued_at_unix_ms)
                .map_err(|_| ComputerArtifactBridgeErrorV1::Invalid)?;
            self.create_private(
                &grant_path(&grant.grant_id),
                &serde_json::to_vec(&grant).map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?,
            )?;
            Ok(grant)
        })
    }

    /// Create a one-use clipboard permission. Clipboard bytes are passed only
    /// in memory and are never written to the operational store or model logs.
    #[allow(clippy::too_many_arguments)] // Explicit consent and both authority generations are required.
    pub fn grant_clipboard(
        &self,
        identity: ComputerIdentityV1,
        direction: ClipboardDirection,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
        lease_generation: u64,
        approval_generation: u64,
        user_approved: bool,
    ) -> Result<ClipboardGrant, ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let grant = new_clipboard_grant(
                identity,
                direction,
                now_unix_ms,
                expires_at_unix_ms,
                lease_generation,
                approval_generation,
                user_approved,
            )
            .map_err(|_| ComputerArtifactBridgeErrorV1::Denied)?;
            self.create_private(
                &clipboard_path(&grant.grant_id),
                &serde_json::to_vec(&grant).map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?,
            )?;
            Ok(grant)
        })
    }

    /// Consume one clipboard grant after checking identity, direction and text
    /// bounds. This records metadata only, not the clipboard content.
    #[allow(clippy::too_many_arguments)] // Every authority and content check is explicit at the call site.
    pub fn consume_clipboard(
        &self,
        grant_id: &str,
        identity: &ComputerIdentityV1,
        direction: ClipboardDirection,
        content: &[u8],
        current_lease_generation: u64,
        current_approval_generation: u64,
        now_unix_ms: u64,
    ) -> Result<(), ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let grant = self.read_json::<ClipboardGrant>(&clipboard_path(grant_id))?;
            if grant.grant_id != grant_id
                || &grant.identity != identity
                || grant.direction != direction
                || !grant.user_approved
                || !grant.visible_indicator
                || grant.expires_at_unix_ms <= now_unix_ms
                || grant.issued_at_unix_ms > now_unix_ms
                || grant.lease_generation != current_lease_generation
                || grant.approval_generation != current_approval_generation
                || content.is_empty()
                || content.len() > COMPUTER_CLIPBOARD_MAX_BYTES_V1
                || std::str::from_utf8(content).is_err()
            {
                return Err(ComputerArtifactBridgeErrorV1::Denied);
            }
            if self.store.read(revoked_path(grant_id)).is_ok() {
                return Err(ComputerArtifactBridgeErrorV1::Expired);
            }
            self.create_private(clipboard_used_path(grant_id).as_str(), b"consumed")
                .map_err(|_| ComputerArtifactBridgeErrorV1::Expired)
        })
    }

    pub fn revoke_clipboard(
        &self,
        grant_id: &str,
        identity: &ComputerIdentityV1,
    ) -> Result<(), ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let grant = self.read_json::<ClipboardGrant>(&clipboard_path(grant_id))?;
            if grant.grant_id != grant_id || &grant.identity != identity {
                return Err(ComputerArtifactBridgeErrorV1::Denied);
            }
            self.create_private(revoked_path(grant_id).as_str(), b"revoked")
                .map_err(|_| ComputerArtifactBridgeErrorV1::Expired)
        })
    }

    /// Export a guest artifact to a user-selected destination with no overwrite.
    /// The path is only used by trusted UI code after an explicit save dialog.
    pub fn export_to_user(
        &self,
        artifact_id: &str,
        identity: &ComputerIdentityV1,
        destination: &Path,
        now_unix_ms: u64,
    ) -> Result<(), ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let manifest = self.load_manifest(artifact_id)?;
            let grant = self.active_grant(&manifest.grant_id, identity, now_unix_ms)?;
            if manifest.flow != Flow::UserDownloadFromGuest
                || !grant.user_approved
                || &manifest.identity != identity
                || !manifest_matches_grant(&manifest, &grant)
            {
                return Err(ComputerArtifactBridgeErrorV1::Denied);
            }
            let bytes = self.load_bytes(&manifest)?;
            create_user_export(destination, &bytes)
        })
    }

    /// Revoke a grant and delete all staged bytes created through it.
    pub fn revoke(
        &self,
        grant_id: &str,
        identity: &ComputerIdentityV1,
    ) -> Result<(), ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let grant = self.read_grant(grant_id)?;
            if &grant.identity != identity {
                return Err(ComputerArtifactBridgeErrorV1::Denied);
            }
            self.create_private(revoked_path(grant_id).as_str(), b"revoked")
                .map_err(|_| ComputerArtifactBridgeErrorV1::Expired)?;
            let manifests = self
                .store
                .list_files(ARTIFACTS)
                .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
            for entry in manifests {
                let manifest =
                    self.read_json::<Manifest>(&format!("{ARTIFACTS}/{}", entry.name))?;
                if manifest.grant_id == grant_id {
                    let _ = self.store.remove(blob_path(&manifest.artifact_id));
                }
            }
            Ok(())
        })
    }
}
