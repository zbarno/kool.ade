use std::path::PathBuf;

pub(super) struct Roots {
    pub(super) app_data_root: PathBuf,
    pub(super) legacy_state_root: Option<PathBuf>,
}

#[cfg(not(test))]
pub(super) fn resolve() -> anyhow::Result<Roots> {
    if let Some(value) = std::env::var_os("KOOLADE_HOME")
        && !value.is_empty()
    {
        let root = PathBuf::from(value);
        anyhow::ensure!(
            root.is_absolute(),
            "KOOLADE_HOME must be an absolute path for machine setup state"
        );
        return Ok(Roots {
            app_data_root: root,
            legacy_state_root: None,
        });
    }

    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let xdg = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let app_data_root = user_data_root(std::env::consts::OS, home, xdg, local)?;
    let legacy_state_root = crate::persistence::state_root();
    anyhow::ensure!(
        legacy_state_root.is_absolute(),
        "the legacy machine setup state directory is unavailable"
    );
    Ok(Roots {
        app_data_root,
        legacy_state_root: Some(legacy_state_root),
    })
}

fn user_data_root(
    os: &str,
    home: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
) -> anyhow::Result<PathBuf> {
    match os {
        "linux" => {
            if let Some(path) = xdg_data_home.filter(|path| path.is_absolute()) {
                return Ok(path.join("koolade"));
            }
            Ok(absolute_path(home, "home")?.join(".local/share/koolade"))
        }
        "macos" => Ok(absolute_path(home, "home")?.join("Library/Application Support/koolade")),
        "windows" => {
            let path = local_app_data
                .filter(|path| path.is_absolute())
                .or_else(|| {
                    home.filter(|path| path.is_absolute())
                        .map(|path| path.join("AppData/Local"))
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "the current user's local application data directory is unavailable"
                    )
                })?;
            Ok(path.join("Koolade"))
        }
        _ => anyhow::bail!("this operating system has no configured Kool.ad/e user data directory"),
    }
}

fn absolute_path(path: Option<PathBuf>, label: &str) -> anyhow::Result<PathBuf> {
    path.filter(|path| path.is_absolute())
        .ok_or_else(|| anyhow::anyhow!("the current user's {label} directory is unavailable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_each_desktop_platform_to_its_standard_user_data_directory() {
        let home = Some(sample_home());
        assert_eq!(
            user_data_root("linux", home.clone(), None, None).unwrap(),
            sample_home().join(".local/share/koolade")
        );
        assert_eq!(
            user_data_root("linux", home.clone(), Some(sample_xdg()), None).unwrap(),
            sample_xdg().join("koolade")
        );
        assert_eq!(
            user_data_root("macos", home, None, None).unwrap(),
            sample_home().join("Library/Application Support/koolade")
        );
        assert_eq!(
            user_data_root(
                "windows",
                Some(sample_home()),
                None,
                Some(sample_local_app_data())
            )
            .unwrap(),
            sample_local_app_data().join("Koolade")
        );
    }

    #[test]
    fn rejects_relative_platform_data_paths_and_unknown_operating_systems() {
        assert!(user_data_root("linux", Some(PathBuf::from("relative")), None, None).is_err());
        assert!(user_data_root("plan9", None, None, None).is_err());
    }

    #[cfg(windows)]
    fn sample_home() -> PathBuf {
        PathBuf::from(r"C:\Users\operator")
    }

    #[cfg(not(windows))]
    fn sample_home() -> PathBuf {
        PathBuf::from("/home/operator")
    }

    #[cfg(windows)]
    fn sample_xdg() -> PathBuf {
        PathBuf::from(r"C:\custom\data")
    }

    #[cfg(not(windows))]
    fn sample_xdg() -> PathBuf {
        PathBuf::from("/custom/data")
    }

    #[cfg(windows)]
    fn sample_local_app_data() -> PathBuf {
        PathBuf::from(r"C:\Users\operator\AppData\Local")
    }

    #[cfg(not(windows))]
    fn sample_local_app_data() -> PathBuf {
        PathBuf::from("/Users/operator/AppData/Local")
    }
}
