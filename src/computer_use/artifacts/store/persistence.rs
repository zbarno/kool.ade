use super::filesystem::*;
use super::{ARTIFACTS, BASE, BYTES, ComputerArtifactBridgeErrorV1, ComputerArtifactBridgeV1};
use crate::computer_use::ComputerIdentityV1;
use crate::computer_use::artifacts::{
    COMPUTER_ARTIFACT_MAX_BYTES_V1, COMPUTER_ARTIFACT_STORE_QUOTA_BYTES_V1,
    ComputerArtifactManifestV1 as Manifest, ComputerArtifactReceiptV1 as Receipt,
    ComputerTransferGrantV1 as Grant,
    validation::{self, casefold_equal, sha256, validate_content},
};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};

impl ComputerArtifactBridgeV1 {
    pub(super) fn create_private(
        &self,
        relative: &str,
        bytes: &[u8],
    ) -> Result<(), ComputerArtifactBridgeErrorV1> {
        self.store
            .atomic_create(relative, bytes)
            .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
        secure_private_file(
            &self.store.local_root(),
            &self.store.local_root().join(relative),
        )
    }

    pub(super) fn store_content(
        &self,
        grant: &Grant,
        path: &str,
        bytes: &[u8],
        now: u64,
    ) -> Result<Receipt, ComputerArtifactBridgeErrorV1> {
        let name = Path::new(path)
            .file_name()
            .and_then(|part| part.to_str())
            .ok_or(ComputerArtifactBridgeErrorV1::Invalid)?;
        let media = if grant.flow
            == crate::computer_use::artifacts::ComputerArtifactFlowV1::TaskArtifactToGuest
        {
            validation::validate_task_build_content(name, bytes)
        } else {
            validate_content(name, bytes)
        }
        .map_err(|_| ComputerArtifactBridgeErrorV1::Invalid)?;
        self.store_content_with_media(grant, name, bytes, &media, now)
    }

    pub(super) fn store_content_with_media(
        &self,
        grant: &Grant,
        name: &str,
        bytes: &[u8],
        media: &str,
        now: u64,
    ) -> Result<Receipt, ComputerArtifactBridgeErrorV1> {
        if bytes.is_empty()
            || bytes.len() as u64 > grant.max_bytes
            || bytes.len() as u64 > COMPUTER_ARTIFACT_MAX_BYTES_V1
        {
            return Err(ComputerArtifactBridgeErrorV1::Quota);
        }
        self.ensure_quota(bytes.len() as u64)?;
        let artifact_id = uuid::Uuid::new_v4().to_string();
        let display_name = name.to_owned();
        let authorized_name = Path::new(&grant.relative_path)
            .file_name()
            .and_then(|part| part.to_str());
        if !validation::valid_relative_path(&grant.relative_path)
            || !validation::valid_display_name(&display_name)
            || authorized_name != Some(name)
        {
            return Err(ComputerArtifactBridgeErrorV1::Invalid);
        }
        let manifest = Manifest::new(
            grant,
            artifact_id.clone(),
            display_name,
            bytes.len() as u64,
            sha256(bytes),
            media.to_owned(),
            now,
        );
        if manifest.validate().is_err() {
            return Err(ComputerArtifactBridgeErrorV1::Invalid);
        }
        self.ensure_no_case_collision(&manifest)?;
        self.create_private(&blob_path(&artifact_id), bytes)?;
        if self
            .create_private(
                &manifest_path(&artifact_id),
                &serde_json::to_vec(&manifest)
                    .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?,
            )
            .is_err()
        {
            let _ = self.store.remove(blob_path(&artifact_id));
            return Err(ComputerArtifactBridgeErrorV1::Storage);
        }
        Ok(Receipt {
            artifact_id,
            display_name: manifest.display_name,
            size_bytes: manifest.size_bytes,
            sha256: manifest.sha256,
            media_type: manifest.media_type,
            expires_at_unix_ms: manifest.expires_at_unix_ms,
        })
    }

    pub(super) fn ensure_quota(&self, incoming: u64) -> Result<(), ComputerArtifactBridgeErrorV1> {
        let files = self
            .store
            .list_files(BYTES)
            .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
        let total = files
            .iter()
            .fold(0_u64, |sum, file| sum.saturating_add(file.bytes));
        if total.saturating_add(incoming) > COMPUTER_ARTIFACT_STORE_QUOTA_BYTES_V1 {
            return Err(ComputerArtifactBridgeErrorV1::Quota);
        }
        Ok(())
    }

    pub(super) fn ensure_no_case_collision(
        &self,
        candidate: &Manifest,
    ) -> Result<(), ComputerArtifactBridgeErrorV1> {
        let files = self
            .store
            .list_files(ARTIFACTS)
            .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
        for file in files {
            let existing = self.read_json::<Manifest>(&format!("{ARTIFACTS}/{}", file.name))?;
            if existing.identity == candidate.identity
                && existing.task_revision == candidate.task_revision
                && casefold_equal(&existing.relative_path, &candidate.relative_path)
                && existing.relative_path != candidate.relative_path
            {
                return Err(ComputerArtifactBridgeErrorV1::Invalid);
            }
        }
        Ok(())
    }

