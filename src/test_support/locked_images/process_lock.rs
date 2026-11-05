use std::{
    fs::{File, OpenOptions},
    path::PathBuf,
};

use fs2::FileExt;

/// Serializes test image setup across worker processes that share a Docker
/// daemon. Fixtures use distinct private homes, so this lock lives in the
/// user's shared runtime directory instead of any fixture's application state.
pub(super) struct ImagePreparationLock(File);

impl ImagePreparationLock {
    pub(super) async fn acquire() -> anyhow::Result<Self> {
        let lock_root = std::env::var_os("XDG_RUNTIME_DIR")
            .or_else(|| std::env::var_os("HOME"))
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .ok_or_else(|| {
                anyhow::anyhow!("no shared per-user directory is available for the image lock")
            })?;
        let path = lock_root.join("koolade-test-locked-image-preparation.lock");
        tokio::task::spawn_blocking(move || {
            let mut options = OpenOptions::new();
            options.create(true).truncate(false).read(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let file = options.open(path)?;
            file.lock_exclusive()?;
            Ok(Self(file))
        })
        .await
        .map_err(|error| anyhow::anyhow!("image preparation lock worker failed: {error}"))?
    }
}

impl Drop for ImagePreparationLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}
