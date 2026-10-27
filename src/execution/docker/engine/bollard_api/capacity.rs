use std::time::Duration;

use bollard::{
    models::{ContainerCreateBody, HostConfig},
    query_parameters::{
        CreateContainerOptionsBuilder, LogsOptionsBuilder, WaitContainerOptionsBuilder,
    },
};
use futures_util::{StreamExt, TryStreamExt};

mod cleanup;
use cleanup::{cleanup_created_probe, recover_orphaned_probes};
mod output;
use output::{capacity_output_failure, parse_capacity_output};

use super::super::failure;
use super::BollardApi;
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    images::{
        DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES, DESKTOP_CAPACITY_PROBE_MAX_STORAGE_BYTES, ImageLock,
        RuntimeProfile, SelectedImage, verify_image_observation,
    },
};

const MAX_PROBE_OUTPUT_BYTES: usize = 256;
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
static CAPACITY_PROBE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

impl BollardApi {
    pub(super) async fn desktop_storage_capacity_impl(
        &self,
        image: &SelectedImage,
        expected_engine_fingerprint: &str,
    ) -> Result<u64, PrerequisiteFailure> {
        let _probe_guard = CAPACITY_PROBE_LOCK.lock().await;
        verify_probe_image(image)?;
        self.require_approved_engine(image, expected_engine_fingerprint)
            .await?;
        recover_orphaned_probes(self, image, expected_engine_fingerprint).await?;
        let observation = self.inspect_image_without_pull(image).await?;
        verify_image_observation(
            image,
            &observation.repo_digests,
            &observation.os,
            &observation.architecture,
        )?;

        let container_name = format!("koolade-capacity-{}", uuid::Uuid::new_v4().simple());
        let options = CreateContainerOptionsBuilder::default()
            .name(&container_name)
            .build();
        let mut container_id = None;
        let result = tokio::time::timeout(PROBE_TIMEOUT, async {
            self.require_approved_engine(image, expected_engine_fingerprint)
                .await?;
            let created = self
                .docker
                .create_container(Some(options), storage_probe_body(image, &container_name)?)
                .await
                .map_err(|error| super::map_api_error(&error))?;
            if created.id.is_empty() {
                return Err(failure(
                    PrerequisiteCode::DaemonStopped,
                    "Docker did not return an ID for the app-owned VM capacity probe",
                    true,
                ));
            }
            container_id = Some(created.id.clone());
            self.require_approved_engine(image, expected_engine_fingerprint)
                .await?;
            self.docker
                .start_container(&created.id, None)
                .await
                .map_err(|error| super::map_api_error(&error))?;
            let mut wait = self.docker.wait_container(
                &created.id,
                Some(
                    WaitContainerOptionsBuilder::default()
                        .condition("not-running")
                        .build(),
                ),
            );
            let mut exit_status = None;
            while let Some(status) = wait.next().await {
                exit_status = Some(
                    status
                        .map_err(|error| super::map_api_error(&error))?
                        .status_code,
                );
            }
            if exit_status != Some(0) {
                return Err(failure(
                    PrerequisiteCode::InsufficientDisk,
                    "The locked Docker Desktop capacity probe did not complete successfully",
                    true,
                ));
            }
            self.require_approved_engine(image, expected_engine_fingerprint)
                .await?;
            read_capacity_output(&self.docker, &created.id).await
        })
        .await
        .unwrap_or_else(|_| {
            Err(failure(
                PrerequisiteCode::InsufficientDisk,
                "The Docker Desktop VM capacity probe exceeded its time limit",
                true,
            ))
        });

        let cleanup = cleanup_created_probe(
            self,
            image,
            expected_engine_fingerprint,
            container_id.as_deref(),
            &container_name,
        )
        .await;

        match (result, cleanup) {
            (Ok(bytes), Ok(())) => Ok(bytes),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Err(mut error), Err(cleanup_error)) => {
                error.message = format!("{}; {}", error.message, cleanup_error.message);
                Err(error)
            }
        }
    }
}

fn verify_probe_image(image: &SelectedImage) -> Result<(), PrerequisiteFailure> {
    if image.profile != RuntimeProfile::General {
        return Err(failure(
            PrerequisiteCode::ProfileUnsupported,
            "Only the bounded, locked General image can measure Docker Desktop VM storage",
            false,
        ));
    }
    let architecture = image.platform.strip_prefix("linux/").unwrap_or_default();
    let lock = ImageLock::bundled()?;
    let expected = lock.select(RuntimeProfile::General, architecture)?;
    let estimate = lock.size_estimate(RuntimeProfile::General, architecture)?;
    if expected != *image
        || estimate.estimated_pull_bytes > DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES
        || estimate.estimated_storage_bytes > DESKTOP_CAPACITY_PROBE_MAX_STORAGE_BYTES
    {
        return Err(failure(
            PrerequisiteCode::ImageDigestMismatch,
            "The locked General image exceeds the Desktop capacity-probe size bound",
            false,
        ));
    }
    Ok(())
}

fn storage_probe_body(
    image: &SelectedImage,
    container_name: &str,
) -> Result<ContainerCreateBody, PrerequisiteFailure> {
    let scoped_id = container_name
        .strip_prefix("koolade-capacity-")
        .unwrap_or_default();
    if !(scoped_id.len() == 32 && scoped_id.bytes().all(|byte| byte.is_ascii_hexdigit())) {
        return Err(failure(
            PrerequisiteCode::CapabilitiesIncompatible,
            "Docker Desktop capacity-probe identity is invalid",
            false,
        ));
    }
    verify_probe_image(image)?;
    Ok(ContainerCreateBody {
        user: Some("10001:10001".into()),
        env: Some(vec![
            "HOME=/tmp".into(),
            "CI=true".into(),
            "LC_ALL=C".into(),
        ]),
        attach_stdin: Some(false),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        tty: Some(false),
        open_stdin: Some(false),
        stdin_once: Some(false),
        image: Some(image.reference.clone()),
        working_dir: Some("/".into()),
        entrypoint: Some(vec![
            "/bin/sh".into(),
            "-c".into(),
            "exec df -P -B1 --output=fstype,avail /".into(),
        ]),
        labels: Some(std::collections::HashMap::from([(
            "koolade.vm-capacity-probe".into(),
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
            memory: Some(64 * 1024 * 1024),
            memory_swap: Some(64 * 1024 * 1024),
            cpu_period: Some(100_000),
            cpu_quota: Some(10_000),
            pids_limit: Some(8),
            init: Some(true),
            auto_remove: Some(false),
            mounts: Some(Vec::new()),
            tmpfs: Some(std::collections::HashMap::new()),
            ..Default::default()
        }),
        ..Default::default()
    })
}

async fn read_capacity_output(
    docker: &bollard::Docker,
    container_id: &str,
) -> Result<u64, PrerequisiteFailure> {
    let mut logs = docker.logs(
        container_id,
        Some(
            LogsOptionsBuilder::default()
                .stdout(true)
                .stderr(true)
                .tail("2")
                .build(),
        ),
    );
    let mut output = Vec::new();
    while let Some(log) = logs
        .try_next()
        .await
        .map_err(|error| super::map_api_error(&error))?
    {
        let bytes = log.into_bytes();
        if output.len().saturating_add(bytes.len()) > MAX_PROBE_OUTPUT_BYTES {
            return Err(capacity_output_failure());
        }
        output.extend_from_slice(&bytes);
    }
    parse_capacity_output(&output)
}

fn cleanup_failure() -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::DaemonStopped,
        "Could not confirm cleanup of the app-owned Docker Desktop capacity-probe container",
        true,
    )
}
