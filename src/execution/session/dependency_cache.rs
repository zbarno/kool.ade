use super::*;

impl ExecutionSession {
    pub(crate) fn set_dependency_approval_waiting(
        &self,
        waiting: bool,
    ) -> Result<(), ExecutionError> {
        if !matches!(
            &self.target.location,
            crate::execution::contract::TargetLocation::Local
        ) {
            return Ok(());
        }
        self.verify_authorization()?;
        let handle = self
            .active
            .lock()
            .map_err(|_| internal_error())?
            .as_ref()
            .map(|(handle, _)| handle.clone());
        let Some(handle) = handle else {
            return Ok(());
        };
        self.backend.set_lifecycle_state(
            &handle,
            if waiting {
                crate::execution::contract::ExecutionLifecycleV1::WaitingForDependencyApproval
            } else {
                crate::execution::contract::ExecutionLifecycleV1::Running
            },
        )
    }

    pub(crate) fn retain_remote_capacity_lease(
        &self,
        lease: crate::persistence::remote_workers::RemoteWorkerCapacityLease,
    ) -> Result<(), ExecutionError> {
        let mut held = self
            .remote_capacity_lease
            .lock()
            .map_err(|_| internal_error())?;
        if held.is_some() {
            return Err(internal_error());
        }
        *held = Some(lease);
        Ok(())
    }

    pub(crate) fn renew_remote_capacity_lease(&self) -> Result<(), ExecutionError> {
        let mut held = self
            .remote_capacity_lease
            .lock()
            .map_err(|_| internal_error())?;
        if let Some(lease) = held.as_mut() {
            lease.renew().map_err(|_| {
                ExecutionError::new(
                    ExecutionErrorKind::WorkerDisconnected,
                    "Remote worker capacity reservation expired; reconnect before executing",
                    true,
                )
            })?;
        }
        Ok(())
    }

    pub(crate) fn mark_remote_capacity_running(&self) -> Result<(), ExecutionError> {
        let mut held = self
            .remote_capacity_lease
            .lock()
            .map_err(|_| internal_error())?;
        if let Some(lease) = held.as_mut() {
            lease.mark_running().map_err(|_| {
                ExecutionError::new(
                    ExecutionErrorKind::WorkerDisconnected,
                    "Remote worker capacity reservation could not be committed",
                    true,
                )
            })?;
        }
        Ok(())
    }

    pub(crate) fn rollback_rejected_remote_capacity_start(&self) -> Result<(), ExecutionError> {
        let mut held = self
            .remote_capacity_lease
            .lock()
            .map_err(|_| internal_error())?;
        if let Some(lease) = held.as_mut() {
            lease.rollback_rejected_start().map_err(|_| {
                ExecutionError::new(
                    ExecutionErrorKind::WorkerDisconnected,
                    "Remote worker capacity rejection could not be recorded",
                    true,
                )
            })?;
        }
        Ok(())
    }

    pub(crate) fn has_remote_capacity_lease(&self) -> Result<bool, ExecutionError> {
        Ok(self
            .remote_capacity_lease
            .lock()
            .map_err(|_| internal_error())?
            .is_some())
    }

    pub(crate) fn prepare_remote_capacity_start(
        &self,
        max_jobs: u32,
        observed_running_jobs: u32,
        worker_id: &str,
        host_key_fingerprint: &str,
    ) -> Result<(), ExecutionError> {
        let mut held = self
            .remote_capacity_lease
            .lock()
            .map_err(|_| internal_error())?;
        let Some(lease) = held.as_mut() else {
            return Ok(());
        };
        if !lease.matches_worker(worker_id, host_key_fingerprint) {
            return Err(ExecutionError::new(
                ExecutionErrorKind::WorkerIncompatible,
                "Remote worker capacity reservation no longer matches enrollment",
                false,
            ));
        }
        lease
            .prepare_start(max_jobs, observed_running_jobs)
            .map_err(|_| {
                ExecutionError::new(
                    ExecutionErrorKind::WorkerDisconnected,
                    "Remote worker has no available execution slot",
                    true,
                )
            })
    }

    pub fn verify_authorization(&self) -> Result<(), ExecutionError> {
        if let Some(verifier) = &self.authorization_verifier {
            verifier.verify(&self.authorization_generation)?;
        }
        Ok(())
    }

    /// Stage a broker snapshot on the selected Docker worker and bind future
    /// commands in this session to its opaque, task-scoped reference.
    #[cfg(test)]
    pub(crate) fn stage_dependency_cache(
        &self,
        snapshot: crate::harness::resource_bridge::CacheSnapshot,
    ) -> Result<(), ExecutionError> {
        self.stage_dependency_cache_inner(None, snapshot)
    }

    pub(crate) fn stage_dependency_cache_for_request(
        &self,
        dependency_request_id: &str,
        snapshot: crate::harness::resource_bridge::CacheSnapshot,
    ) -> Result<(), ExecutionError> {
        crate::execution::contract::validate_identifier(dependency_request_id)
            .map_err(|_| policy_error("Dependency cache request identity is invalid"))?;
        self.stage_dependency_cache_inner(Some(dependency_request_id), snapshot)
    }

    fn stage_dependency_cache_inner(
        &self,
        dependency_request_id: Option<&str>,
        snapshot: crate::harness::resource_bridge::CacheSnapshot,
    ) -> Result<(), ExecutionError> {
        snapshot
            .manifest
            .validate_ref(&snapshot.cache_ref, &snapshot.archive)
            .map_err(|_| policy_error("Dependency cache snapshot failed integrity validation"))?;
        let reference = snapshot.cache_ref.clone();
        if reference.task_id != self.workspace.task_id
            || reference.project_id != self.workspace.project_id
            || reference.target_id != self.target.target_id
        {
            return Err(policy_error(
                "Dependency cache snapshot is outside the execution session scope",
            ));
        }
        let retained_snapshot = snapshot.clone();
        self.runtime.block_on(self.backend.stage_dependency_cache(
            crate::execution::backend::DependencyCacheSnapshot { snapshot },
        ))?;
        if let Some(dependency_request_id) = dependency_request_id {
            crate::execution::DependencyCacheSnapshotStore::new(
                crate::persistence::state_root().join("runtime-qa-dependency-caches"),
            )
            .put(dependency_request_id, &retained_snapshot)
            .map_err(|_| policy_error("Verified dependency cache could not be retained"))?;
        }
        let mut refs = self
            .dependency_cache_refs
            .lock()
            .map_err(|_| internal_error())?;
        if !refs.contains(&reference) {
            refs.push(reference);
        }
        Ok(())
    }
}
