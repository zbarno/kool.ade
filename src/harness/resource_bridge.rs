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
    #[serde(skip)]
    bytes: usize,
}

pub(crate) use broker::ResourceBridge;

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
            bytes: 0,
        }
    }

    fn dependency_outcome(status: &str, request: crate::harness::DependencyRequest) -> Self {
        let summary = request.summary();
        Self {
            status: status.into(),
            summary,
            content: None,
            path: None,
            dependency_request: Some(request),
            bytes: 0,
        }
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
