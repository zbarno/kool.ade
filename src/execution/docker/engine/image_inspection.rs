use super::{
    EngineDiscovery, ImageLock, PrerequisiteCode, PrerequisiteFailure, SelectedImage, failure,
    probe::inspect_image_with_retry,
};

impl EngineDiscovery {
    /// Verify an approved locked image that is already present, without pulling it.
    pub(crate) async fn verify_image_present(
        &self,
        image: &SelectedImage,
    ) -> Result<(), PrerequisiteFailure> {
        let lock = ImageLock::bundled()?;
        let architecture = image.platform.strip_prefix("linux/").unwrap_or_default();
        let expected = lock.select(image.profile, architecture).or_else(|_| {
            lock.select_experimental_qa(
                image.profile,
                architecture,
                &crate::persistence::experimental_features::load().0,
            )
        })?;
        if expected != *image {
            return Err(failure(
                PrerequisiteCode::ImageDigestMismatch,
                "Requested image does not match the bundled approved digest lock",
                false,
            ));
        }
        let observation =
            inspect_image_with_retry(self.api.as_ref(), image, self.timeouts.operation).await?;
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
