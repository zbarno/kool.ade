use super::ResourceBridge;
use std::{fs, sync::atomic::Ordering};

impl ResourceBridge {
    pub(crate) fn validate_dependency_cache_ref(
        &self,
        dependency_request_id: &str,
        cache_ref: &crate::execution::contract::DependencyCacheRefV1,
        project_id: &str,
        target_id: &str,
        target_arch: &str,
    ) -> anyhow::Result<()> {
        let task_id = self
            .task_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("dependency cache broker is not task-bound"))?;
        anyhow::ensure!(
            cache_ref.task_id == task_id
                && cache_ref.project_id == project_id
                && cache_ref.target_id == target_id
                && cache_ref.target_arch == target_arch,
            "dependency cache reference is outside the active task, project, or target"
        );
        let requests = self
            .pending_dependency
            .lock()
            .map_err(|_| anyhow::anyhow!("dependency request state is unavailable"))?;
        let request = requests
            .iter()
            .find(|request| request.id == dependency_request_id)
            .ok_or_else(|| anyhow::anyhow!("dependency cache has no matching broker request"))?;
        let expected_ecosystem = match request.need.ecosystem {
            crate::harness::PackageEcosystem::Npm => {
                crate::execution::contract::DependencyCacheEcosystemV1::Npm
            }
            crate::harness::PackageEcosystem::Cargo => {
                crate::execution::contract::DependencyCacheEcosystemV1::Cargo
            }
            _ => anyhow::bail!("dependency ecosystem has no verified cache adapter"),
        };
        anyhow::ensure!(
            request.task_id == task_id
                && request.status == crate::harness::DependencyRequestStatus::Prepared
                && request.preparation.as_ref().is_some_and(|prep| prep.status
                    == Some(crate::harness::DependencyPreparationStatus::Prepared))
                && request.need.lockfile_identity.as_deref()
                    == Some(cache_ref.resolved_integrity.as_str())
                && expected_ecosystem == cache_ref.ecosystem,
            "dependency cache reference does not match successful request preparation"
        );
        Ok(())
    }

    pub(crate) fn record_dependency_cache_ref(
        &self,
        dependency_request_id: &str,
        cache_ref: crate::execution::contract::DependencyCacheRefV1,
    ) -> anyhow::Result<()> {
        cache_ref.validate()?;
        let mut requests = self
            .pending_dependency
            .lock()
            .map_err(|_| anyhow::anyhow!("dependency request state is unavailable"))?;
        let request = requests
            .iter_mut()
            .find(|request| request.id == dependency_request_id)
            .ok_or_else(|| anyhow::anyhow!("dependency cache request disappeared"))?;
        let preparation = request
            .preparation
            .get_or_insert_with(crate::harness::DependencyPreparationTelemetry::default);
        anyhow::ensure!(
            request.status == crate::harness::DependencyRequestStatus::Prepared
                && preparation.status
                    == Some(crate::harness::DependencyPreparationStatus::Prepared)
                && request.need.lockfile_identity.as_deref()
                    == Some(cache_ref.resolved_integrity.as_str()),
            "dependency cache request changed after preparation"
        );
        if let Some(existing) = preparation
            .cache_refs
            .iter()
            .find(|existing| existing.manifest_sha256 == cache_ref.manifest_sha256)
        {
            anyhow::ensure!(
                existing == &cache_ref,
                "dependency cache reference changed after preparation"
            );
        } else {
            anyhow::ensure!(
                preparation.cache_refs.len() < 8
                    && preparation
                        .cache_refs
                        .iter()
                        .map(|cache_ref| cache_ref.size_bytes)
                        .sum::<u64>()
                        .saturating_add(cache_ref.size_bytes)
                        <= crate::harness::resource_bridge::MAX_CACHE_SNAPSHOT_BYTES,
                "dependency cache references exceed the task transfer limit"
            );
            preparation.cache_refs.push(cache_ref);
        }
        let request = request.clone();
        self.progress.send(crate::harness::LiveProgress {
            dependency_requests: vec![request],
            activity: Some("Verified dependency cache was retained for this task".into()),
            ..Default::default()
        })?;
        Ok(())
    }

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
