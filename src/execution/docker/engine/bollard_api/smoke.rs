use std::time::Duration;

use bollard::{
    models::{ContainerCreateBody, HostConfig},
    query_parameters::{
        CreateContainerOptionsBuilder, RemoveContainerOptionsBuilder, WaitContainerOptionsBuilder,
    },
};
use futures_util::StreamExt;

use super::super::{BollardApi, failure, map_api_error};
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    images::SelectedImage,
};

impl BollardApi {
    pub(super) async fn smoke_test_impl(
        &self,
        image: &SelectedImage,
        container_name: &str,
        expected_engine_fingerprint: Option<&str>,
    ) -> Result<(), PrerequisiteFailure> {
        let body = smoke_test_body(image, container_name)?;
        let options = CreateContainerOptionsBuilder::default()
            .name(container_name)
            .build();
        let mut container_id = None;
        let result = tokio::time::timeout(Duration::from_secs(20), async {
            if let Some(expected) = expected_engine_fingerprint {
                self.require_approved_engine(image, expected).await?;
            }
            let created = self
                .docker
                .create_container(Some(options), body)
                .await
                .map_err(|error| map_api_error(&error))?;
            if created.id.is_empty() {
                return Err(failure(
                    PrerequisiteCode::DaemonStopped,
                    "Docker did not return an ID for the smoke container",
                    true,
                ));
            }
            container_id = Some(created.id.clone());
            if let Some(expected) = expected_engine_fingerprint {
                self.require_approved_engine(image, expected).await?;
            }
            self.docker
                .start_container(&created.id, None)
                .await
                .map_err(|error| map_api_error(&error))?;
            let response = self
                .docker
                .wait_container(
                    &created.id,
                    Some(
                        WaitContainerOptionsBuilder::default()
                            .condition("not-running")
                            .build(),
                    ),
                )
                .next()
                .await
                .ok_or_else(|| {
                    failure(
                        PrerequisiteCode::DaemonStopped,
                        "Docker ended the smoke container wait without an exit status",
                        true,
                    )
                })?
                .map_err(|error| map_api_error(&error))?;
            if response.status_code != 0 {
                return Err(failure(
                    PrerequisiteCode::CapabilitiesIncompatible,
                    "The approved smoke container exited with a failure status",
                    false,
                ));
            }
            Ok(())
        })
        .await;

        let cleanup_error = match smoke_cleanup_target(container_id.as_deref()) {
            Some(cleanup_target) => {
                let cleanup = tokio::time::timeout(
                    Duration::from_secs(5),
                    self.docker.remove_container(
                        cleanup_target,
                        Some(
                            RemoveContainerOptionsBuilder::default()
                                .force(true)
                                .v(false)
                                .build(),
                        ),
                    ),
                )
                .await;
                match cleanup {
                    Ok(Ok(())) => None,
                    Ok(Err(bollard::errors::Error::DockerResponseServerError {
                        status_code: 404,
                        ..
                    })) if result.as_ref().is_ok_and(|result| result.is_ok()) => None,
                    Ok(Err(_)) | Err(_) => Some(failure(
                        PrerequisiteCode::DaemonStopped,
                        "Could not confirm cleanup of the app-owned smoke container",
                        true,
                    )),
                }
            }
            None => None,
        };
        match result {
            Ok(Ok(())) => cleanup_error.map_or(Ok(()), Err),
            Ok(Err(mut error)) => {
                if let Some(cleanup_error) = cleanup_error {
                    error.message = format!("{}; {}", error.message, cleanup_error.message);
                }
                Err(error)
            }
            Err(_) => Err(cleanup_error.unwrap_or_else(|| {
                failure(
                    PrerequisiteCode::DaemonStopped,
                    "Smoke container exceeded its 20 second create/start/exit limit",
                    true,
                )
            })),
        }
    }
}

pub(super) fn smoke_cleanup_target(container_id: Option<&str>) -> Option<&str> {
    container_id.filter(|id| !id.is_empty())
}

pub(super) fn smoke_test_body(
    image: &SelectedImage,
    container_name: &str,
) -> Result<ContainerCreateBody, PrerequisiteFailure> {
    let scoped_id = container_name
        .strip_prefix("koolade-smoke-")
        .unwrap_or_default();
    if !(scoped_id.len() == 32 && scoped_id.bytes().all(|byte| byte.is_ascii_hexdigit())) {
        return Err(failure(
            PrerequisiteCode::CapabilitiesIncompatible,
            "Smoke container name is invalid",
            false,
        ));
    }
    let lock = crate::execution::docker::images::ImageLock::bundled()?;
    if !matches!(
        image.profile,
        crate::execution::docker::images::RuntimeProfile::General
            | crate::execution::docker::images::RuntimeProfile::Rust
            | crate::execution::docker::images::RuntimeProfile::Node
            | crate::execution::docker::images::RuntimeProfile::Python
            | crate::execution::docker::images::RuntimeProfile::Go
    ) {
        return Err(failure(
            PrerequisiteCode::ProfileUnsupported,
            "Smoke tests are restricted to approved runtime images; QA profiles remain disabled",
            false,
        ));
    }
    let architecture = image.platform.strip_prefix("linux/").unwrap_or_default();
    let approved = lock.select(image.profile, architecture)?;
    if approved != *image {
        return Err(failure(
            PrerequisiteCode::ImageDigestMismatch,
            "Smoke image does not match its exact approved profile and architecture lock entry",
            false,
        ));
    }
    Ok(ContainerCreateBody {
        user: Some("10001:10001".into()),
        env: Some(vec!["HOME=/tmp".into(), "CI=true".into()]),
        attach_stdin: Some(false),
        attach_stdout: Some(false),
        attach_stderr: Some(false),
        tty: Some(false),
        open_stdin: Some(false),
        stdin_once: Some(false),
        image: Some(image.reference.clone()),
        working_dir: Some("/".into()),
        entrypoint: Some(vec!["/bin/sh".into(), "-c".into(), "exit 0".into()]),
        labels: Some(std::collections::HashMap::from([(
            "koolade.smoke-test".into(),
            container_name.to_owned(),
        )])),
        host_config: Some(HostConfig {
            privileged: Some(false),
            readonly_rootfs: Some(true),
            network_mode: Some("none".into()),
            ipc_mode: Some("private".into()),
            cap_drop: Some(vec!["ALL".into()]),
            cap_add: Some(Vec::new()),
            security_opt: Some(vec!["no-new-privileges:true".into()]),
            memory: Some(128 * 1024 * 1024),
            memory_swap: Some(128 * 1024 * 1024),
            cpu_period: Some(100_000),
            cpu_quota: Some(25_000),
            pids_limit: Some(16),
            init: Some(true),
            auto_remove: Some(false),
            mounts: Some(Vec::new()),
            tmpfs: Some(std::collections::HashMap::new()),
            binds: Some(Vec::new()),
            ..Default::default()
        }),
        ..Default::default()
    })
}
