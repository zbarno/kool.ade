//! Private, source-bound runtime cache receipts and cross-process build lock.
//!
//! Receipts are durable diagnostics, never authorization. A cache hit still
//! requires locked-image inspection and an isolated toolchain smoke run.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{CompositionCandidate, SelectedImage, is_digest};

const CACHE_VERSION: u16 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CompositionReceipt {
    pub schema_version: u16,
    pub recipe_digest: String,
    pub image_id: String,
    pub base_digest: String,
    pub platform: String,
    pub component_digests: Vec<String>,
    pub tool_versions: std::collections::BTreeMap<String, String>,
    /// Fingerprint, not raw Docker daemon ID or endpoint path.
    pub daemon_fingerprint: String,
    pub verified_at_unix_ms: u64,
}

fn private_root() -> io::Result<PathBuf> {
    let root = crate::persistence::state_root().join("composite-runtimes");
    fs::create_dir_all(&root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::symlink_metadata(&root)?;
        if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Composite cache directory is not owned by the current user",
            ));
        }
        // Newly created directories may inherit an unrestrictive umask.
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    }
    Ok(root)
}

pub(crate) fn try_acquire_process_lock() -> io::Result<Option<fs::File>> {
    let root = private_root()?;
    try_acquire_process_lock_in(&root)
}

fn try_acquire_process_lock_in(root: &Path) -> io::Result<Option<fs::File>> {
    let path = root.join("cache.lock");
    let mut opts = fs::OpenOptions::new();
    opts.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = opts.open(&path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Composite cache lock is not a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Composite cache lock has unsafe permissions",
            ));
        }
    }
    match FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(error),
    }
}

fn receipt_path(root: &Path, recipe: &str) -> Option<PathBuf> {
    if !is_digest(recipe) {
        return None;
    }
    Some(root.join(format!("{}.json", &recipe[7..])))
}

