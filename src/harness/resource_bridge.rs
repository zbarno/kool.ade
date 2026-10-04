//! Mediated, read-only resource retrieval for implementation workers.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

mod fetch;
mod policy;
#[cfg(test)]
mod tests;

const MAX_REQUEST_BYTES: usize = 16 * 1024;
const MAX_REQUESTS: usize = 100;
const MAX_SESSION_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const SANDBOX_RESOURCE_DIR: &str = "/tmp/koolade-resource-files";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ResourceRequest {
    url: String,
    purpose: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ResourceResponse {
    status: String,
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip)]
    bytes: usize,
}

pub(crate) struct ResourceBridge {
    socket: PathBuf,
    cache_dir: PathBuf,
    pending_attention: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    requests: Arc<Mutex<Vec<thread::JoinHandle<()>>>>,
    temp: PathBuf,
}

impl ResourceBridge {
    pub(crate) fn start(worktree: &Path) -> anyhow::Result<Self> {
        let worktree = worktree.canonicalize()?;
        anyhow::ensure!(
            worktree.is_dir(),
            "Resource broker worktree is not a directory"
        );
        let temp = std::env::temp_dir().join(format!(
            "koolade-resources-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&temp)?;
        set_private_dir(&temp)?;
        let cache_dir = temp.join("files");
        fs::create_dir(&cache_dir)?;
        set_private_dir(&cache_dir)?;
        let socket = temp.join("resource.sock");
        let listener = UnixListener::bind(&socket)?;
        set_private_file(&socket)?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let attention = Arc::new(Mutex::new(None));
        let request_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let downloaded_bytes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let request_gate = Arc::new(Mutex::new(()));
        let thread_stop = stop.clone();
        let thread_attention = attention.clone();
        let thread_request_count = request_count.clone();
        let thread_downloaded_bytes = downloaded_bytes.clone();
        let thread_resource_dir = cache_dir.clone();
        let thread_request_gate = request_gate.clone();
        let thread_requests = Arc::new(Mutex::new(Vec::new()));
        let accepting_requests = thread_requests.clone();
        let worker = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((client, _)) => {
                        let resource_dir = thread_resource_dir.clone();
                        let attention = thread_attention.clone();
                        let request_count = thread_request_count.clone();
                        let downloaded_bytes = thread_downloaded_bytes.clone();
                        let request_gate = thread_request_gate.clone();
                        let request_worker = thread::spawn(move || {
                            let _ = serve(
                                client,
                                &resource_dir,
                                &attention,
                                &request_count,
                                &downloaded_bytes,
                                &request_gate,
                            );
                        });
                        if let Ok(mut workers) = accepting_requests.lock() {
                            workers.push(request_worker);
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            socket,
            cache_dir,
            pending_attention: attention,
            stop,
            worker: Some(worker),
            requests: thread_requests,
            temp,
        })
    }

    pub(crate) fn socket_path(&self) -> &Path {
        &self.socket
    }

    pub(crate) fn cache_path(&self) -> &Path {
        &self.cache_dir
    }

    pub(crate) fn attention_detail(&self) -> Option<String> {
        self.pending_attention.lock().ok()?.clone()
    }
}

impl Drop for ResourceBridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if let Ok(mut requests) = self.requests.lock() {
            for request in requests.drain(..) {
                let _ = request.join();
            }
        }
        let _ = fs::remove_dir_all(&self.temp);
    }
}

fn serve(
    mut client: UnixStream,
    worktree: &Path,
    attention: &Arc<Mutex<Option<String>>>,
    request_count: &Arc<std::sync::atomic::AtomicUsize>,
    downloaded_bytes: &Arc<std::sync::atomic::AtomicUsize>,
    request_gate: &Arc<Mutex<()>>,
) -> io::Result<()> {
    client.set_read_timeout(Some(Duration::from_secs(5)))?;
    client.set_write_timeout(Some(Duration::from_secs(30)))?;
    let mut request = Vec::new();
    let read = (&mut client)
        .take(MAX_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut request)?;
    if read == 0 || request.len() > MAX_REQUEST_BYTES {
        return write_json(
            &mut client,
            &ResourceResponse {
                status: "rejected".into(),
                summary: "Resource request is empty or too large.".into(),
                content: None,
                path: None,
                bytes: 0,
            },
        );
    }
    let parsed =
        serde_json::from_slice::<ResourceRequest>(request.strip_suffix(b"\n").unwrap_or(&request));
    let response = match parsed {
        Ok(request) => match prepare_request(
            worktree,
            &request,
            attention,
            request_count,
            downloaded_bytes,
            request_gate,
        ) {
            Ok(response) => response,
            Err(error) => ResourceResponse {
                status: "error".into(),
                summary: format!("Resource request could not be completed: {error:#}"),
                content: None,
                path: None,
                bytes: 0,
            },
        },
        Err(error) => ResourceResponse {
            status: "rejected".into(),
            summary: format!("Resource request was invalid: {error}"),
            content: None,
            path: None,
            bytes: 0,
        },
    };
    if response.status != "allowed"
        && let Ok(mut pending) = attention.lock()
        && pending.is_none()
    {
        *pending = Some(response.summary.clone());
    }
    write_json(&mut client, &response)
}

fn prepare_request(
    worktree: &Path,
    request: &ResourceRequest,
    attention: &Arc<Mutex<Option<String>>>,
    request_count: &std::sync::atomic::AtomicUsize,
    downloaded_bytes: &std::sync::atomic::AtomicUsize,
    request_gate: &Mutex<()>,
) -> anyhow::Result<ResourceResponse> {
    let _gate = request_gate
        .lock()
        .map_err(|_| anyhow::anyhow!("Resource request coordinator is unavailable"))?;
    let count = request_count.fetch_add(1, Ordering::Relaxed);
    anyhow::ensure!(
        count < MAX_REQUESTS,
        "Resource request limit reached for this task run"
    );
    let used = downloaded_bytes.load(Ordering::Relaxed);
    anyhow::ensure!(
        used < MAX_SESSION_BYTES,
        "Resource download budget reached for this task run"
    );
    let reservation = (MAX_SESSION_BYTES - used).min(fetch::MAX_RESOURCE_BYTES as usize);
    downloaded_bytes.fetch_add(reservation, Ordering::Relaxed);
    let response = fetch::retrieve(worktree, &request.url, &request.purpose, reservation as u64)?;
    if response.status == "needs_attention"
        && let Ok(mut pending) = attention.lock()
    {
        *pending = Some(response.summary.clone());
    }
    downloaded_bytes.fetch_sub(
        reservation.saturating_sub(response.bytes),
        Ordering::Relaxed,
    );
    Ok(response)
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
            bytes,
        }
    }

    fn allowed_file(summary: String, path: String, bytes: usize) -> Self {
        Self {
            status: "allowed".into(),
            summary,
            content: None,
            path: Some(path),
            bytes,
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
