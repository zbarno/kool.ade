use super::*;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use crate::harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest};

#[path = "tests/capabilities.rs"]
mod capabilities;

#[path = "tests/command.rs"]
mod command;
#[path = "tests/execution.rs"]
mod execution;
#[path = "tests/probe.rs"]
mod probe;

#[cfg(unix)]
fn request(root: std::path::PathBuf, progress_tx: mpsc::Sender<LiveProgress>) -> PlanningRequest {
    PlanningRequest {
        prompt_manifest: None,
        mode: ExecutionMode::Implementation,
        task_id: None,
        reasoning_level: "medium".into(),
        telemetry_phase: None,
        repo_root: root,
        runtime_config_source: None,
        prompt_body: "fixture task".into(),
        system_instructions: "fixture instructions".into(),
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
fn fake_cli(root: &std::path::Path, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join(format!("codex-{}.sh", uuid::Uuid::new_v4()));
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'codex-cli 1.2.3'; exit 0; fi\nif [ \"$1\" = \"exec\" ] && [ \"$2\" = \"--help\" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'; exit 0; fi\nif [ \"$1\" = \"login\" ] && [ \"$2\" = \"status\" ]; then echo 'Logged in'; exit 0; fi\ncat >/dev/null\n{body}\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
struct EnvOverride {
    name: &'static str,
    environment: crate::test_support::ProcessEnvironmentGuard,
}

#[cfg(unix)]
impl EnvOverride {
    fn set(name: &'static str, value: &std::path::Path) -> Self {
        let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
        environment.set(name, value.as_os_str());
        Self { name, environment }
    }

    fn set_value(&mut self, value: &std::path::Path) {
        self.environment.set(self.name, value.as_os_str());
    }
}
