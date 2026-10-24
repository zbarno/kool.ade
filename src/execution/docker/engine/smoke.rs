use std::{path::Path, time::Duration};

use crate::app::{bootstrap_approval::ApprovedAction, bootstrap_repair_policy::Operation};
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    images::{ImageLock, RuntimeProfile, SelectedImage},
};

use super::{EngineDiscovery, EngineSnapshot};

impl EngineDiscovery {
    /// Run a bounded smoke container with no host workspace, network, or grants.
    pub async fn test_environment(&self, workspace: &Path) -> Result<(), PrerequisiteFailure> {
        let readiness = self.probe(workspace).await?;
        if !readiness.ready {
            return Err(readiness.failure.unwrap_or_else(|| {
                super::failure(
                    PrerequisiteCode::CapabilitiesIncompatible,
                    "Docker host capabilities are not ready for execution",
                    false,
                )
            }));
        }
        let architecture = readiness.architecture.as_deref().ok_or_else(|| {
            super::failure(
                PrerequisiteCode::UnsupportedArchitecture,
                "Docker did not report a supported Linux architecture",
                false,
            )
        })?;
        let image = ImageLock::bundled()?.select(RuntimeProfile::General, architecture)?;
        if !readiness.available_images.contains(&image.reference) {
            return Err(super::failure(
                PrerequisiteCode::ImageUnavailable,
                "The approved General image is not prepared; review its machine setup task first",
                true,
            ));
        }
        let name = format!("koolade-smoke-{}", uuid::Uuid::new_v4().simple());
        self.api.smoke_test(&image, &name, None).await
    }

    /// Pull and smoke-test the exact lock entry previously shown for consent.
    /// Reinspect the daemon on this same EngineDiscovery before each side
    /// effect so endpoint changes cannot redirect the approved operation.
    pub(crate) async fn test_approved_image(
        &self,
        workspace: &Path,
        authorization: &ApprovedAction,
    ) -> Result<(), PrerequisiteFailure> {
        let profile = match authorization.image_profile() {
            Some("general") => RuntimeProfile::General,
            _ => {
                return Err(super::failure(
                    PrerequisiteCode::ProfileUnsupported,
                    "Only the reviewed General image preparation flow is enabled",
                    false,
                ));
            }
        };
        if authorization.operation() != Operation::PrepareLockedImage {
            return Err(super::failure(
                PrerequisiteCode::ImageDigestMismatch,
                "Image preparation requires a one-use approved action",
                false,
            ));
        }
        let lock = ImageLock::bundled()?;
        let reference = format!("setup:images:{}:", authorization.image_profile().unwrap());
        let architecture = authorization
            .prerequisite_id()
            .strip_prefix(&reference)
            .filter(|value| matches!(*value, "amd64" | "arm64"))
            .ok_or_else(|| {
                super::failure(
                    PrerequisiteCode::UnsupportedArchitecture,
                    "The approved image prerequisite is not canonical",
                    false,
                )
            })?;
        let image = lock.select(profile, architecture)?;
        if authorization.image_reference() != image.reference
            || authorization.image_digest() != image.digest
            || authorization.image_platform() != image.platform
        {
            return Err(super::failure(
                PrerequisiteCode::ImageDigestMismatch,
                "The approved action does not match the bundled image lock",
                false,
            ));
        }
        let readiness = self.probe(workspace).await?;
        if !readiness.ready {
            return Err(readiness.failure.unwrap_or_else(|| {
                super::failure(
                    PrerequisiteCode::CapabilitiesIncompatible,
                    "Docker host capabilities are not ready for execution",
                    false,
                )
            }));
        }
        let architecture = readiness.architecture.as_deref().ok_or_else(|| {
            super::failure(
                PrerequisiteCode::UnsupportedArchitecture,
                "Docker did not report a supported Linux architecture",
                false,
            )
        })?;
        if image.platform != format!("linux/{architecture}") {
            return Err(super::failure(
                PrerequisiteCode::ImageArchitectureMismatch,
                "The approved image architecture no longer matches the Docker daemon",
                false,
            ));
        }
        let before_pull = self.inspect_snapshot().await?;
        if !matches_approval(self, authorization, &before_pull, &image)? {
            return Err(super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed before the approved image pull",
                true,
            ));
        }
        tokio::time::timeout(
            Duration::from_secs(120),
            self.prepare_image_with_fingerprint(&image, Some(authorization.engine_fingerprint())),
        )
        .await
        .map_err(|_| {
            super::failure(
                PrerequisiteCode::ImageUnavailable,
                "Approved image preparation timed out",
                true,
            )
        })??;
        let before_smoke = self.inspect_snapshot().await?;
        if !matches_approval(self, authorization, &before_smoke, &image)? {
            return Err(super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed before the isolation smoke test",
                true,
            ));
        }
        let name = format!("koolade-smoke-{}", uuid::Uuid::new_v4().simple());
        self.api
            .smoke_test(&image, &name, Some(authorization.engine_fingerprint()))
            .await?;
        let after_smoke = self.inspect_snapshot().await?;
        if !matches_approval(self, authorization, &after_smoke, &image)? {
            return Err(super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed during the isolation smoke test",
                true,
            ));
        }
        let available = self.api.image_digests().await?;
        if !available.contains(&image.reference) {
            return Err(super::failure(
                PrerequisiteCode::ImageUnavailable,
                "The exact approved image is no longer present after its isolation check",
                true,
            ));
        }
        let after_inventory = self.inspect_snapshot().await?;
        if !matches_approval(self, authorization, &after_inventory, &image)? {
            return Err(super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed during final image verification",
                true,
            ));
        }
        Ok(())
    }
}

fn matches_approval(
    engine: &EngineDiscovery,
    authorization: &ApprovedAction,
    snapshot: &EngineSnapshot,
    image: &SelectedImage,
) -> Result<bool, PrerequisiteFailure> {
    let fingerprint =
        super::approval_fingerprint(engine.endpoint(), snapshot, image).map_err(|_| {
            super::failure(
                PrerequisiteCode::ImageDigestMismatch,
                "The approved Engine fingerprint could not be verified",
                false,
            )
        })?;
    Ok(fingerprint == authorization.engine_fingerprint())
}
