use super::*;
use crate::persistence::harness_settings::HarnessSettings;

fn temp_dir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("koolade-manual-cli-{}", uuid::Uuid::new_v4()))
}

fn planning_request(mode: ExecutionMode) -> PlanningRequest {
    let (progress_tx, _progress_rx) = std::sync::mpsc::channel();
    PlanningRequest {
        mode,
        task_id: Some("synthetic-task".into()),
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: std::path::PathBuf::from("/synthetic/project"),
        runtime_config_source: None,
        prompt_body: String::new(),
        system_instructions: String::new(),
        timeout: std::time::Duration::from_secs(1),
        progress_tx,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    }
}

#[test]
fn application_boundary_accepts_supported_clis_and_rejects_unknown_providers() {
    for mode in ExecutionMode::ALL {
        let request = planning_request(mode);
        for provider in [
            "pi",
            "codex",
            "claude",
            "opencode",
            "copilot",
            "antigravity",
        ] {
            require_application_implementation_boundary(provider, &request).unwrap();
        }
        let error = require_application_implementation_boundary("unsupported", &request)
            .unwrap_err()
            .detail();
        assert!(error.contains("application-owned Linux sandbox"), "{error}");
        assert!(error.contains("No CLI was started"), "{error}");
    }
}

#[test]
fn providers_fail_closed_when_the_application_cannot_prepare_a_workspace() {
    let request = planning_request(ExecutionMode::Implementation);
    let providers: [(&str, &dyn AiHarness); 5] = [
        ("Codex", &CodexHarness),
        ("Claude Code", &ClaudeHarness),
        ("OpenCode", &OpenCodeHarness),
        ("Copilot CLI", &CopilotHarness),
        ("Antigravity", &AntigravityHarness),
    ];
    for (name, provider) in providers {
        let error = provider.execute(&request).unwrap_err().detail();
        assert!(
            error.contains("sandbox") || error.contains("resource broker"),
            "{name}: {error}"
        );
    }
}

#[test]
#[cfg(unix)]
fn manual_path_accepts_spaces_and_symlinks_but_rejects_non_executables() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let root = temp_dir();
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("custom cli wrapper");
    std::fs::write(&binary, "#!/bin/sh\nexec /usr/bin/env true\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let alias = root.join("selected cli");
    symlink(&binary, &alias).unwrap();
    let settings = HarnessSettings {
        manual_executable_paths: std::collections::BTreeMap::from([(
            "codex".into(),
            alias.to_string_lossy().into_owned(),
        )]),
        ..HarnessSettings::default()
    };
    assert_eq!(
        manual_executable_path_from(&settings, "codex").unwrap(),
        Some(alias)
    );

    let non_executable = root.join("plain file");
    std::fs::write(&non_executable, "not executable").unwrap();
    let invalid = HarnessSettings {
        manual_executable_paths: std::collections::BTreeMap::from([(
            "codex".into(),
            non_executable.display().to_string(),
        )]),
        ..HarnessSettings::default()
    };
    assert!(manual_executable_path_from(&invalid, "codex").is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn missing_manual_entry_leaves_automatic_discovery_available() {
    assert_eq!(
        manual_executable_path_from(&HarnessSettings::default(), "pi").unwrap(),
        None
    );
}

#[cfg(unix)]
#[test]
fn manual_selection_wins_when_multiple_installations_exist() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_dir();
    std::fs::create_dir_all(&root).unwrap();
    let old = root.join("codex-v1");
    let selected = root.join("codex-v2");
    for binary in [&old, &selected] {
        std::fs::write(binary, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let settings = HarnessSettings {
        manual_executable_paths: std::collections::BTreeMap::from([(
            "codex".into(),
            selected.display().to_string(),
        )]),
        ..HarnessSettings::default()
    };
    assert_eq!(
        manual_executable_path_from(&settings, "codex").unwrap(),
        Some(selected)
    );
    std::fs::remove_dir_all(root).unwrap();
}
