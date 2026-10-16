use super::{EngineDiscovery, PrerequisiteCode, PrerequisiteFailure, failure, operation_timeout};

mod installed_state;

pub(super) fn unsupported<T: Send + 'static>(
    message: &'static str,
) -> super::EngineFuture<'static, Result<T, PrerequisiteFailure>> {
    Box::pin(async move {
        Err(failure(
            PrerequisiteCode::CapabilitiesIncompatible,
            message,
            false,
        ))
    })
}

impl EngineDiscovery {
    pub(crate) async fn inspect_base_packages(
        &self,
        image_id: &str,
    ) -> Result<
        std::collections::BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>,
        PrerequisiteFailure,
    > {
        let digest = image_id
            .strip_prefix("sha256:")
            .or_else(|| image_id.rsplit_once("@sha256:").map(|(_, digest)| digest));
        if !digest.is_some_and(|digest| {
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            return Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Base image identifier is invalid",
                false,
            ));
        }
        tokio::time::timeout(
            self.timeouts.image_preparation,
            self.api.image_package_state(image_id),
        )
        .await
        .map_err(|_| operation_timeout("base image package-state inspection"))?
    }

    /// Build only from an app-authorized request and broker-verified package
    /// bytes. Worker commands cannot access this control-plane capability.
    pub(crate) async fn derive_authorized_image(
        &self,
        request: &crate::execution::docker::image_extension::AuthorizedImageExtensionRequest,
        packages: Vec<crate::execution::docker::image_extension::builder::VerifiedPackageArtifact>,
    ) -> Result<crate::execution::docker::image_extension::DerivedImage, PrerequisiteFailure> {
        let context =
            crate::execution::docker::image_extension::builder::generate_context(request, packages)
                .map_err(|_| {
                    failure(
                        PrerequisiteCode::ImageUnavailable,
                        "Verified system package snapshot could not produce a safe image context",
                        false,
                    )
                })?;
        let image_id = tokio::time::timeout(
            self.timeouts.image_preparation,
            self.api
                .build_authorized_image(&context, &request.target_platform),
        )
        .await
        .map_err(|_| operation_timeout("authorized system image derivation"))??;
        tokio::time::timeout(
            self.timeouts.image_preparation,
            self.api
                .verify_authorized_image(&image_id, &context.package_manifest),
        )
        .await
        .map_err(|_| operation_timeout("derived image package manifest verification"))??;
        Ok(crate::execution::docker::image_extension::DerivedImage {
            image_id,
            content_identity: context.content_identity,
            base_digest: request.base_digest.clone(),
            target_platform: request.target_platform.clone(),
            package_manifest: context.package_manifest,
        })
    }
}

pub(super) fn image_package_state<'a>(
    api: &'a super::bollard_api::BollardApi,
    image_id: &'a str,
) -> super::EngineFuture<
    'a,
    Result<
        std::collections::BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>,
        PrerequisiteFailure,
    >,
> {
    installed_state::inspect(api, image_id)
}
