use super::{BollardApi, failure, map_api_error};
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    images::{ImageObservation, SelectedImage},
};

impl BollardApi {
    pub(super) async fn inspect_image_without_pull(
        &self,
        image: &SelectedImage,
    ) -> Result<ImageObservation, PrerequisiteFailure> {
        let inspect = self
            .docker
            .inspect_image(&image.reference)
            .await
            .map_err(|error| {
                if matches!(
                    &error,
                    bollard::errors::Error::DockerResponseServerError {
                        status_code: 404,
                        ..
                    }
                ) {
                    failure(
                        PrerequisiteCode::ImageUnavailable,
                        "The approved locked image is not installed; review its setup task before running this workload",
                        false,
                    )
                } else {
                    map_api_error(&error)
                }
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
    }
}
