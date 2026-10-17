#[cfg(test)]
use std::collections::HashMap;
use std::time::Duration;

use crate::execution::{
    contract::ExecutionRequestV1,
    docker::{
        backend::DockerExecutionBackend,
        sandbox::{ValidatedWorkspace, WorkspaceSnapshot, staging},
    },
    errors::ExecutionError,
};

#[cfg(test)]
static TERMINATION_ARCHIVE_OBSERVERS: std::sync::LazyLock<
    std::sync::Mutex<HashMap<String, tokio::sync::oneshot::Sender<String>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));

#[cfg(test)]
pub(in crate::execution::docker::backend) fn observe_cancel_race_archive(
    execution_id: String,
) -> (
    tokio::sync::oneshot::Receiver<String>,
    TerminationArchiveObserverGuard,
) {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    TERMINATION_ARCHIVE_OBSERVERS
        .lock()
        .expect("termination archive observer lock")
        .insert(execution_id.clone(), sender);
    (receiver, TerminationArchiveObserverGuard { execution_id })
}

#[cfg(test)]
pub(in crate::execution::docker::backend) struct TerminationArchiveObserverGuard {
    execution_id: String,
}

#[cfg(test)]
impl Drop for TerminationArchiveObserverGuard {
    fn drop(&mut self) {
        if let Ok(mut observers) = TERMINATION_ARCHIVE_OBSERVERS.lock() {
            observers.remove(&self.execution_id);
        }
    }
}

mod guard;
mod quiesce;

/// Terminate task processes while retaining the container's workspace tmpfs.
/// Only after the bounded live snapshot is captured may the container stop.
pub(in crate::execution::docker::backend::lifecycle) async fn terminate_and_sync(
    backend: &DockerExecutionBackend,
    container_id: &str,
    workspace: &ValidatedWorkspace,
    snapshot: Option<&WorkspaceSnapshot>,
    request: &ExecutionRequestV1,
) -> Result<Vec<crate::execution::contract::ArtifactManifestEntryV1>, ExecutionError> {
    super::super::control::verify_owned(backend, container_id, &request.execution_id).await?;
    let mut guard = guard::StopOnDrop::new(backend, container_id, &request.execution_id);
    let writable = super::sync::workspace_write_enabled(request);
    let artifact_paths = request.policy.artifact_paths();
    let captured = if writable || !artifact_paths.is_empty() {
        tokio::time::timeout(Duration::from_secs(60), async {
            backend.verify_authorization(&request.authorization_generation)?;
            let baseline = if writable {
                Some(snapshot.ok_or_else(super::state::disconnected)?)
            } else {
                None
            };
            quiesce::stop_tasks(backend, container_id, &request.execution_id).await?;
            let archive = super::sync::download_workspace(
                backend,
                container_id,
                request.resource_budget.temporary_disk_bytes,
            )
            .await?;
            #[cfg(test)]
            observe_archive_path(&request.execution_id, &archive);
            Ok::<_, ExecutionError>(Some((baseline, archive)))
        })
        .await
        .map_err(|_| super::super::control::operation_timeout())
        .and_then(|result| result)
    } else {
        Ok(None)
    };
    // Always terminate, including when capture or authorization failed.
    let stopped = super::cleanup::stop_owned(backend, container_id, &request.execution_id).await;
    if stopped.is_ok() {
        guard.disarm();
    }
    stopped?;
    if let Some((baseline, archive)) = captured? {
        backend.verify_authorization(&request.authorization_generation)?;
        return if let Some(baseline) = baseline {
            staging::synchronize_with_artifacts(
                workspace,
                baseline,
                &archive,
                request.resource_budget.temporary_disk_bytes,
                artifact_paths,
            )
        } else {
            staging::collect_artifacts(
                &archive,
                request.resource_budget.temporary_disk_bytes,
                artifact_paths,
            )
        };
    }
    Ok(Vec::new())
}

#[cfg(test)]
fn observe_archive_path(execution_id: &str, archive_bytes: &[u8]) {
    let observer = TERMINATION_ARCHIVE_OBSERVERS
        .lock()
        .ok()
        .and_then(|mut observers| observers.remove(execution_id));
    if let Some(observer) = observer {
        let summary = match tar::Archive::new(std::io::Cursor::new(archive_bytes)).entries() {
            Ok(entries) => {
                let mut names = Vec::new();
                let mut contains_path = false;
                for entry in entries {
                    let Ok(entry) = entry else {
                        if names.len() < 16 {
                            names.push("<entry-error>".to_owned());
                        }
                        continue;
                    };
                    let Ok(path) = entry.path() else {
                        if names.len() < 16 {
                            names.push("<path-error>".to_owned());
                        }
                        continue;
                    };
                    contains_path |= is_cancel_race_path(&path);
                    if names.len() < 16 {
                        names.push(
                            path.to_string_lossy()
                                .chars()
                                .filter(|character| !character.is_control())
                                .take(120)
                                .collect(),
                        );
                    }
                }
                format!(
                    "bytes={}, cancel-race-present={contains_path}, first_entries={names:?}",
                    archive_bytes.len()
                )
            }
            Err(_) => format!("bytes={}, archive-parse=failed", archive_bytes.len()),
        };
        let _ = observer.send(summary);
    }
}

#[cfg(test)]
fn is_cancel_race_path(path: &std::path::Path) -> bool {
    path.components()
        .filter(|component| *component != std::path::Component::CurDir)
        .eq(std::path::Path::new("cancel-race.txt").components())
}
