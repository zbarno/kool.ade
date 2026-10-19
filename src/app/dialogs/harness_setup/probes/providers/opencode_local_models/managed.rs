use std::path::PathBuf;
#[cfg(any(target_os = "macos", test))]
use std::{fs, path::Path};

pub(super) fn has_platform_managed_config() -> Option<bool> {
    let paths = managed_config_paths()?;
    if has_managed_config(&paths)? {
        return Some(true);
    }
    #[cfg(target_os = "macos")]
    return has_user_managed_preference(
        Path::new("/Library/Managed Preferences"),
        Path::new("/Library/Managed Preferences/ai.opencode.managed.plist"),
    );
    #[cfg(not(target_os = "macos"))]
    Some(false)
}

fn managed_config_paths() -> Option<Vec<PathBuf>> {
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
fn has_user_managed_preference(directory: &Path, root_plist: &Path) -> Option<bool> {
    if root_plist.try_exists().ok()? {
        return Some(true);
    }
    if !directory.try_exists().ok()? {
        return Some(false);
    }
    for entry in fs::read_dir(directory).ok()? {
        let path = entry.ok()?.path().join("ai.opencode.managed.plist");
        if path.try_exists().ok()? {
            return Some(true);
        }
    }
    Some(false)
}

pub(super) fn has_managed_config(paths: &[PathBuf]) -> Option<bool> {
    for path in paths {
        match path.try_exists() {
            Ok(true) => return Some(true),
            Ok(false) => {}
            Err(_) => return None,
        }
    }
    Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_root_or_per_user_managed_preferences() {
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

        assert_eq!(
            has_user_managed_preference(&preferences, &root_plist),
            Some(false)
        );
        std::fs::write(&user_plist, "managed").unwrap();
        assert_eq!(
            has_user_managed_preference(&preferences, &root_plist),
            Some(true)
        );
        std::fs::remove_file(user_plist).unwrap();
        std::fs::write(&root_plist, "managed").unwrap();
        assert_eq!(
            has_user_managed_preference(&preferences, &root_plist),
            Some(true)
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
