use super::super::{EngineFuture, failure};
use super::BollardApi;
use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};
use bollard::{
    body_full,
    query_parameters::{
        BuildImageOptionsBuilder, LogsOptionsBuilder, RemoveContainerOptionsBuilder,
        WaitContainerOptionsBuilder,
    },
};
use futures_util::StreamExt;

type PackageIdentity = crate::execution::docker::image_extension::builder::PackageArtifactIdentity;
type GeneratedContext = crate::execution::docker::image_extension::builder::GeneratedImageContext;

pub(super) fn image_package_state<'a>(
    api: &'a BollardApi,
    image_id: &'a str,
) -> EngineFuture<
    'a,
    Result<
        std::collections::BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>,
        PrerequisiteFailure,
    >,
> {
    super::super::image_derivation::image_package_state(api, image_id)
}

pub(super) fn build<'a>(
    api: &'a BollardApi,
    context: &'a GeneratedContext,
    platform: &'a str,
) -> EngineFuture<'a, Result<String, PrerequisiteFailure>> {
    Box::pin(async move {
        if !context.content_identity.starts_with("sha256:") || !platform.starts_with("linux/") {
            return Err(failure(
                PrerequisiteCode::CapabilitiesIncompatible,
                "Authorized image build context is invalid",
                false,
            ));
        }
        let tag = format!("koolade-derived:{}", &context.content_identity[7..]);
        match api.docker.inspect_image(&tag).await {
            Ok(inspect) => {
                let id = inspect.id.unwrap_or_default();
                if valid_image_id(&id)
                    && has_identity_labels(
                        inspect.config.and_then(|config| config.labels),
                        &context.content_identity,
                        &context.base_digest,
                    )
                {
                    return Ok(id);
                }
                return Err(failure(
                    PrerequisiteCode::ImageUnavailable,
                    "Content-addressed derived image cache failed identity verification",
                    false,
                ));
            }
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(_) => {
                return Err(failure(
                    PrerequisiteCode::DaemonStopped,
                    "Docker could not inspect the derived image cache",
                    false,
                ));
            }
        }
        let options = BuildImageOptionsBuilder::default()
            .dockerfile("Dockerfile")
            .t(&tag)
            .rm(true)
            .networkmode("none")
            .platform(platform)
            .build();
        let mut stream =
            api.docker
                .build_image(options, None, Some(body_full(context.tar.clone().into())));
        while let Some(result) = stream.next().await {
            let result = result.map_err(|_| {
                failure(
                    PrerequisiteCode::ImageUnavailable,
                    "Docker failed to build the authorized offline system image",
                    false,
                )
            })?;
            if result.error_detail.is_some() {
                return Err(failure(
                    PrerequisiteCode::ImageUnavailable,
                    "Docker rejected the authorized offline system image build",
                    false,
                ));
            }
        }
        let inspect = api.docker.inspect_image(&tag).await.map_err(|_| {
            failure(
                PrerequisiteCode::ImageUnavailable,
                "Docker built image could not be inspected",
                false,
            )
        })?;
        let id = inspect.id.unwrap_or_default();
        if !valid_image_id(&id)
            || !has_identity_labels(
                inspect.config.and_then(|config| config.labels),
                &context.content_identity,
                &context.base_digest,
            )
        {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Docker returned an invalid derived image identifier",
                false,
            ));
        }
        Ok(id)
    })
}

fn has_identity_labels(
    labels: Option<std::collections::HashMap<String, String>>,
    content_identity: &str,
    base_digest: &str,
) -> bool {
    labels.is_some_and(|labels| {
        labels
            .get("io.koolade.content")
            .is_some_and(|value| value == content_identity)
            && labels
                .get("io.koolade.base-digest")
                .is_some_and(|value| value == base_digest)
    })
}

pub(super) fn verify<'a>(
    api: &'a BollardApi,
    image_id: &'a str,
    packages: &'a [PackageIdentity],
) -> EngineFuture<'a, Result<(), PrerequisiteFailure>> {
    Box::pin(async move {
        if !valid_image_id(image_id) || packages.is_empty() {
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

fn valid_image_id(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
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
