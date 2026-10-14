use bollard::{Docker, query_parameters::ListImagesOptions};
use futures_util::StreamExt;

use super::{
    DockerApi, EngineFuture, EngineSnapshot, daemon_storage_capacity, failure, map_api_error,
};
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure, parse_architecture},
    images::{ImageObservation, SelectedImage},
};

pub(super) struct BollardApi {
    docker: Docker,
    endpoint: super::DockerEndpoint,
    minimum_disk_bytes: u64,
}

mod smoke;

impl BollardApi {
    pub(super) fn new(
        docker: Docker,
        endpoint: super::DockerEndpoint,
        minimum_disk_bytes: u64,
    ) -> Self {
        Self {
            docker,
            endpoint,
            minimum_disk_bytes,
        }
    }
}

impl DockerApi for BollardApi {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>> {
        Box::pin(async move {
            let version = self
                .docker
                .version()
                .await
                .map_err(|error| map_api_error(&error))?;
            let info = self
                .docker
                .info()
                .await
                .map_err(|error| map_api_error(&error))?;
            let security_options = info.security_options.unwrap_or_default();
            let rootless = security_options
                .iter()
                .any(|option| option.split(',').any(|part| part == "name=rootless"));
            let seccomp_enabled = security_options.iter().any(|option| {
                option
                    .split(',')
                    .any(|part| part == "name=seccomp" || part.starts_with("name=seccomp,"))
            });
            let (disk_available_bytes, docker_storage_accessible) = daemon_storage_capacity(
                info.docker_root_dir.as_deref(),
                rootless,
                self.endpoint.is_desktop(),
            );
            let os_type = info.os_type.unwrap_or_default();
            let architecture = parse_architecture(version.arch.as_deref().unwrap_or(""))
                .or_else(|| parse_architecture(info.architecture.as_deref().unwrap_or("")))
                .unwrap_or("unsupported")
                .to_owned();
            Ok(EngineSnapshot {
                daemon_id: info.id.unwrap_or_default(),
                daemon_version: version.version.unwrap_or_default(),
                api_version: version.api_version.unwrap_or_default(),
                minimum_api_version: version.min_api_version.unwrap_or_default(),
                os_type,
                architecture,
                security_options,
                rootless,
                native_linux: cfg!(target_os = "linux"),
                docker_desktop: self.endpoint.is_desktop(),
                memory_limit: info.memory_limit.unwrap_or(false),
                cpu_quota: info.cpu_cfs_quota.unwrap_or(false)
                    && info.cpu_cfs_period.unwrap_or(false),
                pids_limit: info.pids_limit.unwrap_or(false),
                seccomp_enabled,
                docker_storage_accessible,
                disk_available_bytes,
                minimum_disk_bytes: self.minimum_disk_bytes,
            })
        })
    }

    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>> {
        Box::pin(async move {
            let images = self
                .docker
                .list_images(Some(ListImagesOptions {
                    all: true,
                    ..Default::default()
                }))
                .await
                .map_err(|error| map_api_error(&error))?;
            Ok(images
                .into_iter()
                .flat_map(|image| image.repo_digests)
                .collect())
        })
    }

    fn ensure_image<'a>(
        &'a self,
        image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            let result = self.docker.inspect_image(&image.reference).await;
            if matches!(
                &result,
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404,
                    ..
                })
            ) {
                let options = bollard::query_parameters::CreateImageOptionsBuilder::default()
                    .from_image(&image.reference)
                    .platform(&image.platform)
                    .build();
                let mut stream = self.docker.create_image(Some(options), None, None);
                while let Some(pulled) = stream.next().await {
                    pulled.map_err(|_| failure(PrerequisiteCode::ImageUnavailable, "Trusted executor image pull failed; check Docker registry access and retry", true))?;
                }
            }
            let inspect = match result {
                Ok(inspect) => inspect,
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404, ..
                }) => self
                    .docker
                    .inspect_image(&image.reference)
                    .await
                    .map_err(|_| {
                        failure(
                            PrerequisiteCode::ImageUnavailable,
                            "Pulled executor image could not be inspected",
                            true,
                        )
                    })?,
                Err(_) => {
                    return Err(failure(
                        PrerequisiteCode::DaemonStopped,
                        "Docker disconnected while preparing a trusted executor image",
                        true,
                    ));
                }
            };
            Ok(ImageObservation {
                repo_digests: inspect.repo_digests.unwrap_or_default(),
                os: inspect.os.unwrap_or_default(),
                architecture: inspect.architecture.unwrap_or_default(),
                configured_user: inspect
                    .config
                    .and_then(|config| config.user)
                    .unwrap_or_default(),
            })
        })
    }

    fn smoke_test<'a>(
        &'a self,
        image: &'a SelectedImage,
        container_name: &'a str,
    ) -> EngineFuture<'a, Result<(), PrerequisiteFailure>> {
        Box::pin(self.smoke_test_impl(image, container_name))
    }
}

#[cfg(test)]
#[path = "bollard_api/tests.rs"]
mod tests;
