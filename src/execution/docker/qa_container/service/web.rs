use bollard::models::{
    ContainerCreateBody, EndpointSettings, HealthConfig, HostConfig, NetworkConnectRequest,
    NetworkCreateRequest, NetworkingConfig,
};
use std::collections::HashMap;

use super::super::{QaWebPreviewSpec, denied};
use crate::execution::errors::ExecutionError;

mod cleanup;
mod launch;
#[cfg(test)]
#[path = "web/tests.rs"]
mod tests;
pub use cleanup::{cleanup_web_preview, recover_owned_web_preview};
pub use launch::start_web_preview;

const OWNER: &str = "com.koolade.qa.owner";
const RUN: &str = "com.koolade.qa.run";
const TASK: &str = "com.koolade.qa.task";
const ROLE: &str = "com.koolade.qa.role";

pub(super) fn validate_spec(spec: &QaWebPreviewSpec<'_>) -> Result<(), ExecutionError> {
    spec.request
        .validate()
        .map_err(|_| denied("QA web request is invalid"))?;
    spec.recipe
        .validate()
        .map_err(|_| denied("QA startup recipe is invalid"))?;
    spec.resources
        .validate()
        .map_err(|_| denied("QA web resource budget is invalid"))?;
    let recipe_bytes = serde_json::to_vec(spec.recipe)
        .map_err(|_| denied("QA startup recipe could not be encoded"))?;
    let recipe_hash = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(recipe_bytes));
    let images_valid = spec.browser_image.profile
        == crate::execution::docker::images::RuntimeProfile::BrowserQa
        && spec.browser_image.platform == spec.service_image.platform
        && matches!(
            spec.service_image.profile,
            crate::execution::docker::images::RuntimeProfile::General
                | crate::execution::docker::images::RuntimeProfile::Node
                | crate::execution::docker::images::RuntimeProfile::Rust
                | crate::execution::docker::images::RuntimeProfile::Python
                | crate::execution::docker::images::RuntimeProfile::Go
                | crate::execution::docker::images::RuntimeProfile::BrowserQa
        );
    let mut required_tools = std::iter::once(spec.recipe.executable_id.as_str())
        .chain(spec.recipe.prerequisite_tool_ids.iter().map(String::as_str));
    let image_has_recipe_tools =
        required_tools.all(|tool| spec.service_image.tool_versions.contains_key(tool));
    if !images_valid
        || spec.request.relevant_platforms.as_slice() != [crate::runtime_qa::QaPlatform::Web]
        || spec.request.startup_recipe_ref != spec.recipe.recipe_id
        || spec.request.startup_recipe_sha256 != recipe_hash
        || !image_has_recipe_tools
        || spec.recipe.network_requirement
            != crate::project_runtime::NetworkRequirement::ApprovedServicePorts
        || !spec.recipe.allowed_env_keys.iter().any(|key| key == "PORT")
        || !(1024..=u16::MAX).contains(&spec.preview_port)
        || !valid_http_path(spec.health_path)
        || spec.artifact_upload.target != "/workspace"
        || spec.artifact_upload.archive.is_empty()
        || spec.artifact_upload.archive.len() as u64 > spec.resources.temporary_disk_bytes
        || spec.dependency_cache_refs.len() > 2
    {
        return Err(denied(
            "QA web preview inputs do not match an approved task runtime recipe and bounded internal target",
        ));
    }
    Ok(())
}

pub(super) fn labels(
    request: &crate::runtime_qa::QaRunRequestV1,
    role: &str,
) -> HashMap<String, String> {
    HashMap::from([
        (OWNER.into(), request.execution_session_id.clone()),
        (RUN.into(), request.qa_run_id.clone()),
        (TASK.into(), request.parent_task_uid.clone()),
        (ROLE.into(), role.into()),
    ])
}

pub(super) async fn remove_owned_network(
    docker: &bollard::Docker,
    name: &str,
    request: &crate::runtime_qa::QaRunRequestV1,
    role: &str,
) {
    let Ok(network) = docker.inspect_network(name, None).await else {
        return;
    };
    if network.labels.as_ref() != Some(&labels(request, role)) {
        return;
    }
    let _ = docker.remove_network(name).await;
}

pub(super) fn network_request(
    name: String,
    request: &crate::runtime_qa::QaRunRequestV1,
    role: &str,
) -> NetworkCreateRequest {
    NetworkCreateRequest {
        name,
        driver: Some("bridge".into()),
        scope: Some("local".into()),
        internal: Some(true),
        attachable: Some(false),
        labels: Some(labels(request, role)),
        ..Default::default()
    }
}

pub(super) fn endpoint(alias: &str) -> EndpointSettings {
    EndpointSettings {
        aliases: Some(vec![alias.into()]),
        ..Default::default()
    }
}

