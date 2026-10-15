use super::*;

pub(super) fn stage_dependency_cache<'a>(
    backend: &'a DockerExecutionBackend,
    snapshot: crate::execution::backend::DependencyCacheSnapshot,
) -> BackendFuture<'a, ()> {
    Box::pin(async move {
        let snapshot = snapshot.snapshot;
        snapshot
            .manifest
            .validate_ref(&snapshot.cache_ref, &snapshot.archive)
            .map_err(|_| {
                crate::execution::docker::backend::internal_error_with_context(
                    "validating the dependency cache snapshot",
                )
            })?;
        if snapshot.cache_ref.target_id != "local"
            && snapshot.cache_ref.target_id != backend.identity.owner_instance_id
        {
            return Err(crate::execution::docker::backend::invalid_request());
        }
        let mut caches = backend.dependency_caches.lock().map_err(|_| {
            crate::execution::docker::backend::internal_error_with_context(
                "updating the dependency cache registry",
            )
        })?;
        let existing = caches
            .get(&snapshot.cache_ref)
            .map_or(0, |entry| entry.archive.len() as u64);
        let total = caches
            .values()
            .map(|entry| entry.archive.len() as u64)
            .sum::<u64>()
            .saturating_sub(existing)
            .saturating_add(snapshot.archive.len() as u64);
        if total > crate::harness::resource_bridge::MAX_CACHE_SNAPSHOT_BYTES {
            return Err(crate::execution::docker::backend::invalid_request());
        }
        caches.insert(snapshot.cache_ref.clone(), snapshot);
        Ok(())
    })
}
