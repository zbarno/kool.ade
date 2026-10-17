use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

pub(super) fn local_root_for_workspace(workspace_id: uuid::Uuid) -> Result<PathBuf, String> {
    let data_root = user_data_root()?;
    Ok(workspace_root(&data_root, workspace_id))
}

fn workspace_root(data_root: &Path, workspace_id: uuid::Uuid) -> PathBuf {
    data_root
        .join("koolade")
        .join("runtime-qa")
        .join(workspace_id.hyphenated().to_string())
}

#[cfg(target_os = "windows")]
fn user_data_root() -> Result<PathBuf, String> {
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .map(|home| home.join("AppData/Local"))
        })
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            "the current user's local application data directory is unavailable".to_owned()
        })?;
    Ok(root)
}

#[cfg(target_os = "macos")]
fn user_data_root() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| "the current user's home directory is unavailable".to_owned())?;
    Ok(home.join("Library/Application Support"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn user_data_root() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    {
        return Ok(root);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| "the current user's data directory is unavailable".to_owned())?;
    Ok(home.join(".local/share"))
}

#[cfg(not(any(unix, target_os = "windows")))]
fn user_data_root() -> Result<PathBuf, String> {
    Err("the current user's local application data directory is unsupported".into())
}
