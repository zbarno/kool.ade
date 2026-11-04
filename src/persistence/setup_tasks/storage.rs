//! Private platform-local setup task state.

pub(super) mod permissions;
mod root;

#[cfg(test)]
mod tests;

pub(super) fn verify_private_acl(path: &std::path::Path) -> anyhow::Result<()> {
    permissions::verify_private_acl(path)
}

#[cfg(not(test))]
pub(super) fn path() -> anyhow::Result<std::path::PathBuf> {
    Ok(path_for_app_data_root(&root::resolve()?))
}

#[cfg(not(test))]
pub(super) fn state_dir() -> anyhow::Result<std::path::PathBuf> {
    path()?
        .parent()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("machine setup task state has no parent directory"))
}

fn path_for_app_data_root(root: &std::path::Path) -> std::path::PathBuf {
    root.join("machine-setup").join("tasks.json")
}
