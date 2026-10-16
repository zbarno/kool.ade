use super::{DockerExecutionBackend, SelectedImage};
use crate::execution::errors::{ExecutionError, ExecutionErrorKind};

#[derive(Clone)]
pub(super) struct RegisteredDerivedImage {
    pub image: SelectedImage,
    pub content_identity: String,
    pub base_digest: String,
}

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
            .insert(
                derived.image_id.clone(),
                RegisteredDerivedImage {
                    image: selected,
                    content_identity: derived.content_identity.clone(),
                    base_digest: derived.base_digest.clone(),
                },
            );
        Ok(derived)
    }

    pub(super) async fn prepare_selected_image(
        &self,
        image: &SelectedImage,
    ) -> Result<Option<super::PreparedDerivedImage>, ExecutionError> {
        let registered = self
            .derived_images
            .lock()
            .map_err(|_| super::internal_error_with_context("reading derived image registry"))?
            .get(&image.digest)
            .cloned();
        match registered {
            Some(registered) if registered.image == *image => {
                self.engine
                    .prepare_derived_image(
                        image,
                        &registered.content_identity,
                        &registered.base_digest,
                    )
                    .await
                    .map_err(super::prerequisite_error)?;
                Ok(Some(super::PreparedDerivedImage {
                    image: registered.image,
                    content_identity: registered.content_identity,
                    base_digest: registered.base_digest,
                }))
            }
            Some(_) => Err(ExecutionError::new(
                ExecutionErrorKind::ImageUnavailable,
                "Selected derived image differs from its registered identity",
                false,
            )),
            None => {
                self.engine
                    .prepare_image(image)
                    .await
                    .map_err(super::prerequisite_error)?;
                Ok(None)
            }
        }
    }
}
