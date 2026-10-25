//! Platform-local setup task state and its one-time legacy location migration.

mod migration;
pub(super) mod permissions;
mod root;

#[cfg(test)]
mod tests;

pub(super) fn verify_private_acl(path: &std::path::Path) -> anyhow::Result<()> {
    permissions::verify_private_acl(path)
}

#[cfg(not(test))]
pub(super) fn path() -> anyhow::Result<std::path::PathBuf> {
    path_for_roots(&root::resolve()?)
}

#[cfg(not(test))]
pub(super) fn state_dir() -> anyhow::Result<std::path::PathBuf> {
    state_dir_for_roots(&root::resolve()?)
}

fn path_for_roots(roots: &root::Roots) -> anyhow::Result<std::path::PathBuf> {
    if roots.legacy_state_root.is_none() {
        return Ok(roots.app_data_root.join("machine-setup").join("tasks.json"));
    }
    migration::resolve(roots)
}

fn state_dir_for_roots(roots: &root::Roots) -> anyhow::Result<std::path::PathBuf> {
    let path = path_for_roots(roots)?;
    path.parent()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("machine setup task state has no parent directory"))
}
