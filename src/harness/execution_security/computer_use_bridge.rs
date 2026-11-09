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
        mpsc::Sender,
    },
    thread,
    time::Duration,
};

use crate::{
    computer_use::mcp::{
        COMPUTER_USE_MCP_VERSION_V1, ComputerUseMcpRequestV1, ComputerUseMcpResponseV1,
        ComputerUseMcpStatusV1, validate_request,
    },
    harness::{ComputerUseMcpAccessV1, LiveProgress},
};

const REQUEST_LIMIT: usize = 64 * 1024;
const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;

pub(crate) fn tool_names() -> Vec<String> {
    [
        "koolade_computer_use_capabilities",
        "koolade_computer_use_request_session",
        "koolade_computer_use_start_session",
        "koolade_computer_use_observe",
        "koolade_computer_use_action",
        "koolade_computer_use_pause",
        "koolade_computer_use_resume",
        "koolade_computer_use_release",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[derive(Clone, Debug)]
pub(crate) struct ComputerUseBridgeBindingV1 {
    pub socket_path: PathBuf,
    pub bridge_session_id: String,
    pub capability_id: String,
    pub task_id: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct WireRequestV1 {
    protocol_version: u16,
    bridge_session_id: String,
    capability_id: String,
    task_id: String,
    request: ComputerUseMcpRequestV1,
}

pub(crate) struct ComputerUseBridge {
    stopping: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    pub binding: ComputerUseBridgeBindingV1,
}

impl ComputerUseBridge {
    pub(crate) fn start(
        scratch: &Path,
        access: Arc<ComputerUseMcpAccessV1>,
        cancel: Arc<AtomicBool>,
        progress: Sender<LiveProgress>,
    ) -> anyhow::Result<Self> {
        let socket_path = scratch.join("computer-use.sock");
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path)?;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let binding = ComputerUseBridgeBindingV1 {
            socket_path,
            bridge_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            capability_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            task_id: access.caller.identity.task_id.clone(),
        };
        let stopping = Arc::new(AtomicBool::new(false));
        let thread_stopping = stopping.clone();
        let thread_binding = binding.clone();
        let worker = thread::Builder::new()
            .name("koolade-computer-use-mcp".into())
            .spawn(move || {
                while !thread_stopping.load(Ordering::Acquire) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let _ =
                                serve_request(stream, &thread_binding, &access, &cancel, &progress);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(_) => break,
                    }
                }
            })?;
        Ok(Self {
            stopping,
            worker: Some(worker),
            binding,
        })
    }
}

impl Drop for ComputerUseBridge {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = std::fs::remove_file(&self.binding.socket_path);
    }
}

pub(crate) fn call(
    config_path: &Path,
    binding: &ComputerUseBridgeBindingV1,
    request: ComputerUseMcpRequestV1,
) -> anyhow::Result<ComputerUseMcpResponseV1> {
    validate_request(&request).map_err(anyhow::Error::msg)?;
    let expected_socket = config_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("MCP config has no private parent directory"))?
        .join("computer-use.sock");
    anyhow::ensure!(
        expected_socket == binding.socket_path,
        "Computer Use socket escaped the private MCP session"
    );
    let mut stream = UnixStream::connect(&binding.socket_path)?;
    // Consent can remain pending for 60 seconds, then guest readiness can
    // take up to 120 seconds. Keep the bridge open through both bounded waits.
    stream.set_read_timeout(Some(Duration::from_secs(200)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let wire = WireRequestV1 {
        protocol_version: COMPUTER_USE_MCP_VERSION_V1,
        bridge_session_id: binding.bridge_session_id.clone(),
        capability_id: binding.capability_id.clone(),
        task_id: binding.task_id.clone(),
        request,
    };
    let bytes = serde_json::to_vec(&wire)?;
    anyhow::ensure!(
        bytes.len() <= REQUEST_LIMIT,
        "Computer Use request exceeds its size limit"
    );
    stream.write_all(&bytes)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    let mut line = Vec::new();
    BufReader::new(stream)
        .take((RESPONSE_LIMIT + 1) as u64)
        .read_until(b'\n', &mut line)?;
    anyhow::ensure!(
        line.len() <= RESPONSE_LIMIT,
        "Computer Use response exceeds its size limit"
    );
    let response: ComputerUseMcpResponseV1 = serde_json::from_slice(&line)?;
    response.validate().map_err(anyhow::Error::msg)?;
    Ok(response)
}

fn serve_request(
    mut stream: UnixStream,
    binding: &ComputerUseBridgeBindingV1,
    access: &ComputerUseMcpAccessV1,
    cancel: &AtomicBool,
    progress: &Sender<LiveProgress>,
) -> anyhow::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(200)))?;
    let mut line = Vec::new();
    BufReader::new(stream.try_clone()?)
        .take((REQUEST_LIMIT + 1) as u64)
        .read_until(b'\n', &mut line)?;
    anyhow::ensure!(
        line.len() <= REQUEST_LIMIT,
        "Computer Use request exceeds its size limit"
    );
    let response = (|| {
        let request: WireRequestV1 = serde_json::from_slice(&line)?;
        anyhow::ensure!(
            request.protocol_version == COMPUTER_USE_MCP_VERSION_V1
                && request.bridge_session_id == binding.bridge_session_id
                && request.capability_id == binding.capability_id
                && request.task_id == binding.task_id,
            "Computer Use call does not match its app-owned bridge"
        );
        validate_request(&request.request).map_err(anyhow::Error::msg)?;
        anyhow::ensure!(
            !cancel.load(Ordering::Acquire),
            "Computer Use invocation was cancelled"
        );
        let response = access.host.handle(&access.caller, request.request, cancel);
        response.validate().map_err(anyhow::Error::msg)?;
        if response.needs_attention() {
            let update = LiveProgress {
                task_uid: Some(access.caller.identity.task_id.clone()),
                activity: Some(format!("Computer Use is blocked: {}", response.summary)),
                ..Default::default()
            };
            let _ = progress.send(update);
        }
        Ok::<_, anyhow::Error>(response)
    })()
    .unwrap_or_else(|error| ComputerUseMcpResponseV1 {
        schema_version: COMPUTER_USE_MCP_VERSION_V1,
        status: ComputerUseMcpStatusV1::Denied,
        summary: "Computer Use request was denied by the application boundary.".into(),
        reason_code: Some(if error.to_string().contains("cancelled") {
            "cancelled".into()
        } else {
            "authorization_failed".into()
        }),
        session_id: None,
        capabilities: Vec::new(),
        observation: None,
        action_outcome: None,
    });
    let bytes = serde_json::to_vec(&response)?;
    anyhow::ensure!(
        bytes.len() <= RESPONSE_LIMIT,
        "Computer Use response exceeds its size limit"
    );
    stream.write_all(&bytes)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}
