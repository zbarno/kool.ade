use crate::harness::{ExecutionMode, LiveProgress, PlanningRequest};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Duration;

pub(super) fn request(
    root: std::path::PathBuf,
    progress_tx: mpsc::Sender<LiveProgress>,
) -> PlanningRequest {
    PlanningRequest {
        prompt_manifest: None,
        mode: ExecutionMode::Implementation,
        task_id: None,
        reasoning_level: "high".into(),
        telemetry_phase: None,
        repo_root: root,
        runtime_config_source: None,
        prompt_body: "fixture task".into(),
        system_instructions: "fixture policy".into(),
        timeout: Duration::from_secs(5),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        review_identity: None,
    }
}

#[cfg(unix)]
pub(super) fn temp_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("koolade-claude-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[cfg(unix)]
pub(super) fn fake_cli(root: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join(format!("claude-{}.sh", uuid::Uuid::new_v4()));
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--settings\" ]; then\n  [ \"$2\" = '{{\"env\":{{\"DISABLE_UPDATES\":\"1\"}}}}' ] || exit 94\n  shift 2\nfi\n[ \"$DISABLE_UPDATES\" = \"1\" ] || exit 93\n{body}\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
pub(super) struct EnvOverride {
    _environment: crate::test_support::ProcessEnvironmentGuard,
}

#[cfg(unix)]
impl EnvOverride {
    pub(super) fn set(path: &std::path::Path) -> Self {
        let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
        environment.set(super::super::CLAUDE_BINARY_ENV, path.as_os_str());
        Self {
            _environment: environment,
        }
    }
}
