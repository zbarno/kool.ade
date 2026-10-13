use crate::execution::session::ExecutionSession;
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

mod cache;
mod serve;
pub(crate) use cache::stage_cache;
#[cfg(test)]
pub(crate) use serve::prepare_remote_dependency;
use serve::serve_request;

const MESSAGE_LIMIT: usize = 65_536;
const CACHE_HEADER_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(super) struct SessionBinding {
    pub session_id: String,
    pub capability_id: String,
    pub task_id: String,
    pub workspace_id: String,
    pub access: String,
    pub target_id: String,
    pub target_arch: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ToolRequest {
    session_id: String,
    capability_id: String,
    task_id: String,
    workspace_id: String,
    access: String,
    command: String,
    timeout_seconds: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CacheStageRequest {
    session_id: String,
    capability_id: String,
    task_id: String,
    workspace_id: String,
    access: String,
    target_id: String,
    target_arch: String,
    cache_ref: crate::execution::contract::DependencyCacheRefV1,
    manifest: crate::harness::resource_bridge::CacheSnapshotManifest,
    archive_bytes: usize,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DependencyRelayStart {
    session_id: String,
    capability_id: String,
    task_id: String,
    workspace_id: String,
    access: String,
    need: crate::harness::DependencyNeed,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DependencyRelayStartResponse {
    pub(crate) request: crate::execution::remote::dependency::DependencyRelayRequestV1,
    pub(crate) broker_response: serde_json::Value,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum BridgeRequest {
    CacheStage(Box<CacheStageRequest>),
    DependencyStart(DependencyRelayStart),
    Tool(ToolRequest),
}

#[derive(Deserialize, Serialize)]
pub(super) struct ToolResponse {
    pub(super) stdout: String,
    pub(super) stderr: String,
    pub(super) exit_code: Option<i32>,
    pub(super) is_error: bool,
}

pub(super) struct SessionBridge {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    pub binding: SessionBinding,
    socket_path: PathBuf,
}

impl SessionBridge {
    pub fn start(
        scratch: &Path,
        session: Arc<ExecutionSession>,
        task_id: String,
        access: String,
        cancel: Arc<AtomicBool>,
        resource_bridge: Option<Arc<crate::harness::resource_bridge::ResourceBridge>>,
    ) -> anyhow::Result<Self> {
        let socket_path = scratch.join("execution.sock");
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path)?;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let workspace = session.workspace();
        let binding = SessionBinding {
            session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            capability_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            task_id,
            workspace_id: format!(
                "{}:{}:{}",
                workspace.project_id, workspace.task_id, workspace.source_sha
            ),
            access,
            target_id: session.target_id().to_owned(),
            target_arch: session.cache_target_arch(),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_binding = binding.clone();
        let thread_resource_bridge = resource_bridge;
        let worker = thread::Builder::new()
            .name("koolade-execution-session".into())
            .spawn(move || {
                while !thread_stop.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let _ = serve_request(
                                stream,
                                &session,
                                &thread_binding,
                                &cancel,
                                thread_resource_bridge.as_deref(),
                            );
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(_) => break,
                    }
                }
            })?;
        Ok(Self {
            stop,
            worker: Some(worker),
            binding,
            socket_path,
        })
    }
}

impl Drop for SessionBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

pub(super) fn call(
    config_path: &Path,
    binding: &SessionBinding,
    command: &str,
    timeout_seconds: u64,
) -> anyhow::Result<ToolResponse> {
    let socket_path = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("MCP config has no private parent directory"))?
        .join("execution.sock");
    let mut stream = UnixStream::connect(socket_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(
        timeout_seconds.clamp(1, 1200) + 10,
    )))?;
    let request = ToolRequest {
        session_id: binding.session_id.clone(),
        capability_id: binding.capability_id.clone(),
        task_id: binding.task_id.clone(),
        workspace_id: binding.workspace_id.clone(),
        access: binding.access.clone(),
        command: command.to_owned(),
        timeout_seconds,
    };
    serde_json::to_writer(&mut stream, &request)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    let mut line = Vec::new();
    BufReader::new(stream)
        .take((MESSAGE_LIMIT + 1) as u64)
        .read_until(b'\n', &mut line)?;
    anyhow::ensure!(
        line.len() <= MESSAGE_LIMIT,
        "Execution tool response exceeds size limit"
    );
    Ok(serde_json::from_slice(&line)?)
}

pub(crate) fn request_dependency(
    config_path: &Path,
    binding: &SessionBinding,
    need: crate::harness::DependencyNeed,
) -> anyhow::Result<DependencyRelayStartResponse> {
    let socket_path = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("MCP config has no private parent directory"))?
        .join("execution.sock");
    let mut stream = UnixStream::connect(socket_path)?;
    let request = DependencyRelayStart {
        session_id: binding.session_id.clone(),
        capability_id: binding.capability_id.clone(),
        task_id: binding.task_id.clone(),
        workspace_id: binding.workspace_id.clone(),
        access: binding.access.clone(),
        need,
    };
    serde_json::to_writer(&mut stream, &BridgeRequest::DependencyStart(request))?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    let mut line = Vec::new();
    BufReader::new(stream)
        .take(MESSAGE_LIMIT as u64 + 1)
        .read_until(b'\n', &mut line)?;
    anyhow::ensure!(
        line.len() <= MESSAGE_LIMIT,
        "Dependency relay response exceeds the limit"
    );
    serde_json::from_slice(&line).map_err(Into::into)
}