impl CompositionReceipt {
    pub(crate) fn verified(
        plan: &CompositionCandidate,
        image: &SelectedImage,
        endpoint_fingerprint: &str,
        daemon_id: &str,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            is_digest(&image.digest)
                && image.digest == image.reference
                && plan.platform == image.platform
                && is_digest(&plan.recipe_sha256),
            "Composite runtime receipt is not bound to a verified image"
        );
        let daemon_fingerprint = format!(
            "sha256:{:x}",
            Sha256::digest(format!("{endpoint_fingerprint}:{daemon_id}").as_bytes())
        );
        Ok(Self {
            schema_version: CACHE_VERSION,
            recipe_digest: plan.recipe_sha256.clone(),
            image_id: image.digest.clone(),
            base_digest: plan.foundation.digest.clone(),
            platform: plan.platform.clone(),
            component_digests: std::iter::once(&plan.foundation)
                .chain(plan.components.iter())
                .map(|image| image.digest.clone())
                .collect(),
            tool_versions: image.tool_versions.clone(),
            daemon_fingerprint,
            verified_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)?
                .as_millis()
                .min(u64::MAX as u128) as u64,
        })
    }

    /// A persisted receipt is an LRU hint, not proof that an image may be
    /// executed. The caller must still inspect the daemon and smoke-test tools.
    pub(crate) fn last_verified_at(recipe: &str, image_id: &str) -> Option<u64> {
        let root = private_root().ok()?;
        Self::last_verified_at_in(&root, recipe, image_id)
    }

    fn last_verified_at_in(root: &Path, recipe: &str, image_id: &str) -> Option<u64> {
        let file = receipt_path(root, recipe)?;
        let metadata = fs::symlink_metadata(&file).ok()?;
        if !metadata.file_type().is_file() || metadata.len() > 16 * 1024 {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return None;
            }
        }
        let receipt: Self = serde_json::from_slice(&fs::read(file).ok()?).ok()?;
        (receipt.schema_version == CACHE_VERSION
            && receipt.recipe_digest == recipe
            && receipt.image_id == image_id)
            .then_some(receipt.verified_at_unix_ms)
    }

    /// Forget a receipt only after Docker confirms that the precisely
    /// identified app-owned cache tag was removed. A concurrent or corrupted
    /// receipt must never be used to authorize deletion.
    pub(crate) fn forget_if_matches(recipe: &str, image_id: &str) -> anyhow::Result<()> {
        let root = private_root()?;
        Self::forget_if_matches_in(&root, recipe, image_id)
    }

    fn forget_if_matches_in(root: &Path, recipe: &str, image_id: &str) -> anyhow::Result<()> {
        let Some(file) = receipt_path(root, recipe) else {
            anyhow::bail!("Invalid composite recipe identity");
        };
        let metadata = match fs::symlink_metadata(&file) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            metadata.file_type().is_file() && metadata.len() <= 16 * 1024,
            "Composite cache receipt has an unsafe type or size"
        );
        let parsed: Self = serde_json::from_slice(&fs::read(&file)?)?;
        if parsed.schema_version == CACHE_VERSION
            && parsed.recipe_digest == recipe
            && parsed.image_id == image_id
        {
            fs::remove_file(file)?;
        }
        Ok(())
    }

    pub(crate) fn record(&self) -> anyhow::Result<()> {
        let root = private_root()?;
        self.record_in(&root)
    }

    fn record_in(&self, root: &Path) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == CACHE_VERSION
                && is_digest(&self.recipe_digest)
                && is_digest(&self.image_id)
                && is_digest(&self.base_digest)
                && is_digest(&self.daemon_fingerprint),
            "Invalid composite cache receipt"
        );
        let target = receipt_path(root, &self.recipe_digest)
            .ok_or_else(|| anyhow::anyhow!("Invalid composite receipt cache key"))?;
        // Never replace a symlink or an unexpected object in app-owned state.
        if let Ok(metadata) = fs::symlink_metadata(&target) {
            anyhow::ensure!(
                metadata.file_type().is_file(),
                "Composite cache receipt is not a regular file"
            );
        }
        let mut temporary = tempfile::NamedTempFile::new_in(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        use std::io::Write;
        temporary.write_all(&serde_json::to_vec_pretty(self)?)?;
        temporary.as_file().sync_all()?;
        temporary.persist(&target)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::docker::images::{ImageLock, ProjectRuntimeImagePlan};

    #[test]
    fn cache_lock_is_nonblocking_exclusive_and_released_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let first = try_acquire_process_lock_in(temp.path())
            .unwrap()
            .expect("first process acquires cache lock");
        assert!(
            try_acquire_process_lock_in(temp.path()).unwrap().is_none(),
            "second build must defer without blocking the async executor"
        );
        drop(first);
        assert!(try_acquire_process_lock_in(temp.path()).unwrap().is_some());
    }

    #[test]
    fn receipt_evidence_is_read_only_and_wrong_identity_never_evicts_it() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let receipt = CompositionReceipt {
            schema_version: CACHE_VERSION,
            recipe_digest: format!("sha256:{}", "a".repeat(64)),
            image_id: format!("sha256:{}", "b".repeat(64)),
            base_digest: format!("sha256:{}", "c".repeat(64)),
            platform: "linux/amd64".into(),
            component_digests: Vec::new(),
            tool_versions: Default::default(),
            daemon_fingerprint: format!("sha256:{}", "d".repeat(64)),
            verified_at_unix_ms: 42,
        };
        receipt.record_in(root).unwrap();
        let file = receipt_path(root, &receipt.recipe_digest).unwrap();
        assert!(file.exists());
        // The optional lookup only ever returns an LRU hint.
        assert_eq!(
            serde_json::from_slice::<CompositionReceipt>(&fs::read(&file).unwrap())
                .unwrap()
                .verified_at_unix_ms,
            42
        );
        assert!(receipt_path(root, "../../other").is_none());
        let mismatched = format!("sha256:{}", "e".repeat(64));
        assert_ne!(mismatched, receipt.image_id);
        assert_eq!(
            CompositionReceipt::last_verified_at_in(
                root,
                &receipt.recipe_digest,
                &receipt.image_id,
            ),
            Some(42)
        );
        CompositionReceipt::forget_if_matches_in(root, &receipt.recipe_digest, &mismatched)
            .unwrap();
        assert!(file.exists(), "wrong image ID must not delete the receipt");

        fs::write(&file, b"corrupt-not-json").unwrap();
        assert!(
            CompositionReceipt::forget_if_matches_in(
                root,
                &receipt.recipe_digest,
                &receipt.image_id,
            )
            .is_err(),
            "corrupt receipts must fail closed"
        );
        assert_eq!(
            CompositionReceipt::last_verified_at_in(
                root,
                &receipt.recipe_digest,
                &receipt.image_id,
            ),
            None
        );
        receipt.record_in(root).unwrap();
        CompositionReceipt::forget_if_matches_in(root, &receipt.recipe_digest, &receipt.image_id)
            .unwrap();
        assert!(
            !file.exists(),
            "matching, verified receipt may be forgotten"
        );
    }

    #[test]
    fn cache_receipts_are_content_addressed_and_private() {
        let temp = tempfile::tempdir().unwrap();
        let lock = ImageLock::bundled().unwrap();
        let ProjectRuntimeImagePlan::Composition(plan) = lock
            .plan_project_runtime("amd64", &["cargo".into(), "node".into()])
            .unwrap()
        else {
            panic!("expected composite")
        };
        let image = SelectedImage {
            reference: format!("sha256:{}", "a".repeat(64)),
            digest: format!("sha256:{}", "a".repeat(64)),
            platform: plan.platform.clone(),
            profile: plan.foundation.profile,
            runtime_user: "10001:10001".into(),
            tool_versions: std::collections::BTreeMap::new(),
        };
        let receipt = CompositionReceipt::verified(&plan, &image, "endpoint", "daemon").unwrap();
        receipt.record_in(temp.path()).unwrap();
        let path = temp
            .path()
            .join(format!("{}.json", &plan.recipe_sha256[7..]));
        let stored: CompositionReceipt = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(receipt, stored);
        assert!(!fs::read_to_string(&path).unwrap().contains("endpoint"));
        assert!(!fs::read_to_string(&path).unwrap().contains("daemon\""));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
        }
    }
}
