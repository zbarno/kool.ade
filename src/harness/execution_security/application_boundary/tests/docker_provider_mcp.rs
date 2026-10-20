//! Docker-backed integration coverage for Codex's production launch path.
//!
//! Boundary and invariant: a fake Codex process must read a selected repository
//! only through the real Kool.ad/e MCP server and a read-only Docker session.
//! A unit test cannot prove the image mount, child process, IPC socket, and MCP
//! result propagation work together. External dependency: Linux Docker Engine
//! and the pinned general executor image. Setup is low after the image is
//! present; the first run may need its normal image preparation step.

use super::super::override_test_app_binary;
use crate::{
    execution::contract::WorkspaceRef,
    harness::{AiHarness, CodexHarness, ExecutionMode, PlanningRequest},
    test_support::ProcessEnvironmentGuard,
};
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::Duration,
};

#[test]
fn codex_reads_the_selected_repository_through_the_docker_mcp_bridge() {
    if std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("skipping Docker provider smoke; set KOOLADE_DOCKER_BACKEND_INTEGRATION=1");
        return;
    }
    if !cfg!(target_os = "linux") {
        return;
    }

    let _identity =
        crate::core::gitops::test_support::shield_with_synthetic_identity("codex-docker-mcp-smoke");
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("fixture");
    fs::create_dir_all(&root).unwrap();
    let mut environment = ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", root.join("koolade-home"));

    let repository = root.join("repository");
    fs::create_dir(&repository).unwrap();
    git(&repository, &["init", "--quiet"]);
    fs::write(repository.join("README.md"), "docker-mcp-read-only-proof\n").unwrap();
    git(&repository, &["add", "README.md"]);
    git(
        &repository,
        &[
            "-c",
            "user.name=Koolade test",
            "-c",
            "user.email=koolade-test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    let source_sha = String::from_utf8(git_output(&repository, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let workspace = WorkspaceRef {
        project_id,
        task_id: task_id.clone(),
        source_sha,
    };

    let app_binary = current_application_binary();
    let _app_binary = override_test_app_binary(app_binary.clone());
    let codex_script = root.join("codex");
    let result_path = root.join("mcp-result.json");
    let codex_fixture = r##"#!/bin/sh
set -eu
if [ "$1" = "--version" ]; then echo 'codex-cli 1.2.3'; exit 0; fi
if [ "$1" = "login" ] && [ "$2" = "status" ]; then echo 'Logged in'; exit 0; fi
if [ "$1" = "exec" ] && [ "$2" = "--help" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'; exit 0; fi
config=''
previous=''
for arg in "$@"; do
  if [ "$previous" = '--config' ]; then
    case "$arg" in mcp_servers.*.args=*) config=$(printf '%s' "$arg" | sed -E 's/^.*[,\"]([^\"]+)\"\]$/\1/');; esac
  fi
  previous="$arg"
done
test -n "$config"
printf '%s\n' '{"type":"error","error":{"message":"code mode is disabled on host by policy"}}'
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"koolade_bash","arguments":{"command":"cat README.md"}}}' | {app} --internal-mcp-server "$config" > {result}
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"read completed through Kool.ad/e Docker MCP"}}' '{"type":"turn.completed"}'
"##;
    let codex_fixture = codex_fixture
        .replace("{app}", &shell_quote(&app_binary))
        .replace("{result}", &shell_quote(&result_path));
    fs::write(&codex_script, codex_fixture).unwrap();
    fs::set_permissions(&codex_script, fs::Permissions::from_mode(0o700)).unwrap();
    environment.set(
        crate::harness::codex_harness::CODEX_BINARY_ENV,
        &codex_script,
    );

    let (progress_tx, _progress_rx) = mpsc::channel();
    let request = PlanningRequest {
        prompt_manifest: None,
        mode: ExecutionMode::Planning,
        task_id: Some(task_id),
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: repository,
        runtime_config_source: None,
        prompt_body: "Read README.md through the app-owned repository tool.".into(),
        system_instructions: "Use only Kool.ad/e MCP tools for repository access.".into(),
        timeout: Duration::from_secs(90),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: Some(workspace),
        destination_sha: None,
        authorization_verifier: None,
        review_identity: None,
    };
    let outcome = CodexHarness
        .execute_with_model(&request, Some("synthetic-model"))
        .unwrap_or_else(|error| panic!("Docker-backed Codex MCP operation failed: {error:#}"));
    assert_eq!(
        outcome.final_text,
        "read completed through Kool.ad/e Docker MCP"
    );
    let response: Value = serde_json::from_slice(&fs::read(result_path).unwrap()).unwrap();
    assert_eq!(response["result"]["isError"], false);
    assert!(
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("docker-mcp-read-only-proof")
    );
}

fn current_application_binary() -> PathBuf {
    let test_binary = std::env::current_exe().unwrap();
    let path = test_binary
        .parent()
        .and_then(Path::parent)
        .expect("all-target test binary must be in target/debug/deps")
        .join("koolade");
    assert!(
        path.is_file(),
        "all-targets must build the app MCP server binary"
    );
    path
}

fn git(root: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap()
            .success(),
        "git command failed: {args:?}"
    );
}

fn git_output(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success(), "git command failed: {args:?}");
    output.stdout
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
