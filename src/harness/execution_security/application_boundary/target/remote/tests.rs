use super::{require_rootless_docker, verify_task_capabilities};
use crate::execution::backend::BackendCapabilitiesV1;
use crate::persistence::execution_environment_settings::TaskCapabilityRequirements;

fn capabilities(rootless: Option<bool>) -> BackendCapabilitiesV1 {
    BackendCapabilitiesV1 {
        app_version: "test".into(),
        docker_version: Some("test".into()),
        docker_api_version: None,
        docker_rootless: rootless,
        docker_desktop: Some(false),
        linux_containers: Some(true),
        seccomp_enabled: Some(true),
        memory_limit: Some(true),
        cpu_quota: Some(true),
        pids_limit: Some(true),
        disk_available_bytes: Some(1024),
        gpu_available: false,
        display_available: false,
        current_execution_count: 0,
        maximum_execution_count: Some(1),
        resource_ceiling: crate::execution::contract::ResourceBudgetV1 {
            cpu_millis: 1,
            memory_bytes: 1,
            process_count: 1,
            temporary_disk_bytes: 1,
            output_bytes: 1,
        },
        trusted_image_digests: Vec::new(),
        policy_hash: "sha256:test".into(),
    }
}

#[test]
fn explicit_remote_target_requires_rootless_docker_evidence() {
    assert!(require_rootless_docker(&capabilities(Some(true))).is_ok());
    assert!(require_rootless_docker(&capabilities(Some(false))).is_err());
    assert!(require_rootless_docker(&capabilities(None)).is_err());
}

#[test]
fn explicit_remote_target_rejects_missing_required_capabilities() {
    let requirements = TaskCapabilityRequirements {
        requires_gpu: true,
        requires_display: false,
    };
    assert!(verify_task_capabilities(&capabilities(Some(true)), requirements).is_err());
    let mut supported = capabilities(Some(true));
    supported.gpu_available = true;
    assert!(verify_task_capabilities(&supported, requirements).is_ok());
}
