//! Independent binary-version checks for dynamically assembled runtimes.
//!
//! A derived image label alone is not proof that its advertised toolchains
//! actually execute. This smoke container has no workspace or host mounts.

use std::{collections::HashMap, time::Duration};

use bollard::{
    models::{ContainerCreateBody, HostConfig},
    query_parameters::{RemoveContainerOptionsBuilder, WaitContainerOptionsBuilder},
};
use futures_util::StreamExt;

use super::{BollardApi, failure};
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    images::SelectedImage,
};

fn version_command(tool: &str, version: &str) -> Option<String> {
    if version.is_empty()
        || version.len() > 64
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
    {
        return None;
    }
    // Only these fixed commands can be executed by the verifier. The version
    // is a bounded exact-lock value, never a recipe-provided shell fragment.
    let command = match tool {
        "rustc" => format!("test \"$(rustc --version | cut -d ' ' -f 2)\" = \"{version}\""),
        "cargo" => format!("test \"$(cargo --version | cut -d ' ' -f 2)\" = \"{version}\""),
        "node" => format!("test \"$(node --version)\" = \"v{version}\""),
        "npm" => format!("test \"$(npm --version)\" = \"{version}\""),
        "python" => format!("test \"$(python --version | cut -d ' ' -f 2)\" = \"{version}\""),
        "go" => format!("test \"$(go version | cut -d ' ' -f 3)\" = \"go{version}\""),
        _ => return None,
    };
    Some(command)
}

pub(super) async fn verify(
    api: &BollardApi,
    image: &SelectedImage,
) -> Result<(), PrerequisiteFailure> {
    if image.reference != image.digest
        || !crate::execution::docker::images::is_digest(&image.digest)
        || image.runtime_user != "10001:10001"
    {
        return Err(invalid());
    }
    let inspected = api
        .docker
        .inspect_image(&image.reference)
        .await
        .map_err(|_| invalid())?;
    if inspected.id.as_deref() != Some(image.digest.as_str())
        || inspected.os.as_deref() != Some("linux")
        || inspected.architecture.as_deref() != image.platform.strip_prefix("linux/")
        || inspected.config.as_ref().and_then(|c| c.user.as_deref()) != Some("10001:10001")
    {
        return Err(invalid());
    }
    let mut checks = vec!["set -eu".to_owned()];
    for (tool, version) in &image.tool_versions {
        checks.push(version_command(tool, version).ok_or_else(invalid)?);
    }
    if checks.len() < 3 {
        return Err(invalid());
    }
    let body = ContainerCreateBody {
        image: Some(image.digest.clone()),
        user: Some("10001:10001".into()),
        entrypoint: Some(vec!["/bin/sh".into(), "-c".into(), checks.join("; ")]),
        env: Some(vec!["HOME=/tmp".into(), "RUSTUP_NO_UPDATE_CHECK=1".into()]),
        attach_stdout: Some(false),
        attach_stderr: Some(false),
        network_disabled: Some(true),
        working_dir: Some("/".into()),
        host_config: Some(HostConfig {
            privileged: Some(false),
            readonly_rootfs: Some(true),
            network_mode: Some("none".into()),
            ipc_mode: Some("private".into()),
            cap_drop: Some(vec!["ALL".into()]),
            cap_add: Some(Vec::new()),
            security_opt: Some(vec!["no-new-privileges:true".into()]),
            memory: Some(512 * 1024 * 1024),
            memory_swap: Some(512 * 1024 * 1024),
            cpu_period: Some(100_000),
            cpu_quota: Some(50_000),
            pids_limit: Some(32),
            mounts: Some(Vec::new()),
            binds: Some(Vec::new()),
            tmpfs: Some(HashMap::from([(
                "/tmp".into(),
                "rw,nosuid,nodev,noexec,size=16777216".into(),
            )])),
            ..Default::default()
        }),
        ..Default::default()
    };
    let created = api
        .docker
        .create_container(None, body)
        .await
        .map_err(|_| invalid())?;
    if created.id.is_empty() {
        return Err(invalid());
    }
    let completed = tokio::time::timeout(Duration::from_secs(45), async {
        api.docker
            .start_container(&created.id, None)
            .await
            .map_err(|_| invalid())?;
        let mut wait = api.docker.wait_container(
            &created.id,
            Some(
                WaitContainerOptionsBuilder::default()
                    .condition("not-running")
                    .build(),
            ),
        );
        let result = wait
            .next()
            .await
            .ok_or_else(invalid)?
            .map_err(|_| invalid())?;
        if result.status_code != 0 {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Derived runtime failed exact toolchain version verification",
                false,
            ));
        }
        Ok(())
    })
    .await;
    let cleanup = api
        .docker
        .remove_container(
            &created.id,
            Some(
                RemoveContainerOptionsBuilder::default()
                    .force(true)
                    .v(false)
                    .build(),
            ),
        )
        .await;
    if cleanup.is_err() {
        return Err(failure(
            PrerequisiteCode::ImageUnavailable,
            "Derived runtime version verification container could not be cleaned up",
            false,
        ));
    }
    completed.map_err(|_| {
        failure(
            PrerequisiteCode::ImageUnavailable,
            "Derived runtime toolchain verification timed out",
            false,
        )
    })?
}

fn invalid() -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::ImageUnavailable,
        "Derived runtime toolchain identity or verification request is invalid",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_whitelisted_and_cannot_inject_shell() {
        assert!(version_command("cargo", "1.98.1").is_some());
        assert!(version_command("node", "24.21.0").is_some());
        assert!(version_command("curl", "8.0").is_none());
        assert!(version_command("npm", "1.0\";touch /tmp/pwn").is_none());
        assert!(version_command("go", "").is_none());
    }
}
