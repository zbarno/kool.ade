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
        };
        let upload = backend
            .docker
            .upload_to_container(
                container_id,
                Some(
                    bollard::query_parameters::UploadToContainerOptionsBuilder::default()
                        .path(destination)
                        .copy_uidgid("1")
                        .build(),
                ),
                bollard::body_full(snapshot.archive.clone().into()),
            )
            .await;
        if !matches!(upload, Ok(())) {
            return false;
        }
    }
    true
}
