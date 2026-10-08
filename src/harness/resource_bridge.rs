//! Mediated, read-only resource retrieval for implementation workers.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
};

mod adapter;
mod broker;
mod cargo;
pub(super) mod dependency;
mod fetch;
mod npm;
mod policy;
mod proxy;
#[cfg(test)]
mod tests;

const MAX_REQUEST_BYTES: usize = 16 * 1024;
const MAX_REQUESTS: usize = 100;
const MAX_IN_FLIGHT_REQUESTS: usize = 16;
const MAX_SESSION_BYTES: usize = 512 * 1024 * 1024;
pub(crate) const SANDBOX_RESOURCE_DIR: &str = "/tmp/koolade-resource-files";

pub(crate) fn prepared_npm_cache_path(worktree: &Path) -> anyhow::Result<PathBuf> {
    npm::persistent_cache_at(&crate::persistence::state_root(), worktree)
}

pub(crate) fn publish_npm_cache_index_snapshot(
    cache_root: &Path,
    snapshot_root: &Path,
) -> anyhow::Result<()> {
    npm::publish_index_snapshot(cache_root, snapshot_root)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ResourceRequest {
    #[serde(default)]
    action: ResourceAction,
    #[serde(default)]
    manager: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    dependency: Option<crate::harness::DependencyNeed>,
    #[serde(default)]
    dependency_request_id: Option<String>,
    #[serde(default)]
    retry_succeeded: Option<bool>,
    purpose: String,
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ResourceAction {
    #[default]
    Fetch,
    PrepareNpm,
    PrepareNugetAudit,
    UnsupportedManager,
    DependencyRequest,
    DependencyRetryResult,
}

#[derive(Debug, Deserialize, Serialize)]
struct ResourceResponse {
    status: String,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dependency_request: Option<crate::harness::DependencyRequest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dependency_result: Option<crate::harness::DependencyPreparationTelemetry>,
    #[serde(skip)]
    preparation: Option<crate::harness::DependencyPreparationTelemetry>,
    #[serde(skip)]
    bytes: usize,
}

pub(crate) use broker::ResourceBridge;

#[cfg(test)]
pub(crate) fn start_with_test_npm_preparation(
    worktree: &Path,
    task_id: Option<&str>,
    progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    state_root: &Path,
    archive: PathBuf,
    registry_url: String,
) -> anyhow::Result<ResourceBridge> {
    broker::start_with_state_root_and_npm_operations(
        worktree,
        task_id,
        progress,
        cancel,
        state_root,
        npm::test_preparation_operations(archive, registry_url),
    )
}

impl ResourceResponse {
    fn needs_attention(summary: String) -> Self {
        Self::needs_attention_with_bytes(summary, 0)
    }

    fn needs_attention_with_bytes(summary: String, bytes: usize) -> Self {
        Self {
            status: "needs_attention".into(),
            summary,
            content: None,
            path: None,
            dependency_request: None,
            dependency_result: None,
            preparation: None,
            bytes,
        }
    }

    fn allowed_text(summary: String, content: String) -> Self {
        let bytes = content.len();
        Self {
            status: "allowed".into(),
            summary,
            content: Some(content),
            path: None,
            dependency_request: None,
            dependency_result: None,
            preparation: None,
            bytes,
        }
    }

    fn allowed_file(summary: String, path: String, bytes: usize) -> Self {
        Self {
            status: "allowed".into(),
            summary,
            content: None,
            path: Some(path),
            dependency_request: None,
            dependency_result: None,
            preparation: None,
            bytes,
        }
    }

    fn prepared(summary: String) -> Self {
        Self {
            status: "prepared".into(),
            summary,
            content: None,
            path: None,
            dependency_request: None,
            dependency_result: None,
            preparation: None,
            bytes: 0,
        }
    }

    fn dependency_outcome(status: &str, mut request: crate::harness::DependencyRequest) -> Self {
        let summary = request.summary();
        if request.preparation.is_none() {
            request.preparation = Some(crate::harness::DependencyPreparationTelemetry {
                status: Some(match status {
                    "prepared" => crate::harness::DependencyPreparationStatus::Prepared,
                    "already_available" => {
                        crate::harness::DependencyPreparationStatus::AlreadyAvailable
                    }
                    "authorization_required" => {
                        crate::harness::DependencyPreparationStatus::AuthorizationRequired
                    }
                    "denied" => crate::harness::DependencyPreparationStatus::Denied,
                    "unsupported" => crate::harness::DependencyPreparationStatus::Unsupported,
                    "integrity_failure" => {
                        crate::harness::DependencyPreparationStatus::IntegrityFailure
                    }
                    "source_rejected" => {
                        crate::harness::DependencyPreparationStatus::SourceRejected
                    }
                    "credentials_required" => {
                        crate::harness::DependencyPreparationStatus::CredentialsRequired
                    }
                    _ => crate::harness::DependencyPreparationStatus::Error,
                }),
                ..Default::default()
            });
        }
        let dependency_result = request.preparation.clone();
        Self {
            status: status.into(),
            summary,
            content: None,
            path: None,
            dependency_request: Some(request),
            dependency_result,
            preparation: None,
            bytes: 0,
        }
    }

    fn with_preparation(
        mut self,
        preparation: crate::harness::DependencyPreparationTelemetry,
    ) -> Self {
        self.preparation = Some(preparation);
        self
    }
}

fn write_json(stream: &mut UnixStream, response: &ResourceResponse) -> io::Result<()> {
    serde_json::to_writer(&mut *stream, response).map_err(io::Error::other)?;
    stream.write_all(b"\n")
}

#[cfg(unix)]
fn set_private_dir(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}
