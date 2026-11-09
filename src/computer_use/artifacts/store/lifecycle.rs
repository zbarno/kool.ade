use super::file_flows::manifest_matches_grant;
use super::filesystem::*;
use super::{
    ARTIFACTS, CLIPBOARD, ComputerArtifactBridgeErrorV1, ComputerArtifactBridgeV1, GRANTS,
};
use crate::computer_use::ComputerIdentityV1;
use crate::computer_use::artifacts::{
    ComputerArtifactManifestV1 as Manifest, ComputerClipboardGrantV1 as ClipboardGrant,
    ComputerTransferGrantV1 as Grant,
};

impl ComputerArtifactBridgeV1 {
    /// Enumerate only non-expired artifacts belonging to the exact local task/session.
    pub fn list_for_identity(
        &self,
        identity: &ComputerIdentityV1,
        now_unix_ms: u64,
    ) -> Result<Vec<Manifest>, ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let files = self
                .store
                .list_files(ARTIFACTS)
                .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
            let mut manifests = Vec::new();
            for file in files {
                let manifest = self.read_json::<Manifest>(&format!("{ARTIFACTS}/{}", file.name))?;
                let grant = self.read_grant(&manifest.grant_id)?;
                if manifest.validate().is_ok()
                    && manifest_matches_grant(&manifest, &grant)
                    && manifest.identity == *identity
                    && manifest.expires_at_unix_ms > now_unix_ms
                    && self.is_active(&manifest.grant_id)?
                {
                    manifests.push(manifest);
                }
            }
            Ok(manifests)
        })
    }

    /// Remove expired/revoked records and blob bytes; safe to call repeatedly.
    pub fn cleanup_expired(
        &self,
        now_unix_ms: u64,
    ) -> Result<usize, ComputerArtifactBridgeErrorV1> {
        self.with_lock(|| {
            let files = self
                .store
                .list_files(ARTIFACTS)
                .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
            let mut removed = 0;
            for file in files {
                let path = format!("{ARTIFACTS}/{}", file.name);
                let manifest = self.read_json::<Manifest>(&path)?;
                if manifest.expires_at_unix_ms <= now_unix_ms
                    || !self.is_active(&manifest.grant_id)?
                {
                    let _ = self.store.remove(blob_path(&manifest.artifact_id));
                    let _ = self.store.remove(path);
                    removed += 1;
                }
            }
            for grant in self
                .store
                .list_files(GRANTS)
                .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?
            {
                if grant.name.ends_with(".json") {
                    let id = grant.name.trim_end_matches(".json");
                    let record = self.read_json::<Grant>(&grant_path(id))?;
                    if record.expires_at_unix_ms <= now_unix_ms {
                        let _ = self.store.remove(grant_path(id));
                        let _ = self.store.remove(revoked_path(id));
                        let _ = self.store.remove(transfer_used_path(id));
                    }
                }
            }
            for grant in self
                .store
                .list_files(CLIPBOARD)
                .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?
            {
                if grant.name.ends_with(".json") {
                    let id = grant.name.trim_end_matches(".json");
                    let record = self.read_json::<ClipboardGrant>(&clipboard_path(id))?;
                    if record.expires_at_unix_ms <= now_unix_ms {
                        let _ = self.store.remove(clipboard_path(id));
                        let _ = self.store.remove(clipboard_used_path(id));
                        let _ = self.store.remove(revoked_path(id));
                    }
                }
            }
            Ok(removed)
        })
    }
}
