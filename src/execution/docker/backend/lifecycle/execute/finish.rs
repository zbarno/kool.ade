#[cfg(test)]
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::super::runtime::RuntimeExecution;
use super::{cleanup, state, sync};
use crate::execution::{
    contract::{ExecutionLifecycleV1, ExecutionRequestV1},
    docker::{
        backend::DockerExecutionBackend,
        sandbox::{ValidatedWorkspace, WorkspaceSnapshot},
    },
    errors::{ExecutionError, ExecutionErrorKind},
};
use std::time::Instant;

#[cfg(test)]
enum StageOutTestHook {
    Crash,
    Pause {
        reached: tokio::sync::oneshot::Sender<()>,
        resume: tokio::sync::oneshot::Receiver<()>,
    },
}

#[cfg(test)]
type PreCollectingTestHook = (
    tokio::sync::oneshot::Sender<()>,
    tokio::sync::oneshot::Receiver<()>,
);

#[cfg(test)]
static STAGE_OUT_TEST_HOOKS: std::sync::LazyLock<Mutex<HashMap<String, StageOutTestHook>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
static PRE_COLLECTING_TEST_HOOKS: std::sync::LazyLock<
    Mutex<HashMap<String, PreCollectingTestHook>>,
> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
pub(in crate::execution::docker::backend) fn inject_crash_after_stage_out(execution_id: String) {
    STAGE_OUT_TEST_HOOKS
        .lock()
        .expect("stage-out test hook lock")
        .insert(execution_id, StageOutTestHook::Crash);
}

#[cfg(test)]
pub(in crate::execution::docker::backend) fn pause_after_stage_out(
    execution_id: String,
) -> (
    tokio::sync::oneshot::Receiver<()>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (reached, reached_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume) = tokio::sync::oneshot::channel();
    STAGE_OUT_TEST_HOOKS
        .lock()
        .expect("stage-out test hook lock")
        .insert(execution_id, StageOutTestHook::Pause { reached, resume });
    (reached_rx, resume_tx)
}

#[cfg(test)]
pub(in crate::execution::docker::backend) fn pause_before_collecting(
    execution_id: String,
) -> (
    tokio::sync::oneshot::Receiver<()>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (reached, reached_rx) = tokio::sync::oneshot::channel();
    let (resume_tx, resume) = tokio::sync::oneshot::channel();
    PRE_COLLECTING_TEST_HOOKS
        .lock()
        .expect("pre-collecting test hook lock")
        .insert(execution_id, (reached, resume));
    (reached_rx, resume_tx)
}

#[cfg(test)]
async fn run_stage_out_test_hook(execution_id: &str) {
    let hook = {
        let Ok(mut armed) = STAGE_OUT_TEST_HOOKS.lock() else {
            return;
        };
        armed.remove(execution_id)
    };
    match hook {
        Some(StageOutTestHook::Crash) => {
            // This injected panic is deliberately after the full sync and
            // manifest persistence, before terminal result persistence.
            panic!("injected worker crash after workspace stage-out");
        }
        Some(StageOutTestHook::Pause { reached, resume }) => {
            let _ = reached.send(());
            let _ = resume.await;
        }
        None => {}
    }
}

#[cfg(test)]
async fn run_pre_collecting_test_hook(execution_id: &str) {
    let hook = PRE_COLLECTING_TEST_HOOKS
        .lock()
        .ok()
        .and_then(|mut armed| armed.remove(execution_id));
    if let Some((reached, resume)) = hook {
        let _ = reached.send(());
        let _ = resume.await;
    }
}

pub(super) async fn finish_command(
    backend: &DockerExecutionBackend,
    execution: &Arc<Mutex<RuntimeExecution>>,
    request: &ExecutionRequestV1,
    container_id: &str,
    command_id: &str,
    workspace: &ValidatedWorkspace,
    snapshot: Option<&WorkspaceSnapshot>,
) -> Result<(), ExecutionError> {
    let missing_exit_status_deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let inspected = loop {
        let inspected = match tokio::time::timeout_at(
            tokio::time::Instant::now() + cleanup::remaining(request.deadline_unix_ms),
            backend.docker.inspect_exec(command_id),
        )
        .await
        {
            Ok(Ok(inspected)) => inspected,
            Ok(Err(error)) => {
                return state::interrupted_with(
                    execution,
                    state::operation_disconnected("inspect the workspace command", &error),
                );
            }
            Err(_) => {
                return cleanup::deadline_failure(backend, execution).await;
            }
        };
        if inspected.exit_code.is_some() {
            break inspected;
        }
        if inspected.running == Some(false)
            && tokio::time::Instant::now() >= missing_exit_status_deadline
        {
            state::interrupted_with(
                execution,
                ExecutionError::new(
                    ExecutionErrorKind::WorkerDisconnected,
                    "Docker stopped the command without recording its exit status",
                    true,
                ),
            )?;
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    let code = inspected
        .exit_code
        .and_then(|code| i32::try_from(code).ok());
    let Some(code) = code else {
        state::interrupted_with(
            execution,
            ExecutionError::new(
                ExecutionErrorKind::WorkerDisconnected,
                "Docker returned an invalid command exit status",
                true,
            ),
        )?;
        return Ok(());
    };
    #[cfg(test)]
    run_pre_collecting_test_hook(&request.execution_id).await;
    if !state::mark_collecting(execution)? {
        return Ok(());
    }
    let stage_out_expected = sync::workspace_stage_out_enabled(request);
    let stage_out_started = Instant::now();
    let synchronized = sync::sync_workspace_if_active(
        execution,
        sync::WorkspaceSyncRequest {
            backend,
            container_id,
            workspace,
            snapshot,
            budget: request.resource_budget.temporary_disk_bytes,
            writable: sync::workspace_write_enabled(request),
            artifact_paths: request.policy.artifact_paths(),
            authorization_generation: &request.authorization_generation,
        },
    )
    .await;
    let artifacts = match synchronized {
        Ok(Some(artifacts)) => {
            if stage_out_expected {
                state::persist_stage_out(
                    execution,
                    elapsed_millis(stage_out_started),
                    Some(&artifacts),
                )?;
            }
            artifacts
        }
        Ok(None) => return Ok(()),
        Err(error) => {
            if stage_out_expected {
                state::persist_stage_out(execution, elapsed_millis(stage_out_started), None)?;
            }
            state::finish(
                execution,
                ExecutionLifecycleV1::Failed,
                Some(error.clone()),
                Some(code),
                false,
                false,
            )?;
            return Err(error);
        }
    };
    #[cfg(test)]
    run_stage_out_test_hook(&request.execution_id).await;
    let failure = (code != 0).then(|| {
        ExecutionError::new(
            ExecutionErrorKind::ExecutionFailed,
            "Command exited unsuccessfully",
            false,
        )
    });
    state::finish_with_artifacts(
        execution,
        if failure.is_some() {
            ExecutionLifecycleV1::Failed
        } else {
            ExecutionLifecycleV1::Succeeded
        },
        failure,
        Some(code),
        false,
        false,
        artifacts,
    )?;
    Ok(())
}

fn elapsed_millis(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}
