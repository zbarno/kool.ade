#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HarnessProbe {
    pub id: String,
    pub status: String,
    pub version: Option<String>,
    pub executable: Option<String>,
    pub diagnostic: Option<String>,
    pub ready: bool,
    pub models: Vec<String>,
    pub default_model: Option<String>,
    pub model_catalog_fresh: bool,
    pub configuration_required: bool,
    pub implementation_available: bool,
}

pub(super) fn is_executable_file(path: &std::path::Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        path.extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|extension| {
                ["exe", "com", "cmd", "bat"]
                    .iter()
                    .any(|allowed| extension.eq_ignore_ascii_case(allowed))
            })
    }
    #[cfg(not(any(unix, windows)))]
    {
        true
    }
}
