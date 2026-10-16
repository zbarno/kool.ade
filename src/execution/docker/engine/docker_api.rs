use super::*;

pub(crate) trait DockerApi: Send + Sync {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>>;
    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>>;
    fn ensure_image<'a>(
        &'a self,
        image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>>;

    /// Inspect an already-built local image without attempting a registry pull.
    fn inspect_derived_image<'a>(
        &'a self,
        _image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async {
            Err(failure(
                PrerequisiteCode::ImageUnavailable,
                "Docker adapter cannot inspect a locally derived image",
                false,
            ))
        })
    }

    /// Query package state without networking.
    fn image_package_state<'a>(
        &'a self,
        _image_id: &'a str,
    ) -> EngineFuture<
        'a,
        Result<
            std::collections::BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>,
            PrerequisiteFailure,
        >,
    > {
        super::image_derivation::unsupported(
            "Docker API adapter does not support offline base package inspection",
        )
    }

    /// Build an app-generated context without networking.
    fn build_authorized_image<'a>(
        &'a self,
        _context: &'a crate::execution::docker::image_extension::builder::GeneratedImageContext,
        _platform: &'a str,
    ) -> EngineFuture<'a, Result<String, PrerequisiteFailure>> {
        super::image_derivation::unsupported(
            "Docker API adapter does not support authorized offline image derivation",
        )
    }

    /// Verify installed packages before publishing the image digest.
    fn verify_authorized_image<'a>(
        &'a self,
        _image_id: &'a str,
        _packages: &'a [crate::execution::docker::image_extension::builder::PackageArtifactIdentity],
    ) -> EngineFuture<'a, Result<(), PrerequisiteFailure>> {
        super::image_derivation::unsupported(
            "Docker API adapter does not support derived image manifest verification",
        )
    }

    fn smoke_test<'a>(
        &'a self,
        _image: &'a SelectedImage,
        _container_name: &'a str,
    ) -> EngineFuture<'a, Result<(), PrerequisiteFailure>> {
        Box::pin(async {
            Err(failure(
                PrerequisiteCode::CapabilitiesIncompatible,
                "Docker API adapter does not support the bounded smoke-container operation",
                false,
            ))
        })
    }
}
