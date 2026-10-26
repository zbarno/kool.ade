use super::{
    EngineDiscovery, ImageLock, PrerequisiteCode, PrerequisiteFailure, SelectedImage, failure,
    operation_timeout,
};

impl EngineDiscovery {
    /// Verify an approved locked image that is already present, without pulling it.
    pub(crate) async fn verify_image_present(
        &self,
        image: &SelectedImage,
    ) -> Result<(), PrerequisiteFailure> {
        let lock = ImageLock::bundled()?;
        let architecture = image.platform.strip_prefix("linux/").unwrap_or_default();
        let expected = lock.select(image.profile, architecture)?;
        if expected != *image {
            return Err(failure(
                PrerequisiteCode::ImageDigestMismatch,
                "Requested image does not match the bundled approved digest lock",
                false,
            ));
        }
        let observation =
            tokio::time::timeout(self.timeouts.operation, self.api.inspect_image(image))
                .await
                .map_err(|_| operation_timeout("approved local image inspection"))??;
        crate::execution::docker::images::verify_image_observation(
            image,
            &observation.repo_digests,
            &observation.os,
            &observation.architecture,
        )
    }
}

#[cfg(test)]
#[path = "tests/image_inspection.rs"]
mod tests;
