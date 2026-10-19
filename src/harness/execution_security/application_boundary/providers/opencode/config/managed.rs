use std::path::PathBuf;
#[cfg(any(target_os = "macos", test))]
use std::{fs, path::Path};

pub(super) fn ensure_absent() -> anyhow::Result<()> {
    let paths = config_paths().ok_or_else(|| {
        anyhow::anyhow!("OpenCode managed settings cannot be verified on this platform")
    })?;
    ensure_paths_absent(&paths)?;
    #[cfg(target_os = "macos")]
    anyhow::ensure!(
        !has_user_managed_preference(
            Path::new("/Library/Managed Preferences"),
            Path::new("/Library/Managed Preferences/ai.opencode.managed.plist"),
        )?,
        "OpenCode has administrator-managed settings; Kool.ad/e cannot verify its tool policy"
    );
    Ok(())
}

fn config_paths() -> Option<Vec<PathBuf>> {
    #[cfg(target_os = "linux")]
    {
        return Some(
            [
                "/etc/opencode/opencode.json",
                "/etc/opencode/opencode.jsonc",
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        );
    }
    #[cfg(target_os = "macos")]
    {
        return Some(vec![
            PathBuf::from("/Library/Application Support/opencode/opencode.json"),
            PathBuf::from("/Library/Application Support/opencode/opencode.jsonc"),
        ]);
    }
    #[cfg(target_os = "windows")]
    {
        let program_data = std::env::var_os("ProgramData")?;
        return Some(
            [
                PathBuf::from(&program_data).join("opencode/opencode.json"),
                PathBuf::from(program_data).join("opencode/opencode.jsonc"),
            ]
            .into_iter()
            .collect(),
        );
    }
    #[allow(unreachable_code)]
    None
}

#[cfg(any(target_os = "macos", test))]
fn has_user_managed_preference(directory: &Path, root_plist: &Path) -> anyhow::Result<bool> {
    anyhow::ensure!(
        !root_plist.try_exists()?,
        "OpenCode has administrator-managed settings; Kool.ad/e cannot verify its tool policy"
    );
    if !directory.try_exists()? {
        return Ok(false);
    }
    for entry in fs::read_dir(directory)? {
        let path = entry?.path().join("ai.opencode.managed.plist");
        anyhow::ensure!(
            !path.try_exists()?,
            "OpenCode has administrator-managed settings; Kool.ad/e cannot verify its tool policy"
        );
    }
    Ok(false)
}

fn ensure_paths_absent(paths: &[PathBuf]) -> anyhow::Result<()> {
    for path in paths {
        anyhow::ensure!(
            !path.try_exists().map_err(|error| anyhow::anyhow!(
                "Cannot inspect OpenCode managed config {}: {error}",
                path.display()
            ))?,
            "OpenCode has administrator-managed settings; Kool.ad/e cannot verify its tool policy"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_present_managed_config_and_accepts_missing_paths() {
        let root = std::env::temp_dir().join(format!(
            "koolade-opencode-managed-config-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let present = root.join("opencode.json");
        std::fs::write(&present, "{}").unwrap();
        assert!(ensure_paths_absent(std::slice::from_ref(&present)).is_err());
        assert!(ensure_paths_absent(&[root.join("missing.json")]).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detects_root_and_per_user_managed_preference_plists() {
        let root = std::env::temp_dir().join(format!(
            "koolade-opencode-managed-preferences-{}",
            uuid::Uuid::new_v4()
        ));
        let preferences = root.join("Managed Preferences");
        std::fs::create_dir_all(preferences.join("managed-account")).unwrap();
        let root_plist = preferences.join("ai.opencode.managed.plist");
        let user_plist = preferences
            .join("managed-account")
            .join("ai.opencode.managed.plist");

        assert!(!has_user_managed_preference(&preferences, &root_plist).unwrap());
        std::fs::write(&user_plist, "managed").unwrap();
        assert!(has_user_managed_preference(&preferences, &root_plist).is_err());
        std::fs::remove_file(user_plist).unwrap();
        std::fs::write(&root_plist, "managed").unwrap();
        assert!(has_user_managed_preference(&preferences, &root_plist).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
