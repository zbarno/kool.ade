//! Private, source-bound runtime cache receipts and cross-process build lock.
//!
//! Receipts are durable diagnostics, never authorization. A cache hit still
//! requires locked-image inspection and an isolated toolchain smoke run.

use std::{fs, io, path::{Path, PathBuf}, time::{SystemTime, UNIX_EPOCH}};

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
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::symlink_metadata(&root)?;
        if !metadata.file_type().is_dir() || metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Composite cache directory is not private"));
        }
    }
    Ok(root)
}

pub(crate) fn acquire_process_lock() -> io::Result<fs::File> {
    let root = private_root()?;
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
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Composite cache lock is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "Composite cache lock has unsafe permissions"));
        }
    }
    file.lock_exclusive()?;
    Ok(file)
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
        let name = format!("{}.json", &self.recipe_digest[7..]);
        let target = root.join(name);
        // Never replace a symlink or an unexpected object in app-owned state.
        if let Ok(metadata) = fs::symlink_metadata(&target) {
            anyhow::ensure!(metadata.file_type().is_file(), "Composite cache receipt is not a regular file");
        }
        let mut temporary = tempfile::NamedTempFile::new_in(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary.as_file().set_permissions(fs::Permissions::from_mode(0o600))?;
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
    fn cache_receipts_are_content_addressed_and_private() {
        let temp = tempfile::tempdir().unwrap();
        let lock = ImageLock::bundled().unwrap();
        let ProjectRuntimeImagePlan::Composition(plan) = lock
            .plan_project_runtime("amd64", &["cargo".into(), "node".into()])
            .unwrap()
        else { panic!("expected composite") };
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
        let path = temp.path().join(format!("{}.json", &plan.recipe_sha256[7..]));
        let stored: CompositionReceipt =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
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
