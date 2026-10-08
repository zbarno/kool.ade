use super::Sandbox;
use std::{fs, path::Path};

pub(super) fn mount(
    sandbox: &mut Sandbox,
    source: &Path,
    snapshot_root: &Path,
) -> anyhow::Result<()> {
    let source = source.canonicalize()?;
    let cacache = cache_directory(&source.join("_cacache"), &source)?;
    let content = cache_directory(&cacache.join("content-v2"), &cacache)?;
    let snapshot_parent = snapshot_root
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Prepared npm snapshot root has no parent"))?
        .canonicalize()?;
    let snapshots = cache_directory(snapshot_root, &snapshot_parent)?;
    match fs::symlink_metadata(snapshots.join("current")) {
        Ok(current) => anyhow::ensure!(
            current.is_file() && !current.file_type().is_symlink(),
            "Prepared npm cache snapshot pointer is not a regular file"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::harness::publish_npm_cache_index_snapshot(&source, &snapshots)?;
        }
        Err(error) => return Err(error.into()),
    }
    let destination = "/tmp/koolade-home/.npm-prepared";
    let sandbox_cacache = format!("{destination}/_cacache");
    sandbox.args.extend([
        "--dir".into(),
        destination.into(),
        "--dir".into(),
        sandbox_cacache.clone(),
        "--ro-bind".into(),
        content.to_string_lossy().into_owned(),
        format!("{sandbox_cacache}/content-v2"),
        "--ro-bind".into(),
        snapshots.to_string_lossy().into_owned(),
        format!("{sandbox_cacache}/index-source-v5"),
        "--dir".into(),
        format!("{sandbox_cacache}/index-v5"),
        "--dir".into(),
        format!("{sandbox_cacache}/tmp"),
        "--setenv".into(),
        "npm_config_cache".into(),
        destination.into(),
    ]);
    Ok(())
}

fn cache_directory(path: &Path, root: &Path) -> anyhow::Result<std::path::PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Prepared npm cache contains a symlink or non-directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
            super::set_mode(path, 0o700)?;
        }
        Err(error) => return Err(error.into()),
    }
    let canonical = path.canonicalize()?;
    anyhow::ensure!(
        canonical.starts_with(root.canonicalize()?) && canonical.is_dir(),
        "Prepared npm cache directory escaped its application-owned root"
    );
    Ok(canonical)
}
