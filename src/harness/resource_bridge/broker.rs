//! Local socket lifecycle and request coordination for mediated resources.
use super::{
    MAX_REQUEST_BYTES, MAX_REQUESTS, MAX_SESSION_BYTES, ResourceAction, ResourceRequest,
    ResourceResponse, fetch, npm, set_private_dir, set_private_file, write_json,
};
use std::{
    fs,
    io::{self, Read},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

pub(crate) struct ResourceBridge {
    socket: PathBuf,
    cache_dir: PathBuf,
    npm_cache: PathBuf,
    pending_attention: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    requests: Arc<Mutex<Vec<thread::JoinHandle<()>>>>,
    temp: PathBuf,
}

struct BrokerContext<'a> {
    worktree: &'a Path,
    resource_dir: &'a Path,
    npm_cache: &'a Path,
    attention: &'a Arc<Mutex<Option<String>>>,
    request_count: &'a AtomicUsize,
    downloaded_bytes: &'a AtomicUsize,
    request_gate: &'a Mutex<()>,
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
        let npm_cache = npm::persistent_cache()?;
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
        let thread_npm_cache = npm_cache.clone();
        let thread_worktree = worktree.clone();
        let thread_request_gate = request_gate.clone();
        let thread_requests = Arc::new(Mutex::new(Vec::new()));
        let accepting_requests = thread_requests.clone();
        let worker = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((client, _)) => {
                        let resource_dir = thread_resource_dir.clone();
                        let worktree = thread_worktree.clone();
                        let attention = thread_attention.clone();
                        let request_count = thread_request_count.clone();
                        let downloaded_bytes = thread_downloaded_bytes.clone();
                        let npm_cache = thread_npm_cache.clone();
                        let request_gate = thread_request_gate.clone();
                        let request_worker = thread::spawn(move || {
                            let context = BrokerContext {
                                worktree: &worktree,
                                resource_dir: &resource_dir,
                                npm_cache: &npm_cache,
                                attention: &attention,
                                request_count: &request_count,
                                downloaded_bytes: &downloaded_bytes,
                                request_gate: &request_gate,
                            };
                            let _ = serve(client, &context);
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
            npm_cache,
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

    pub(crate) fn npm_cache_path(&self) -> &Path {
        &self.npm_cache
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

fn serve(mut client: UnixStream, context: &BrokerContext<'_>) -> io::Result<()> {
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
        Ok(request) => match prepare_request(context, &request) {
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
        && response.status != "prepared"
        && let Ok(mut pending) = context.attention.lock()
        && pending.is_none()
    {
        *pending = Some(response.summary.clone());
    }
    write_json(&mut client, &response)
}

fn prepare_request(
    context: &BrokerContext<'_>,
    request: &ResourceRequest,
) -> anyhow::Result<ResourceResponse> {
    let _gate = context
        .request_gate
        .lock()
        .map_err(|_| anyhow::anyhow!("Resource request coordinator is unavailable"))?;
    let count = context.request_count.fetch_add(1, Ordering::Relaxed);
    anyhow::ensure!(
        count < MAX_REQUESTS,
        "Resource request limit reached for this task run"
    );
    let used = context.downloaded_bytes.load(Ordering::Relaxed);
    anyhow::ensure!(
        used < MAX_SESSION_BYTES,
        "Resource download budget reached for this task run"
    );
    let response = match request.action {
        ResourceAction::Fetch => {
            let url = request
                .url
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Resource URL is required"))?;
            let reservation = (MAX_SESSION_BYTES - used).min(fetch::MAX_RESOURCE_BYTES as usize);
            context
                .downloaded_bytes
                .fetch_add(reservation, Ordering::Relaxed);
            let response = fetch::retrieve(
                context.resource_dir,
                url,
                &request.purpose,
                reservation as u64,
            )?;
            context.downloaded_bytes.fetch_sub(
                reservation.saturating_sub(response.bytes),
                Ordering::Relaxed,
            );
            response
        }
        ResourceAction::PrepareNpm => npm::prepare(
            context.worktree,
            context.resource_dir,
            context.npm_cache,
            &request.purpose,
            context.downloaded_bytes,
        )?,
        ResourceAction::UnsupportedManager => ResourceResponse::needs_attention(format!(
            "Automatic package cache preparation does not yet support {}. Kool.ad/e can currently prepare lockfile-pinned npm dependencies; this package manager needs an operator-provided cache or support.",
            request.manager.as_deref().unwrap_or("this package manager")
        )),
    };
    if response.status == "needs_attention"
        && let Ok(mut pending) = context.attention.lock()
    {
        *pending = Some(response.summary.clone());
    }
    Ok(response)
}
