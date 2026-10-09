//! Local socket lifecycle and request coordination for mediated resources.
mod cache_locks;
mod context;
mod dependency_flow;
mod lifecycle;
mod server;
#[cfg(test)]
mod test_support;

#[cfg(test)]
pub(super) use test_support::start_with_state_root_and_npm_operations;

use super::{MAX_IN_FLIGHT_REQUESTS, cargo, npm, set_private_dir, set_private_file};
use crate::harness::DependencyRequest;
use context::BrokerContext;
use std::{
    fs, io,
    os::unix::net::UnixListener,
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
    npm_snapshot: PathBuf,
    cargo_cache: PathBuf,
    pending_attention: Arc<Mutex<Option<String>>>,
    pending_dependency: Arc<Mutex<Vec<DependencyRequest>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    requests: Arc<Mutex<Vec<thread::JoinHandle<()>>>>,
    temp: PathBuf,
}

struct InFlightRequest(Arc<AtomicUsize>);

struct TemporaryDirectoryGuard(Option<PathBuf>);

impl Drop for TemporaryDirectoryGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

impl Drop for InFlightRequest {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

impl ResourceBridge {
    #[cfg(test)]
    pub(crate) fn start(
        worktree: &Path,
        task_id: Option<&str>,
        progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
        cancel: Arc<AtomicBool>,
    ) -> anyhow::Result<Self> {
        Self::start_inner(
            worktree,
            None,
            task_id,
            progress,
            cancel,
            &crate::persistence::state_root(),
            None,
            None,
        )
    }

    pub(crate) fn start_for_task_repository(
        task_repository: &Path,
        runtime_source: Option<&Path>,
        task_id: Option<&str>,
        progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
        cancel: Arc<AtomicBool>,
    ) -> anyhow::Result<Self> {
        Self::start_inner(
            task_repository,
            runtime_source,
            task_id,
            progress,
            cancel,
            &crate::persistence::state_root(),
            None,
            None,
        )
    }

