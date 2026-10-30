use std::{
    fs,
    path::{Path, PathBuf},
};

pub(in crate::core::managed_workspace) fn canonical_location(
    path: &Path,
) -> anyhow::Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(path.canonicalize()?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("location has no existing ancestor"))?;
            let name = path
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("invalid location"))?;
            Ok(canonical_location(parent)?.join(name))
        }
        Err(error) => Err(error.into()),
    }
}

pub(in crate::core::managed_workspace) fn outside_repository(parent: &Path) -> anyhow::Result<()> {
    let mut command = crate::core::planning_sync::standalone_git_command();
    let result = command
        .current_dir(parent)
        .env("LC_ALL", "C")
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    anyhow::ensure!(
        !result.status.success(),
        "choose a parent folder outside an existing Git repository"
    );
    let error = String::from_utf8_lossy(&result.stderr);
    anyhow::ensure!(
        result.status.code() == Some(128) && error.starts_with("fatal: not a git repository"),
        "could not safely inspect the parent repository boundary"
    );
    // Bare repos and ancestor markers are excluded independently of Git's
    // discovery diagnostics (including unreadable or redirected .git entries).
    for ancestor in parent.ancestors() {
        match fs::symlink_metadata(ancestor.join(".git")) {
            Ok(_) => anyhow::bail!("parent contains an existing repository boundary"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        anyhow::ensure!(
            !(ancestor.join("HEAD").exists() && ancestor.join("objects").is_dir()),
            "choose a parent outside a bare repository"
        );
    }
    Ok(())
}