    pub(super) fn active_grant(
        &self,
        id: &str,
        identity: &ComputerIdentityV1,
        now: u64,
    ) -> Result<Grant, ComputerArtifactBridgeErrorV1> {
        let grant = self.read_grant(id)?;
        grant
            .validate(now)
            .map_err(|_| ComputerArtifactBridgeErrorV1::Expired)?;
        if &grant.identity != identity || !self.is_active(id)? {
            return Err(ComputerArtifactBridgeErrorV1::Denied);
        }
        Ok(grant)
    }

    pub(super) fn ensure_unused(&self, id: &str) -> Result<(), ComputerArtifactBridgeErrorV1> {
        match self.store.read(transfer_used_path(id)) {
            Ok(_) => Err(ComputerArtifactBridgeErrorV1::Expired),
            Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(())
            }
            Err(_) => Err(ComputerArtifactBridgeErrorV1::Storage),
        }
    }

    pub(super) fn mark_used(
        &self,
        grant_id: &str,
        artifact_id: &str,
    ) -> Result<(), ComputerArtifactBridgeErrorV1> {
        if self
            .create_private(&transfer_used_path(grant_id), artifact_id.as_bytes())
            .is_err()
        {
            let _ = self.store.remove(blob_path(artifact_id));
            let _ = self.store.remove(manifest_path(artifact_id));
            return Err(ComputerArtifactBridgeErrorV1::Expired);
        }
        Ok(())
    }

    pub(super) fn read_grant(&self, id: &str) -> Result<Grant, ComputerArtifactBridgeErrorV1> {
        if !canonical_uuid(id) {
            return Err(ComputerArtifactBridgeErrorV1::Invalid);
        }
        let grant = self.read_json::<Grant>(&grant_path(id))?;
        if grant.grant_id != id {
            return Err(ComputerArtifactBridgeErrorV1::Invalid);
        }
        Ok(grant)
    }

    pub(super) fn load_manifest(
        &self,
        id: &str,
    ) -> Result<Manifest, ComputerArtifactBridgeErrorV1> {
        if !canonical_uuid(id) {
            return Err(ComputerArtifactBridgeErrorV1::Invalid);
        }
        let manifest = self.read_json::<Manifest>(&manifest_path(id))?;
        if manifest.artifact_id != id || manifest.validate().is_err() {
            return Err(ComputerArtifactBridgeErrorV1::Invalid);
        }
        Ok(manifest)
    }

    pub(super) fn load_bytes(
        &self,
        manifest: &Manifest,
    ) -> Result<Vec<u8>, ComputerArtifactBridgeErrorV1> {
        secure_private_file(
            &self.store.local_root(),
            &self
                .store
                .local_root()
                .join(blob_path(&manifest.artifact_id)),
        )?;
        let bytes = self
            .store
            .read(blob_path(&manifest.artifact_id))
            .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
        if bytes.len() as u64 != manifest.size_bytes || sha256(&bytes) != manifest.sha256 {
            return Err(ComputerArtifactBridgeErrorV1::Invalid);
        }
        Ok(bytes)
    }

    pub(super) fn is_active(&self, id: &str) -> Result<bool, ComputerArtifactBridgeErrorV1> {
        self.store
            .read(revoked_path(id))
            .map(|_| false)
            .or_else(|error| match error {
                crate::artifacts::planning_store::StoreError::Io { source, .. }
                    if source.kind() == std::io::ErrorKind::NotFound =>
                {
                    Ok(true)
                }
                _ => Err(ComputerArtifactBridgeErrorV1::Storage),
            })
    }

    pub(super) fn read_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, ComputerArtifactBridgeErrorV1> {
        secure_private_file(
            &self.store.local_root(),
            &self.store.local_root().join(path),
        )?;
        let bytes = self
            .store
            .read(path)
            .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
        serde_json::from_slice(&bytes).map_err(|_| ComputerArtifactBridgeErrorV1::Invalid)
    }

    pub(super) fn with_lock<T>(
        &self,
        run: impl FnOnce() -> Result<T, ComputerArtifactBridgeErrorV1>,
    ) -> Result<T, ComputerArtifactBridgeErrorV1> {
        use fs2::FileExt;
        let lock_path = self.store.local_root().join(BASE).join("bridge.lock");
        ensure_private_store_directory(
            &self.store.local_root(),
            lock_path
                .parent()
                .ok_or(ComputerArtifactBridgeErrorV1::Storage)?,
        )?;
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options
            .open(&lock_path)
            .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
        lock.lock_exclusive()
            .map_err(|_| ComputerArtifactBridgeErrorV1::Storage)?;
        secure_private_file(&self.store.local_root(), &lock_path)?;
        let result = run();
        let unlock = File::unlock(&lock);
        if unlock.is_err() {
            return Err(ComputerArtifactBridgeErrorV1::Storage);
        }
        result
    }
}
