//! Find-folder recovery is relocation of the original checkout, not import of
//! an arbitrary backup. Filesystem identity deliberately rejects copied trees.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::core::managed_workspace) struct RelocationIdentity {
    root: (u64, u64),
    git: (u64, u64),
}

impl RelocationIdentity {
    pub(in crate::core::managed_workspace) fn capture(path: &Path) -> anyhow::Result<Self> {
        let output = git::run(Some(path), &["rev-parse", "--absolute-git-dir"])?;
        let git_dir = PathBuf::from(String::from_utf8(output.stdout)?.trim());
        Ok(Self {
            root: directory_identity(path)?,
            git: directory_identity(&git_dir)?,
        })
    }
}

#[cfg(unix)]
fn directory_identity(path: &Path) -> anyhow::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir(),
        "relocation identity requires a directory"
    );
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn directory_identity(_path: &Path) -> anyhow::Result<(u64, u64)> {
    anyhow::bail!(
        "safe folder relocation is not supported on this platform; restore the original location"
    )
}
