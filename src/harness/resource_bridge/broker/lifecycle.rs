use super::ResourceBridge;
use std::{fs, sync::atomic::Ordering};

impl ResourceBridge {
    pub(crate) fn bind_execution_session(
        &self,
        session: std::sync::Arc<crate::execution::session::ExecutionSession>,
    ) -> anyhow::Result<()> {
        let mut bound = self
            .execution_session
            .lock()
            .map_err(|_| anyhow::anyhow!("execution session binding is unavailable"))?;
        anyhow::ensure!(
            bound.is_none(),
            "resource broker already has an execution session"
        );
        *bound = Some(session);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn cache_snapshot(
        &self,
        task_id: &str,
        ecosystem: crate::execution::contract::DependencyCacheEcosystemV1,
        project_id: &str,
        resolved_integrity: &str,
        target_arch: &str,
        target_id: &str,
    ) -> anyhow::Result<super::super::cache_snapshot::CacheSnapshot> {
        if self
            .task_id
            .as_deref()
            .is_some_and(|bound| bound != task_id)
        {
            anyhow::bail!("cache snapshot task does not match its broker session");
        }
        let root = match ecosystem {
            crate::execution::contract::DependencyCacheEcosystemV1::Npm => &self.npm_cache,
            crate::execution::contract::DependencyCacheEcosystemV1::Cargo => &self.cargo_cache,
        };
        super::super::cache_snapshot::CacheSnapshot::capture(
            root,
            task_id,
            ecosystem,
            project_id,
            resolved_integrity,
            target_arch,
            target_id,
        )
    }

    pub(crate) fn socket_path(&self) -> &std::path::Path {
        &self.socket
    }

    pub(crate) fn cache_path(&self) -> &std::path::Path {
        &self.cache_dir
    }

    #[cfg(test)]
    pub(crate) fn npm_cache_path(&self) -> &std::path::Path {
        &self.npm_cache
    }

    pub(crate) fn attention_detail(&self) -> Option<String> {
        self.pending_attention.lock().ok()?.clone()
    }

    pub(crate) fn dependency_request(&self) -> Option<crate::harness::DependencyRequest> {
        self.pending_dependency.lock().ok()?.last().cloned()
    }
}

#[cfg(test)]
impl ResourceBridge {
    pub(crate) fn start_with_preparation_hook(
        worktree: &std::path::Path,
        task_id: Option<&str>,
        progress: std::sync::mpsc::Sender<crate::harness::LiveProgress>,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        hook: std::sync::Arc<dyn Fn() + Send + Sync>,
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
            Some(hook),
        )
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
