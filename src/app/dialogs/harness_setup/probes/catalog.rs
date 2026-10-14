use std::io::Read;

pub(super) fn configured_model_catalog(harness: &str) -> (Vec<String>, Option<String>) {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    match (harness, home) {
        ("pi", _) => {
            crate::harness::pi_provider_settings::configured_model_catalog().unwrap_or_default()
        }
        ("codex", Some(home)) => {
            let mut models = Vec::new();
            let mut global_default = None;
            let mut active_profile = None;
            let mut profile_models = std::collections::BTreeMap::new();
            let mut section = String::new();
            if let Some(text) = read_small_text(&home.join(".codex/config.toml")) {
                for line in text.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with('[') && trimmed.ends_with(']') {
                        section = trimmed.trim_matches(['[', ']']).to_owned();
                        continue;
                    }
                    let Some((key, value)) = line.split_once('=') else {
                        continue;
                    };
                    let key = key.trim();
                    let model = value.trim().trim_matches(['"', '\'']);
                    if key == "profile" && section.is_empty() {
                        active_profile = Some(model.to_owned());
                    }
                    if key == "model" && !model.is_empty() {
                        models.push(model.to_owned());
                        if section.is_empty() {
                            global_default = Some(model.to_owned());
                        } else if let Some(profile) = section.strip_prefix("profiles.") {
                            profile_models.insert(profile.to_owned(), model.to_owned());
                        }
                    }
                }
            }
            let mut default_model = active_profile
                .and_then(|profile| profile_models.get(&profile).cloned())
                .or(global_default);
            if let Ok(model) = std::env::var(crate::harness::codex_harness::CODEX_MODEL_ENV)
                && !model.trim().is_empty()
            {
                models.push(model.clone());
                default_model = Some(model);
            }
            models.sort();
            models.dedup();
            (models, default_model)
        }
        ("claude", Some(home)) => {
            let configured = read_small_bytes(&home.join(".claude/settings.json"))
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|settings| {
                    settings
                        .get("model")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                });
            let mut default_model = configured.clone();
            let mut models = configured.into_iter().collect::<Vec<_>>();
            if let Ok(model) = std::env::var(crate::harness::claude_harness::CLAUDE_MODEL_ENV)
                && !model.trim().is_empty()
            {
                models.push(model.clone());
                default_model = Some(model);
            }
            models.sort();
            models.dedup();
            (models, default_model)
        }
        ("copilot", home) => {
            let configured = std::env::var(crate::harness::copilot_harness::COPILOT_MODEL_ENV)
                .ok()
                .filter(|model| !model.trim().is_empty())
                .or_else(|| {
                    home.and_then(|home| read_small_bytes(&home.join(".copilot/settings.json")))
                        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                        .and_then(|settings| {
                            settings
                                .get("model")
                                .and_then(serde_json::Value::as_str)
                                .map(str::to_owned)
                        })
                });
            let mut models = vec!["auto".to_owned()];
            if let Some(model) = &configured {
                models.push(model.clone());
            }
            models.sort();
            models.dedup();
            (models, configured)
        }
        _ => (Vec::new(), None),
    }
}

fn read_small_text(path: &std::path::Path) -> Option<String> {
    String::from_utf8(read_small_bytes(path)?).ok()
}

pub(super) fn read_small_bytes(path: &std::path::Path) -> Option<Vec<u8>> {
    const LIMIT: u64 = 1024 * 1024;
    let before_open = std::fs::symlink_metadata(path).ok()?;
    if !before_open.is_file() || before_open.file_type().is_symlink() || before_open.len() > LIMIT {
        return None;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return None;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= LIMIT).then_some(bytes)
}

#[cfg(all(test, unix))]
mod tests {
    use super::{configured_model_catalog, read_small_bytes};
    use std::{ffi::CString, os::unix::ffi::OsStrExt, path::PathBuf};

    #[test]
    fn config_reader_ignores_symlinks_and_non_regular_files() {
        let root = std::env::temp_dir().join(format!(
            "koolade-cli-config-{}",
            uuid::Uuid::new_v4().hyphenated()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let cleanup = Cleanup(root.clone());
        let regular = root.join("settings.json");
        std::fs::write(&regular, br#"{"model":"example"}"#).unwrap();
        assert_eq!(
            read_small_bytes(&regular).unwrap(),
            br#"{"model":"example"}"#
        );

        let linked = root.join("linked-settings.json");
        std::os::unix::fs::symlink(&regular, &linked).unwrap();
        assert!(read_small_bytes(&linked).is_none());

        let fifo = root.join("settings.fifo");
        let path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: the NUL-terminated path stays alive for the call and the mode
        // creates a private test fixture in this unique temporary directory.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        assert!(read_small_bytes(&fifo).is_none());
        drop(cleanup);
    }

    #[test]
    fn malformed_and_oversized_provider_configs_do_not_create_models_or_panic() {
        let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
        let root = std::env::temp_dir().join(format!(
            "koolade-invalid-provider-config-{}",
            uuid::Uuid::new_v4().hyphenated()
        ));
        std::fs::create_dir_all(root.join(".codex")).unwrap();
        std::fs::create_dir_all(root.join(".claude")).unwrap();
        std::fs::create_dir_all(root.join(".copilot")).unwrap();
        let cleanup = Cleanup(root.clone());
        std::fs::write(root.join(".codex/config.toml"), vec![b'x'; 1024 * 1024 + 1]).unwrap();
        std::fs::write(root.join(".claude/settings.json"), b"{\"model\":").unwrap();
        std::fs::write(
            root.join(".copilot/settings.json"),
            vec![b'x'; 1024 * 1024 + 1],
        )
        .unwrap();
        environment.set("HOME", &root);
        for name in [
            crate::harness::codex_harness::CODEX_MODEL_ENV,
            crate::harness::claude_harness::CLAUDE_MODEL_ENV,
            crate::harness::copilot_harness::COPILOT_MODEL_ENV,
        ] {
            environment.remove(name);
        }

        assert_eq!(configured_model_catalog("codex"), (Vec::new(), None));
        assert_eq!(configured_model_catalog("claude"), (Vec::new(), None));
        assert_eq!(
            configured_model_catalog("copilot"),
            (vec!["auto".into()], None)
        );
        drop(cleanup);
    }

    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
