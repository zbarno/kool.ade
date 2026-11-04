use super::{
    CliProvider, ValidatedProviderLaunch, antigravity_environment, complete_evidence, provider_argv,
};
use std::path::Path;

fn claude_argv() -> Vec<String> {
    vec![
        "/bin/true".into(),
        "--tools".into(),
        "--restricted".into(),
        "--strict-mcp-config".into(),
        "--permission-mode".into(),
        "dontAsk".into(),
        "--effort".into(),
        "high".into(),
        "--settings".into(),
        crate::harness::claude_harness::UPDATE_LOCK_SETTINGS.into(),
    ]
}

fn claude_environment() -> Vec<(String, String)> {
    vec![("DISABLE_UPDATES".into(), "1".into())]
}

fn codex_argv() -> Vec<String> {
    vec![
        "/bin/true".into(),
        "--sandbox".into(),
        "--ignore-user-config".into(),
        "--skip-git-repo-check".into(),
        "--config".into(),
        "model_reasoning_effort=high".into(),
    ]
}

// Final launch boundary: a complete Codex profile still cannot start a task
// while managed-hook execution is not contained. Metadata alone cannot stop
// an alternate caller constructing a launch. This uses a synthetic binary and
// requires no Docker, network, or provider account.
#[test]
fn codex_task_launch_is_rejected_before_process_creation() {
    let error = ValidatedProviderLaunch::bind(
        complete_evidence(CliProvider::Codex, "99.0.0"),
        codex_argv(),
        Path::new("/tmp"),
        vec![],
    )
    .err()
    .expect("Codex task launch must remain blocked");

    assert!(error.to_string().contains("organization-managed hooks"));
}

// Production boundary: final validated launch binding used by `spawn_with_input`.
// This proves the Claude task process cannot be constructed even with otherwise
// valid restricted arguments. It uses a synthetic executable, with no Docker,
// network, provider account, or costly setup.
#[test]
fn claude_task_launch_is_rejected_before_process_creation() {
    let error = ValidatedProviderLaunch::bind(
        complete_evidence(CliProvider::Claude, "2.1.268"),
        claude_argv(),
        Path::new("/tmp"),
        claude_environment(),
    )
    .err()
    .expect("Claude task launch must remain blocked");

    assert!(error.to_string().contains("organization-managed hooks"));
}

#[test]
fn claude_task_launch_is_blocked_even_with_a_valid_update_lock() {
    let provider = CliProvider::Claude;
    let error = ValidatedProviderLaunch::bind(
        complete_evidence(provider, "2.1.268"),
        claude_argv(),
        Path::new("/tmp"),
        claude_environment(),
    )
    .err()
    .expect("Claude task launch must remain blocked");
    assert!(error.to_string().contains("organization-managed hooks"));
}

#[test]
fn antigravity_manager_launch_is_rejected_when_instruction_sources_are_unverified() {
    let provider = CliProvider::Antigravity;
    let error = ValidatedProviderLaunch::bind_for_workspace(
        complete_evidence(provider, "99.0.0"),
        crate::harness::ExecutionMode::Manager,
        provider_argv(provider),
        Path::new("/tmp"),
        antigravity_environment(),
        Path::new("/tmp"),
    )
    .err()
    .expect("Antigravity has no verified strict Manager profile");

    assert!(error.to_string().contains("not for Man.ager"));
}
