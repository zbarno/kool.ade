use super::*;
use crate::test_support::ProcessEnvironmentGuard;
use std::os::unix::fs::PermissionsExt;

#[test]
fn current_opencode_anthropic_key_is_exported_into_the_private_runtime_home() {
    // This crosses application-boundary configuration and the real OpenCode subprocess helper.
    // It proves only the selected Anthropic key reaches a private legacy-import file, which a
    // serializer unit test cannot show. It uses a synthetic CLI and temporary directories only;
    // no provider account, network, Docker, or SSH is required.
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-runtime-auth-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut environment = ProcessEnvironmentGuard::new();
    environment.set("HOME", &root);
    environment.remove("XDG_CONFIG_HOME");
    environment.remove("XDG_DATA_HOME");
    environment.remove("OPENCODE_CONFIG");
    environment.set("OPENCODE_CONFIG_CONTENT", r#"{"model":"openai/gpt-test"}"#);
    let binary = root.join("opencode");
    std::fs::write(
        &binary,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then printf '%s\n' '2.1.4'; exit 0; fi
if [ "$1 $2 $3 $4" = "--pure auth export anthropic" ]; then
  printf '%s\n' '[{"integrationID":"anthropic","active":true,"value":{"type":"key","key":"synthetic-api-key"}}]'
  exit 0
fi
exit 9
"#,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let boundary = test_boundary(ToolAccess::None);
    let mut args = vec![
        binary.to_string_lossy().into_owned(),
        "--model".into(),
        "anthropic/claude-test".into(),
    ];
    let mut env = Vec::new();
    configure(&boundary, CliProvider::OpenCode, &mut args, &mut env).unwrap();

    let auth_path = boundary
        .scratch
        .path
        .join("opencode-home/.local/share/opencode/auth.json");
    let auth: Value = read_json(&auth_path);
    assert_eq!(auth["anthropic"]["type"], "api");
    assert_eq!(auth["anthropic"]["key"], "synthetic-api-key");
    assert!(auth.get("openai").is_none());
    assert!(std::fs::metadata(&auth_path).unwrap().permissions().mode() & 0o077 == 0);
    drop(boundary);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn custom_anthropic_endpoint_settings_are_rejected_before_scratch_config_is_written() {
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-custom-auth-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let mut environment = ProcessEnvironmentGuard::new();
    environment.set("HOME", &root);
    environment.remove("XDG_CONFIG_HOME");
    environment.remove("XDG_DATA_HOME");
    environment.remove("OPENCODE_CONFIG");
    environment.set(
        "OPENCODE_CONFIG_CONTENT",
        r#"{"model":"openai/gpt-test","provider":{"anthropic":{"options":{"baseURL":"https://invalid.example"}}}}"#,
    );
    let boundary = test_boundary(ToolAccess::None);
    let mut args = vec![
        "/synthetic/opencode".into(),
        "--model".into(),
        "anthropic/claude-test".into(),
    ];
    assert!(configure(&boundary, CliProvider::OpenCode, &mut args, &mut Vec::new()).is_err());
    assert!(!boundary.scratch.path.join("opencode.json").exists());
    drop(boundary);
    std::fs::remove_dir_all(root).unwrap();
}
