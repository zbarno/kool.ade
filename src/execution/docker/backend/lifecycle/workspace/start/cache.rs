use crate::execution::{
    contract::{DependencyCacheEcosystemV1, ExecutionRequestV1},
    docker::backend::DockerExecutionBackend,
};

pub(super) async fn install_dependency_caches(
    backend: &DockerExecutionBackend,
    container_id: &str,
    request: &ExecutionRequestV1,
) -> bool {
    if request.dependency_cache_refs.is_empty() {
        return true;
    }
    let caches = match backend.dependency_caches.lock() {
        Ok(caches) => request
            .dependency_cache_refs
            .iter()
            .filter_map(|cache_ref| {
                caches
                    .get(cache_ref)
                    .cloned()
                    .map(|snapshot| (cache_ref.clone(), snapshot))
            })
            .collect::<std::collections::HashMap<_, _>>(),
        Err(_) => return false,
    };
    for cache_ref in &request.dependency_cache_refs {
        let Some(snapshot) = caches.get(cache_ref) else {
            return false;
        };
        if snapshot
            .manifest
            .validate_ref(cache_ref, &snapshot.archive)
            .is_err()
        {
            return false;
        }
        let destination = match cache_ref.ecosystem {
            DependencyCacheEcosystemV1::Npm => "/home/executor/.npm",
            DependencyCacheEcosystemV1::Cargo => "/home/executor/.cargo",
            DependencyCacheEcosystemV1::Pip => "/home/executor/.cache/koolade/pip",
            DependencyCacheEcosystemV1::Go => "/home/executor/go",
        };
        if super::super::extract_archive(
            backend,
            container_id,
            destination,
            &snapshot.archive,
            false,
        )
        .await
        .is_err()
        {
            return false;
        }
    }
    true
}