    #[cfg(test)]
    pub(crate) fn start_with_runtime_source(
        task_repository: &Path,
        runtime_source: Option<&Path>,
    ) -> anyhow::Result<Self> {
        let (progress, _updates) = std::sync::mpsc::channel();
        Self::start_for_task_repository(
            task_repository,
            runtime_source,
            None,
            progress,
            Arc::new(AtomicBool::new(false)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn start_inner(
        worktree: &Path,
        runtime_source: Option<&Path>,
        task_id: Option<&str>,
        progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
        cancel: Arc<AtomicBool>,
        state_root: &Path,
        npm_operations: Option<Arc<super::npm::SharedPreparationOperations>>,
        before_authorized: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> anyhow::Result<Self> {
        let worktree = worktree.canonicalize()?;
        anyhow::ensure!(
            worktree.is_dir(),
            "Resource broker worktree is not a directory"
        );
        let private_configuration = worktree.join(".git").try_exists()?
            && !crate::harness::pi_sandbox::runtime_config::paths_with_source(
                &worktree,
                runtime_source,
            )?
            .is_empty();
        let baseline_commit = super::dependency::baseline_commit(&worktree);
        let temp = std::env::temp_dir().join(format!(
            "koolade-resources-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&temp)?;
        let mut temp_guard = TemporaryDirectoryGuard(Some(temp.clone()));
        set_private_dir(&temp)?;
        let cache_dir = temp.join("files");
        fs::create_dir(&cache_dir)?;
        set_private_dir(&cache_dir)?;
        let npm_cache = npm::persistent_cache_at(state_root, &worktree)?;
        let npm_cache_gate = cache_locks::gate_for(&npm_cache)?;
        let npm_snapshot = temp.join("npm-index-snapshots");
        {
            let _guard = npm_cache_gate
                .lock()
                .map_err(|_| anyhow::anyhow!("npm cache coordination lock is unavailable"))?;
            npm::publish_index_snapshot(&npm_cache, &npm_snapshot)?;
        }
        let cargo_cache = cargo::persistent_cache_at(state_root)?;
        let cargo_cache_gate = cache_locks::gate_for(&cargo_cache)?;
        let socket = temp.join("resource.sock");
        let listener = UnixListener::bind(&socket)?;
        set_private_file(&socket)?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let attention = Arc::new(Mutex::new(None));
        let pending_dependency = Arc::new(Mutex::new(Vec::new()));
        let request_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let in_flight_requests = Arc::new(AtomicUsize::new(0));
        let downloaded_bytes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let thread_stop = stop.clone();
        let thread_attention = attention.clone();
        let thread_dependency = pending_dependency.clone();
        let thread_task_id = task_id.map(str::to_owned);
        let thread_baseline_commit = baseline_commit.clone();
        let thread_progress = progress.clone();
        let thread_cancel = cancel.clone();
        let thread_stop_signal = stop.clone();
        let thread_request_count = request_count.clone();
        let thread_in_flight_requests = in_flight_requests.clone();
        let thread_downloaded_bytes = downloaded_bytes.clone();
        let thread_resource_dir = cache_dir.clone();
        let thread_npm_cache = npm_cache.clone();
        let thread_npm_snapshot = npm_snapshot.clone();
        let thread_npm_cache_gate = npm_cache_gate.clone();
        let thread_npm_operations = npm_operations;
        let thread_before_authorized = before_authorized;
        let thread_cargo_cache = cargo_cache.clone();
        let thread_cargo_cache_gate = cargo_cache_gate.clone();
        let thread_worktree = worktree.clone();
        let thread_requests = Arc::new(Mutex::new(Vec::new()));
        let accepting_requests = thread_requests.clone();
        let worker = thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((client, _)) => {
                        reap_finished_requests(&accepting_requests);
                        if !acquire_request_slot(&thread_in_flight_requests) {
                            drop(client);
                            continue;
                        }
                        let in_flight_requests = thread_in_flight_requests.clone();
                        let baseline_commit = thread_baseline_commit.clone();
                        let resource_dir = thread_resource_dir.clone();
                        let worktree = thread_worktree.clone();
                        let attention = thread_attention.clone();
                        let dependency = thread_dependency.clone();
                        let task_id = thread_task_id.clone();
                        let progress = thread_progress.clone();
                        let cancel = thread_cancel.clone();
                        let stop = thread_stop_signal.clone();
                        let request_count = thread_request_count.clone();
                        let downloaded_bytes = thread_downloaded_bytes.clone();
                        let npm_cache = thread_npm_cache.clone();
                        let npm_snapshot = thread_npm_snapshot.clone();
                        let npm_cache_gate = thread_npm_cache_gate.clone();
                        let npm_operations = thread_npm_operations.clone();
                        let before_authorized = thread_before_authorized.clone();
                        let cargo_cache = thread_cargo_cache.clone();
                        let cargo_cache_gate = thread_cargo_cache_gate.clone();
                        let request_worker = thread::spawn(move || {
                            let _in_flight = InFlightRequest(in_flight_requests);
                            let context = BrokerContext {
                                worktree: &worktree,
                                private_configuration,
                                resource_dir: &resource_dir,
                                npm_cache: &npm_cache,
                                npm_snapshot: &npm_snapshot,
                                cargo_cache: &cargo_cache,
                                npm_operations: npm_operations.as_deref(),
                                attention: &attention,
                                dependency: &dependency,
                                task_id: task_id.as_deref(),
                                baseline_commit: baseline_commit.as_deref(),
                                progress: &progress,
                                cancel: &cancel,
                                stop: &stop,
                                request_count: &request_count,
                                downloaded_bytes: &downloaded_bytes,
                                before_authorized: before_authorized.as_deref(),
                                npm_cache_gate: &npm_cache_gate,
                                cargo_cache_gate: &cargo_cache_gate,
                            };
                            let _ = server::serve(client, &context);
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
        let bridge = Self {
            socket,
            cache_dir,
            npm_cache,
            npm_snapshot,
            cargo_cache,
            pending_attention: attention,
            pending_dependency,
            stop,
            worker: Some(worker),
            requests: thread_requests,
            temp,
        };
        temp_guard.0.take();
        Ok(bridge)
    }
}

fn acquire_request_slot(in_flight: &AtomicUsize) -> bool {
    in_flight
        .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |active| {
            (active < MAX_IN_FLIGHT_REQUESTS).then_some(active + 1)
        })
        .is_ok()
}

fn reap_finished_requests(requests: &Mutex<Vec<thread::JoinHandle<()>>>) {
    let Ok(mut requests) = requests.lock() else {
        return;
    };
    let mut index = 0;
    while index < requests.len() {
        if requests[index].is_finished() {
            let finished = requests.swap_remove(index);
            let _ = finished.join();
        } else {
            index += 1;
        }
    }
}
