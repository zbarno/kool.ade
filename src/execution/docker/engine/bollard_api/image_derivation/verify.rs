use super::{BollardApi, EngineFuture, PackageIdentity, PrerequisiteCode, PrerequisiteFailure};
use bollard::query_parameters::{
    LogsOptionsBuilder, RemoveContainerOptionsBuilder, WaitContainerOptionsBuilder,
};
use futures_util::StreamExt;

use super::failure;

pub fn verify<'a>(
    api: &'a BollardApi,
    image_id: &'a str,
    packages: &'a [PackageIdentity],
) -> EngineFuture<'a, Result<(), PrerequisiteFailure>> {
    Box::pin(async move {
        if !super::valid_image_id(image_id) || packages.is_empty() {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Derived image manifest request is invalid",
                false,
            ));
        }
        let mut command = vec![
            "-W".to_owned(),
            "-f=${Package}=${Version}=${Architecture}\\n".to_owned(),
        ];
        command.extend(packages.iter().map(|package| package.name.clone()));
        let config = bollard::models::ContainerCreateBody {
            image: Some(image_id.to_owned()),
            entrypoint: Some(vec!["/usr/bin/dpkg-query".into()]),
            cmd: Some(command),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            network_disabled: Some(true),
            host_config: Some(bollard::models::HostConfig {
                network_mode: Some("none".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let created = api
            .docker
            .create_container(None, config)
            .await
            .map_err(|_| {
                failure(
                    PrerequisiteCode::ImageUnavailable,
                    "Docker could not start the offline image manifest check",
                    false,
                )
            })?;
        let result = verify_container_manifest(&api.docker, &created.id, packages).await;
        let _ = api
            .docker
            .remove_container(
                &created.id,
                Some(RemoveContainerOptionsBuilder::default().force(true).build()),
            )
            .await;
        result
    })
}

async fn verify_container_manifest(
    docker: &bollard::Docker,
    container_id: &str,
    packages: &[PackageIdentity],
) -> Result<(), PrerequisiteFailure> {
    docker
        .start_container(container_id, None)
        .await
        .map_err(|_| {
            failure(
                PrerequisiteCode::ImageUnavailable,
                "Docker could not run the offline image manifest check",
                false,
            )
        })?;
    let mut wait = docker.wait_container(
        container_id,
        Some(
            WaitContainerOptionsBuilder::default()
                .condition("not-running")
                .build(),
        ),
    );
    let mut exit_code = None;
    while let Some(result) = wait.next().await {
        exit_code = Some(
            result
                .map_err(|_| {
                    failure(
                        PrerequisiteCode::ImageUnavailable,
                        "Docker image manifest check did not finish",
                        false,
                    )
                })?
                .status_code,
        );
    }
    if exit_code != Some(0) {
        return Err(failure(
            PrerequisiteCode::ImageUnavailable,
            "Derived image package manifest command failed",
            false,
        ));
    }
    let mut logs = docker.logs(
        container_id,
        Some(LogsOptionsBuilder::default().stdout(true).build()),
    );
    let mut output = String::new();
    while let Some(log) = logs.next().await {
        output.push_str(&String::from_utf8_lossy(
            &log.map_err(|_| {
                failure(
                    PrerequisiteCode::ImageUnavailable,
                    "Docker image manifest output could not be read",
                    false,
                )
            })?
            .into_bytes(),
        ));
        if output.len() > 64 * 1024 {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Docker image manifest output exceeded its limit",
                false,
            ));
        }
    }
    crate::execution::docker::image_extension::builder::verify_installed_manifest(&output, packages)
        .map_err(|_| {
            failure(
                PrerequisiteCode::ImageUnavailable,
                "Derived image installed package manifest does not match the authorized closure",
                false,
            )
        })
}
