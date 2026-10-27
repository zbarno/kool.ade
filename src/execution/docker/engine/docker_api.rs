use super::*;

pub(crate) trait DockerApi: Send + Sync {
    fn inspect<'a>(&'a self) -> EngineFuture<'a, Result<EngineSnapshot, PrerequisiteFailure>>;
    fn image_digests<'a>(&'a self) -> EngineFuture<'a, Result<Vec<String>, PrerequisiteFailure>>;

    /// Inspect a locally available image without attempting a registry pull.
    fn inspect_image<'a>(
        &'a self,
        _image: &'a SelectedImage,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async {
            Err(failure(
                PrerequisiteCode::CapabilitiesIncompatible,
                "Docker adapter cannot inspect images without pulling",
                false,
            ))
        })
    }

    /// Measure free bytes in a verified Docker Desktop Linux VM using an
    /// already-installed, exact-lock General image. The probe has no host
    /// mounts or network access and adapters must clean up only its container.
    fn desktop_storage_capacity<'a>(
        &'a self,
        _image: &'a SelectedImage,
        _expected_engine_fingerprint: &'a str,
    ) -> EngineFuture<'a, Result<u64, PrerequisiteFailure>> {
        Box::pin(async {
            Err(failure(
                PrerequisiteCode::CapabilitiesIncompatible,
                "Docker adapter cannot measure Desktop VM storage safely",
                false,
            ))
        })
    }

    #[cfg(test)]
    fn ensure_image<'a>(
        &'a self,
        image: &'a SelectedImage,
        expected_engine_fingerprint: Option<&'a str>,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>>;

    /// Pull an approved image with cancellation and bounded progress control.
    /// `verified_capacity_bytes` is fresh VM/engine evidence from the same
    /// fingerprinted engine; `None` may authorize only the size-capped first
    /// Desktop General image. Adapters that cannot uphold cancellation must
    /// fail closed.
    fn ensure_image_with_control<'a>(
        &'a self,
        _image: &'a SelectedImage,
        _expected_engine_fingerprint: &'a str,
        _verified_capacity_bytes: Option<u64>,
        _control: &'a super::ImagePullControl,
    ) -> EngineFuture<'a, Result<ImageObservation, PrerequisiteFailure>> {
        Box::pin(async {
            Err(failure(
                PrerequisiteCode::CapabilitiesIncompatible,
                "Docker adapter does not support cancellable approved image pulls",
                false,
            ))
        })
    }

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
        _expected_engine_fingerprint: Option<&'a str>,
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