pub(super) fn service_body(
    spec: &QaWebPreviewSpec<'_>,
    network_id: &str,
) -> Result<ContainerCreateBody, ExecutionError> {
    let relative_cwd = if spec.recipe.working_directory_relative == "." {
        "/workspace".to_owned()
    } else {
        format!("/workspace/{}", spec.recipe.working_directory_relative)
    };
    let mut env = vec!["HOME=/tmp".into(), "CI=true".into(), "LANG=C.UTF-8".into()];
    env.push(format!("PORT={}", spec.preview_port));
    if spec.dependency_cache_refs.iter().any(|cache_ref| {
        matches!(
            cache_ref.ecosystem,
            crate::execution::contract::DependencyCacheEcosystemV1::Npm
        )
    }) {
        env.push("npm_config_cache=/tmp/.npm".into());
    }
    if spec.dependency_cache_refs.iter().any(|cache_ref| {
        matches!(
            cache_ref.ecosystem,
            crate::execution::contract::DependencyCacheEcosystemV1::Cargo
        )
    }) {
        env.push("CARGO_HOME=/tmp/.cargo".into());
    }
    if spec.recipe.allowed_env_keys.iter().any(|key| key == "HOST") {
        env.push("HOST=0.0.0.0".into());
    }
    let memory = i64::try_from(spec.resources.memory_bytes)
        .map_err(|_| denied("QA memory budget is invalid"))?;
    Ok(ContainerCreateBody {
        user: Some(spec.service_image.runtime_user.clone()),
        image: Some(spec.service_image.reference.clone()),
        entrypoint: Some(vec![spec.recipe.executable_id.clone()]),
        cmd: Some(spec.recipe.argv.clone()),
        env: Some(env),
        working_dir: Some(relative_cwd),
        attach_stdin: Some(false),
        attach_stdout: Some(false),
        attach_stderr: Some(false),
        tty: Some(false),
        open_stdin: Some(false),
        stdin_once: Some(false),
        labels: Some(labels(spec.request, "task-service")),
        host_config: Some(HostConfig {
            privileged: Some(false),
            readonly_rootfs: Some(true),
            network_mode: Some(network_id.into()),
            ipc_mode: Some("private".into()),
            cap_drop: Some(vec!["ALL".into()]),
            cap_add: Some(Vec::new()),
            security_opt: Some(vec!["no-new-privileges:true".into()]),
            memory: Some(memory),
            memory_swap: Some(memory),
            cpu_period: Some(100_000),
            cpu_quota: Some(i64::from(spec.resources.cpu_millis) * 100),
            pids_limit: Some(i64::from(spec.resources.process_count)),
            init: Some(true),
            auto_remove: Some(false),
            tmpfs: Some(HashMap::from([
                (
                    "/tmp".into(),
                    format!(
                        "rw,nosuid,nodev,noexec,size={}",
                        spec.resources.temporary_disk_bytes / 2
                    ),
                ),
                (
                    "/workspace".into(),
                    format!(
                        "rw,nosuid,nodev,size={}",
                        spec.resources.temporary_disk_bytes / 2
                    ),
                ),
            ])),
            ..Default::default()
        }),
        networking_config: Some(NetworkingConfig {
            endpoints_config: Some(HashMap::from([(network_id.into(), endpoint("qa-task"))])),
        }),
        ..Default::default()
    })
}

pub(super) fn relay_body(spec: &QaWebPreviewSpec<'_>, network_id: &str) -> ContainerCreateBody {
    ContainerCreateBody {
        user: Some(spec.browser_image.runtime_user.clone()),
        image: Some(spec.browser_image.reference.clone()),
        entrypoint: Some(vec!["node".into()]),
        cmd: Some(vec!["/opt/koolade-runtime-qa/relay.mjs".into()]),
        env: Some(vec![
            "HOME=/tmp".into(), "CI=true".into(), "LANG=C.UTF-8".into(),
            format!("QA_TARGET_PORT={}", spec.preview_port),
            format!("QA_HEALTH_PATH={}", spec.health_path),
        ]),
        labels: Some(labels(spec.request, "preview-relay")),
        host_config: Some(HostConfig {
            privileged: Some(false), readonly_rootfs: Some(true), network_mode: Some(network_id.into()),
            ipc_mode: Some("private".into()),
            cap_drop: Some(vec!["ALL".into()]), cap_add: Some(Vec::new()),
            security_opt: Some(vec!["no-new-privileges:true".into()]),
            memory: Some(256 * 1024 * 1024), memory_swap: Some(256 * 1024 * 1024),
            cpu_period: Some(100_000), cpu_quota: Some(50_000), pids_limit: Some(64),
            init: Some(true), auto_remove: Some(false),
            tmpfs: Some(HashMap::from([("/tmp".into(), "rw,nosuid,nodev,noexec,size=16777216".into())])),
            ..Default::default()
        }),
        healthcheck: Some(HealthConfig {
            test: Some(vec!["CMD".into(), "node".into(), "-e".into(),
                "fetch('http://127.0.0.1:8080/__koolade_health').then(r=>process.exit(r.status===204?0:1)).catch(()=>process.exit(1))".into()]),
            interval: Some(1_000_000_000), timeout: Some(2_000_000_000), retries: Some(3),
            start_period: Some(65_000_000_000), ..Default::default()
        }),
        networking_config: Some(NetworkingConfig {
            endpoints_config: Some(HashMap::from([(network_id.into(), endpoint("qa-relay"))])),
        }),
        ..Default::default()
    }
}

pub(super) fn connect_request(container: &str, alias: &str) -> NetworkConnectRequest {
    NetworkConnectRequest {
        container: container.into(),
        endpoint_config: Some(endpoint(alias)),
    }
}

fn valid_http_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && path.len() <= 1024
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && !path.split('/').any(|part| part == "..")
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

pub(super) fn readiness_timeout(
    recipe: &crate::project_runtime::ExecutionRecipeV1,
) -> std::time::Duration {
    std::time::Duration::from_secs(recipe.timeout_seconds.min(180))
}
