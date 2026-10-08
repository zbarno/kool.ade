use super::ResourceBridge;
use std::{fs, sync::atomic::Ordering};

impl ResourceBridge {
    pub(crate) fn socket_path(&self) -> &std::path::Path {
        &self.socket
    }

    pub(crate) fn cache_path(&self) -> &std::path::Path {
        &self.cache_dir
    }

    pub(crate) fn npm_cache_path(&self) -> &std::path::Path {
        &self.npm_cache
    }

    pub(crate) fn npm_index_snapshot_path(&self) -> &std::path::Path {
        &self.npm_snapshot
    }

    pub(crate) fn cargo_cache_path(&self) -> &std::path::Path {
        &self.cargo_cache
    }

    pub(crate) fn attention_detail(&self) -> Option<String> {
        self.pending_attention.lock().ok()?.clone()
    }

    pub(crate) fn dependency_request(&self) -> Option<crate::harness::DependencyRequest> {
        self.pending_dependency.lock().ok()?.clone()
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
