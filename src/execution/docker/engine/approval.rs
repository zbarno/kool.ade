use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::execution::docker::images::SelectedImage;

use super::{DockerEndpoint, EngineSnapshot};

#[derive(Serialize)]
struct EngineBinding<'a> {
    endpoint_identity: String,
    daemon_id: &'a str,
    daemon_version: &'a str,
    api_version: &'a str,
    minimum_api_version: &'a str,
    os_type: &'a str,
    architecture: &'a str,
    security_options: &'a [String],
    rootless: bool,
    native_linux: bool,
    docker_desktop: bool,
    memory_limit: bool,
    cpu_quota: bool,
    pids_limit: bool,
    cgroup_version: &'a Option<String>,
    cgroup_driver: &'a Option<String>,
    seccomp_enabled: bool,
    docker_storage_accessible: bool,
    minimum_disk_bytes: u64,
    image_reference: &'a str,
    image_digest: &'a str,
    image_platform: &'a str,
}

pub(crate) fn fingerprint(
    endpoint: &DockerEndpoint,
    snapshot: &EngineSnapshot,
    image: &SelectedImage,
) -> anyhow::Result<String> {
    let binding = EngineBinding {
        endpoint_identity: endpoint.identity(),
        daemon_id: &snapshot.daemon_id,
        daemon_version: &snapshot.daemon_version,
        api_version: &snapshot.api_version,
        minimum_api_version: &snapshot.minimum_api_version,
        os_type: &snapshot.os_type,
        architecture: &snapshot.architecture,
        security_options: &snapshot.security_options,
        rootless: snapshot.rootless,
        native_linux: snapshot.native_linux,
        docker_desktop: snapshot.docker_desktop,
        memory_limit: snapshot.memory_limit,
        cpu_quota: snapshot.cpu_quota,
        pids_limit: snapshot.pids_limit,
        cgroup_version: &snapshot.cgroup_version,
        cgroup_driver: &snapshot.cgroup_driver,
        seccomp_enabled: snapshot.seccomp_enabled,
        docker_storage_accessible: snapshot.docker_storage_accessible,
        minimum_disk_bytes: snapshot.minimum_disk_bytes,
        image_reference: &image.reference,
        image_digest: &image.digest,
        image_platform: &image.platform,
    };
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&binding)?)
    ))
}
