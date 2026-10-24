use super::*;
use crate::persistence::harness_settings::HarnessSettings;

fn temp_dir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("koolade-manual-cli-{}", uuid::Uuid::new_v4()))
}

fn planning_request(mode: ExecutionMode) -> PlanningRequest {
    let (progress_tx, _progress_rx) = std::sync::mpsc::channel();
    PlanningRequest {
        prompt_manifest: None,
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
        execution_workspace: None,
        destination_sha: None,

        authorization_verifier: None,
        review_identity: None,
    }
}

fn runtime_qa_review_request() -> PlanningRequest {
    let mut request = planning_request(ExecutionMode::ReadOnlyAnalysis);
    request.telemetry_phase = Some("runtime_qa_review".into());
    request.prompt_manifest = Some(crate::harness::prompt_catalog::PromptManifest {
        role_version: "runtime_qa/v3".into(),
        system_template_sha256: "0".repeat(64),
        policy_template_sha256: "0".repeat(64),
        user_template_sha256: "0".repeat(64),
        output_schema_version: "ReviewVerdictV1".into(),
        privilege_profile: "read_only; isolated_fresh_review_context".into(),
    });
    request.review_identity = Some(crate::harness::ReviewExecutionIdentity {
        execution_id: "00000000-0000-4000-8000-000000000001".into(),
        session_id: "00000000-0000-4000-8000-000000000002".into(),
        runtime_qa_screenshot_review: None,
    });
    request.prompt_body = "reviewer execution 00000000-0000-4000-8000-000000000001 session 00000000-0000-4000-8000-000000000002".into();
    request
}

#[test]
fn reviewer_dispatch_requires_distinct_controller_identity_and_read_only_prompt() {
    let mut request = planning_request(ExecutionMode::ReadOnlyAnalysis);
    request.telemetry_phase = Some("independent_code_review".into());
    request.prompt_manifest = Some(crate::harness::prompt_catalog::PromptManifest {
        role_version: "code_review/v2".into(),
        system_template_sha256: "0".repeat(64),
        policy_template_sha256: "0".repeat(64),
        user_template_sha256: "0".repeat(64),
        output_schema_version: "1".into(),
        privilege_profile: "read_only; isolated_fresh_review_context".into(),
    });
    request.review_identity = Some(crate::harness::ReviewExecutionIdentity {
        execution_id: "00000000-0000-4000-8000-000000000001".into(),
        session_id: "00000000-0000-4000-8000-000000000002".into(),
        runtime_qa_screenshot_review: None,
    });
    request.prompt_body = "reviewer execution 00000000-0000-4000-8000-000000000001 session 00000000-0000-4000-8000-000000000002".into();
    assert!(super::validate_review_invocation(&request).is_ok());

    let execution_id = request
        .review_identity
        .as_ref()
        .unwrap()
        .execution_id
        .clone();
    request.review_identity.as_mut().unwrap().session_id = execution_id;
    assert!(super::validate_review_invocation(&request).is_err());
}

#[test]
fn runtime_qa_reviewer_dispatch_uses_a_fresh_toolless_context() {
    let request = runtime_qa_review_request();
    assert!(super::validate_review_invocation(&request).is_ok());
    let boundary = crate::harness::execution_security::ApplicationBoundary::new(&request).unwrap();
    assert_eq!(boundary.access(), ToolAccess::None);

    let mut wrong_phase = request.clone();
    wrong_phase.telemetry_phase = Some("independent_code_review".into());
    assert!(super::validate_review_invocation(&wrong_phase).is_err());

    let mut writable_mode = request.clone();
    writable_mode.mode = ExecutionMode::Implementation;
    assert!(super::validate_review_invocation(&writable_mode).is_err());

    let mut attached_workspace = request;
    attached_workspace.execution_workspace = Some(crate::execution::contract::WorkspaceRef {
        project_id: "00000000-0000-4000-8000-000000000003".into(),
        task_id: "00000000-0000-4000-8000-000000000004".into(),
        source_sha: "a".repeat(40),
    });
    assert!(super::validate_review_invocation(&attached_workspace).is_err());
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
        assert!(
            error.contains("no configured application-owned Docker execution policy"),
            "{error}"
        );
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
        Some(binary.canonicalize().unwrap())
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

#[test]
#[cfg(unix)]
fn manual_path_rejects_relative_executable_paths() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_dir();
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("relative-cli");
    std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let settings = HarnessSettings {
        manual_executable_paths: std::collections::BTreeMap::from([(
            "codex".into(),
            "relative-cli".into(),
        )]),
        ..HarnessSettings::default()
    };

    let error = manual_executable_path_from(&settings, "codex").unwrap_err();

    assert!(error.detail().contains("must be absolute"));
    std::fs::remove_dir_all(root).unwrap();
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
