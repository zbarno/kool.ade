use super::*;

#[cfg(unix)]
#[test]
fn saved_manual_cli_precedes_environment_and_path_and_executes_wrapper() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_dir("manual-locator");
    let state = root.join("state");
    let path_dir = root.join("path-bin");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::create_dir_all(&path_dir).unwrap();
    let manual = fake_cli(&root, "echo 'GitHub Copilot CLI 2.4.6'\n");
    let log = root.join("wrapper.log");
    std::fs::write(
        &manual,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\necho 'GitHub Copilot CLI 2.4.6'\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&manual, std::fs::Permissions::from_mode(0o755)).unwrap();
    let environment = fake_cli(&root, "echo 'GitHub Copilot CLI 3.0.0'\n");
    let path_cli = path_dir.join("copilot");
    std::fs::write(&path_cli, "#!/bin/sh\necho 'GitHub Copilot CLI 4.0.0'\n").unwrap();
    std::fs::set_permissions(&path_cli, std::fs::Permissions::from_mode(0o755)).unwrap();

    let settings_path = state.join("harness-settings.json");
    let mut settings = crate::persistence::harness_settings::HarnessSettings {
        schema_version: 2,
        ..crate::persistence::harness_settings::HarnessSettings::default()
    };
    settings
        .manual_executable_paths
        .insert("copilot".into(), manual.display().to_string());
    std::fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();
    let _env = EnvRestore::set(&[
        ("KOOLADE_HOME", Some(state.as_os_str())),
        (COPILOT_BINARY_ENV, Some(environment.as_os_str())),
        ("PATH", Some(path_dir.as_os_str())),
    ]);

    assert_eq!(CopilotHarness::locate_binary().unwrap(), manual);
    let report = CopilotHarness::probe_report();
    assert_eq!(report.readiness, CopilotReadiness::Ready);
    assert_eq!(report.version.as_deref(), Some("2.4.6"));
    assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "--version");

    settings.manual_executable_paths.clear();
    std::fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();
    assert_eq!(CopilotHarness::locate_binary().unwrap(), environment);
    unsafe { std::env::remove_var(COPILOT_BINARY_ENV) };
    assert_eq!(CopilotHarness::locate_binary().unwrap(), path_cli);

    settings.manual_executable_paths.insert(
        "copilot".into(),
        fake_cli(&root, "echo 'Unrelated application 1.2.3'\n")
            .display()
            .to_string(),
    );
    std::fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();
    let report = CopilotHarness::probe_report();
    assert_eq!(report.readiness, CopilotReadiness::Unusable);
    assert!(report.diagnostic.contains("did not identify itself"));
    let _ = std::fs::remove_dir_all(root);
}

/// Verifies a bad persisted manual selection does not fall through to a valid
/// environment override. This covers the settings-to-provider locator
/// boundary; no subprocess is launched, and setup uses synthetic paths only.
#[cfg(unix)]
#[test]
fn invalid_saved_manual_path_does_not_silently_select_environment_cli() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_dir("invalid-manual-locator");
    let state = root.join("state");
    std::fs::create_dir_all(&state).unwrap();
    let fallback = fake_cli(&root, "echo 'GitHub Copilot CLI 3.0.0'\n");
    std::fs::set_permissions(&fallback, std::fs::Permissions::from_mode(0o755)).unwrap();
    let settings = crate::persistence::harness_settings::HarnessSettings {
        schema_version: 3,
        manual_executable_paths: std::collections::BTreeMap::from([(
            "copilot".into(),
            root.join("missing/copilot").display().to_string(),
        )]),
        ..crate::persistence::harness_settings::HarnessSettings::default()
    };
    std::fs::write(
        state.join("harness-settings.json"),
        serde_json::to_vec(&settings).unwrap(),
    )
    .unwrap();
    let _environment = EnvRestore::set(&[
        ("KOOLADE_HOME", Some(state.as_os_str())),
        (COPILOT_BINARY_ENV, Some(fallback.as_os_str())),
    ]);

    let error = CopilotHarness::locate_binary().unwrap_err();

    assert!(
        error
            .detail()
            .contains("Configured copilot executable path")
    );
    assert!(!error.detail().contains(fallback.to_string_lossy().as_ref()));
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
struct EnvRestore {
    previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
    _lock: crate::test_support::ProcessEnvironmentLock,
}

#[cfg(unix)]
impl EnvRestore {
    fn set(values: &[(&'static str, Option<&std::ffi::OsStr>)]) -> Self {
        let lock = crate::test_support::lock_process_environment();
        let previous = values
            .iter()
            .map(|(name, _)| (*name, std::env::var_os(name)))
            .collect();
        for (name, value) in values {
            match value {
                Some(value) => unsafe { std::env::set_var(name, value) },
                None => unsafe { std::env::remove_var(name) },
            }
        }
        Self {
            previous,
            _lock: lock,
        }
    }
}

#[cfg(unix)]
impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (name, value) in self.previous.drain(..) {
            match value {
                Some(value) => unsafe { std::env::set_var(name, value) },
                None => unsafe { std::env::remove_var(name) },
            }
        }
    }
}
