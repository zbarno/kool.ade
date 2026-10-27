use super::support::{EnvOverride, fake_cli, request, temp_dir};
use crate::harness::{AiHarness, ClaudeHarness, ExecutionMode};
use std::sync::mpsc;

#[test]
// Production boundary: the public Claude AiHarness task entry point. This
// verifies no provider subprocess starts in any mode; capability metadata
// alone cannot prove the invocation path stops before CLI discovery and Docker
// setup. Uses one temporary fake executable, no Docker/network/auth, low cost.
fn task_execution_is_blocked_before_starting_the_provider_process() {
    let root = temp_dir("blocked-execute");
    let marker = root.join("provider-was-started");
    let binary = fake_cli(&root, &format!("touch '{}'; exit 0", marker.display()));
    let _env = EnvOverride::set(&binary);

    for mode in ExecutionMode::ALL {
        let mut task = request(root.clone(), mpsc::channel().0);
        task.mode = mode;
        let error = ClaudeHarness.execute(&task).unwrap_err();
        assert!(error.detail().contains("organization-managed hooks"));
        assert!(!marker.exists(), "Claude {mode:?} started its CLI");
    }

    std::fs::remove_dir_all(root).unwrap();
}
