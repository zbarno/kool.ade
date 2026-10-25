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
    pub(super) docker: Docker,
    endpoint: super::DockerEndpoint,
    minimum_disk_bytes: u64,
}

mod image_derivation;
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

    pub(super) async fn require_approved_engine(
        &self,
        image: &SelectedImage,
        expected_fingerprint: &str,
    ) -> Result<(), PrerequisiteFailure> {
        let snapshot = DockerApi::inspect(self).await?;
        let actual =
            super::approval_fingerprint(&self.endpoint, &snapshot, image).map_err(|_| {
                failure(
                    PrerequisiteCode::UntrustedEndpoint,
                    "Could not verify the approved Docker daemon identity",
                    false,
                )
            })?;
        if actual != expected_fingerprint {
            return Err(failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed before the approved operation",
                true,
            ));
        }
        Ok(())
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
                cgroup_version: info.cgroup_version.map(|version| version.to_string()),
                cgroup_driver: info.cgroup_driver.map(|driver| driver.to_string()),
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
        expected_engine_fingerprint: Option<&'a str>,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            if let Some(expected) = expected_engine_fingerprint {
                self.require_approved_engine(image, expected).await?;
            }
            let result = self.docker.inspect_image(&image.reference).await;
            if matches!(
                &result,
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404,
                    ..
                })
            ) {
                if let Some(expected) = expected_engine_fingerprint {
                    self.require_approved_engine(image, expected).await?;
                }
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
                image_id: inspect.id.unwrap_or_default(),
                repo_digests: inspect.repo_digests.unwrap_or_default(),
                os: inspect.os.unwrap_or_default(),
                architecture: inspect.architecture.unwrap_or_default(),
                configured_user: inspect
                    .config
                    .as_ref()
                    .and_then(|config| config.user.clone())
                    .unwrap_or_default(),
                labels: inspect
                    .config
                    .and_then(|config| config.labels)
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
            })
        })
    }

    fn inspect_derived_image<'a>(
        &'a self,
        image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async move {
            let inspect = self
                .docker
                .inspect_image(&image.reference)
                .await
                .map_err(|_| {
                    failure(
                        PrerequisiteCode::ImageUnavailable,
                        "Registered derived image is not present in the selected Docker daemon",
                        false,
                    )
                })?;
            Ok(ImageObservation {
                image_id: inspect.id.unwrap_or_default(),
                repo_digests: inspect.repo_digests.unwrap_or_default(),
                os: inspect.os.unwrap_or_default(),
                architecture: inspect.architecture.unwrap_or_default(),
                configured_user: inspect
                    .config
                    .as_ref()
                    .and_then(|config| config.user.clone())
                    .unwrap_or_default(),
                labels: inspect
                    .config
                    .and_then(|config| config.labels)
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
            })
        })
    }

    fn image_package_state<'a>(
        &'a self,
        image_id: &'a str,
    ) -> EngineFuture<
        'a,
        Result<
            std::collections::BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>,
            PrerequisiteFailure,
        >,
    > {
        image_derivation::image_package_state(self, image_id)
    }

    fn build_authorized_image<'a>(
        &'a self,
        context: &'a crate::execution::docker::image_extension::builder::GeneratedImageContext,
        platform: &'a str,
    ) -> EngineFuture<'a, Result<String, PrerequisiteFailure>> {
        image_derivation::build(self, context, platform)
    }

    fn verify_authorized_image<'a>(
        &'a self,
        image_id: &'a str,
        packages: &'a [crate::execution::docker::image_extension::builder::PackageArtifactIdentity],
    ) -> EngineFuture<'a, Result<(), PrerequisiteFailure>> {
        image_derivation::verify(self, image_id, packages)
    }

    fn smoke_test<'a>(
        &'a self,
        image: &'a SelectedImage,
        container_name: &'a str,
        expected_engine_fingerprint: Option<&'a str>,
    ) -> EngineFuture<'a, Result<(), PrerequisiteFailure>> {
        Box::pin(self.smoke_test_impl(image, container_name, expected_engine_fingerprint))
    }
}

#[cfg(test)]
#[path = "bollard_api/tests.rs"]
mod tests;
