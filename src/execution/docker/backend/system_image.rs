use super::{DockerExecutionBackend, SelectedImage};
use crate::execution::errors::{ExecutionError, ExecutionErrorKind};

fn disconnected() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        "Docker system image operation is unavailable",
        true,
    )
}

impl DockerExecutionBackend {
    pub(super) async fn inspect_system_packages(
        &self,
        base: &crate::execution::docker::image_extension::BaseImageSelection,
    ) -> Result<
        std::collections::BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>,
        crate::execution::errors::ExecutionError,
    > {
        let architecture = base
            .platform
            .strip_prefix("linux/")
            .ok_or_else(disconnected)?;
        let locked = crate::execution::docker::images::ImageLock::bundled()
            .map_err(|_| disconnected())?
            .select(base.profile, architecture)
            .map_err(|_| disconnected())?;
        if locked.digest != base.digest || locked.reference != base.reference {
            return Err(disconnected());
        }
        self.engine
            .inspect_base_packages(&locked.reference)
            .await
            .map_err(super::prerequisite_error)
    }

    pub(super) async fn derive_system_image(
        &self,
        request: &crate::execution::docker::image_extension::AuthorizedImageExtensionRequest,
        packages: Vec<crate::execution::docker::image_extension::builder::VerifiedPackageArtifact>,
    ) -> Result<
        crate::execution::docker::image_extension::DerivedImage,
        crate::execution::errors::ExecutionError,
    > {
        let derived = self
            .engine
            .derive_authorized_image(request, packages)
            .await
            .map_err(super::prerequisite_error)?;
        let architecture = request
            .target_platform
            .strip_prefix("linux/")
            .ok_or_else(disconnected)?;
        let base = crate::execution::docker::images::ImageLock::bundled()
            .map_err(|_| disconnected())?
            .select(request.base_profile, architecture)
            .map_err(|_| disconnected())?;
        let selected = SelectedImage {
            profile: base.profile,
            platform: request.target_platform.clone(),
            reference: derived.image_id.clone(),
            digest: derived.image_id.clone(),
            runtime_user: base.runtime_user,
            tool_versions: base.tool_versions,
        };
        self.derived_images
            .lock()
            .map_err(|_| disconnected())?
            .insert(derived.image_id.clone(), selected);
        Ok(derived)
    }
}
