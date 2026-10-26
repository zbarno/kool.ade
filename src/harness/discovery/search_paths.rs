use std::{
    env,
    ffi::{OsStr, OsString},
    path::PathBuf,
};

pub(crate) fn host_executable_candidates(name: &str) -> Vec<PathBuf> {
    search_directories()
        .into_iter()
        .flat_map(|directory| {
            executable_names(name).map(move |candidate| directory.join(candidate))
        })
        .collect()
}

pub(crate) fn host_search_path() -> OsString {
    let directories = search_directories();
    env::join_paths(directories).unwrap_or_else(|_| env::var_os("PATH").unwrap_or_default())
}

fn search_directories() -> Vec<PathBuf> {
    let mut paths = env::var_os("PATH")
        .map(|path| env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    paths.extend(common_install_directories());
    unique_existing_absolute(paths)
}

fn unique_existing_absolute(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for path in paths {
        if path.is_absolute() && path.is_dir() && !result.contains(&path) {
            result.push(path);
        }
    }
    result
}

#[cfg(windows)]
fn common_install_directories() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = env::var_os("USERPROFILE").map(PathBuf::from) {
        for child in [".bun/bin", ".cargo/bin", "scoop/shims"] {
            paths.push(home.join(child));
        }
    }
    if let Some(app_data) = env::var_os("APPDATA").map(PathBuf::from) {
        paths.push(app_data.join("npm"));
    }
    if let Some(local) = env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        for child in ["Programs/Codex", "Programs/Claude", "Programs/OpenCode"] {
            paths.push(local.join(child));
        }
    }
    paths
}

#[cfg(target_os = "macos")]
fn common_install_directories() -> Vec<PathBuf> {
    let mut paths = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        for child in [
            ".local/bin",
            ".cargo/bin",
            ".npm-global/bin",
            ".npm/bin",
            ".bun/bin",
            ".pi/bin",
            ".volta/bin",
            ".local/share/mise/shims",
            ".asdf/shims",
            "Library/pnpm",
        ] {
            paths.push(home.join(child));
        }
    }
    paths
}

#[cfg(all(unix, not(target_os = "macos")))]
fn common_install_directories() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        for child in [
            ".local/bin",
            ".cargo/bin",
            ".npm-global/bin",
            ".npm/bin",
            ".bun/bin",
            ".pi/bin",
            ".local/share/pnpm",
            ".local/share/mise/shims",
            ".asdf/shims",
        ] {
            paths.push(home.join(child));
        }
    }
    paths
}

#[cfg(not(any(unix, windows)))]
fn common_install_directories() -> Vec<PathBuf> {
    Vec::new()
}

fn executable_names(name: &str) -> impl Iterator<Item = OsString> {
    let name = OsStr::new(name);
    #[cfg(windows)]
    {
        let names = vec![
            append_extension(name, ".exe"),
            append_extension(name, ".com"),
            name.to_os_string(),
        ];
        names.into_iter()
    }
    #[cfg(not(windows))]
    {
        std::iter::once(name.to_os_string())
    }
}

#[cfg(windows)]
fn append_extension(name: &OsStr, extension: &str) -> OsString {
    let mut value = name.to_os_string();
    value.push(extension);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_path_preserves_order_drops_relative_paths_and_deduplicates() {
        let root = std::env::temp_dir().join(format!("koolade-search-path-{}", std::process::id()));
        let first = root.join("first");
        let second = root.join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let paths = unique_existing_absolute(vec![
            first.clone(),
            PathBuf::from("relative"),
            first.clone(),
            second.clone(),
        ]);
        assert_eq!(paths, [first.clone(), second]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn windows_candidates_prefer_native_executables() {
        let names = executable_names("codex").collect::<Vec<_>>();
        assert_eq!(names, ["codex.exe", "codex.com", "codex"]);
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_candidates_use_the_exact_executable_name() {
        assert_eq!(
            executable_names("codex").collect::<Vec<_>>(),
            [OsString::from("codex")]
        );
    }
}
