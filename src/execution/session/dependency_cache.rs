use super::*;

impl ExecutionSession {
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
