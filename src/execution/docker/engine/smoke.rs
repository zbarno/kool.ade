use std::path::Path;

use crate::app::{bootstrap_approval::ApprovedAction, bootstrap_repair_policy::Operation};
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    images::{ImageLock, RuntimeProfile, SelectedImage, verify_image_observation},
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
        authorization: ApprovedAction,
        pull_control: &super::ImagePullControl,
    ) -> Result<crate::app::bootstrap_approval::ImagePreparationReceipt, PrerequisiteFailure> {
        let profile = match authorization.image_profile() {
            Some("general") => RuntimeProfile::General,
            Some("rust") => RuntimeProfile::Rust,
            Some("node") => RuntimeProfile::Node,
            Some("python") => RuntimeProfile::Python,
            Some("go") => RuntimeProfile::Go,
            _ => {
                return Err(super::failure(
                    PrerequisiteCode::ProfileUnsupported,
                    "Only approved General, Rust, Node, Python, and Go images can be prepared",
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
        let profile_name = authorization.image_profile().ok_or_else(|| {
            super::failure(
                PrerequisiteCode::ProfileUnsupported,
                "The approved action does not name an image profile",
                false,
            )
        })?;
        let mut task_parts = authorization.prerequisite_id().split(':');
        let canonical = task_parts.next() == Some("setup")
            && task_parts.next() == Some("images")
            && task_parts.next() == Some(profile_name);
        let architecture = task_parts.next().ok_or_else(|| {
            super::failure(
                PrerequisiteCode::ImageDigestMismatch,
                "The approved image task identity is incomplete",
                false,
            )
        })?;
        let requested_engine_identity = task_parts.next().ok_or_else(|| {
            super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "The approved image task is missing its trusted engine identity",
                false,
            )
        })?;
        if !canonical || task_parts.next().is_some() {
            return Err(super::failure(
                PrerequisiteCode::ImageDigestMismatch,
                "The approved image task identity is not canonical",
                false,
            ));
        }
        if !matches!(architecture, "amd64" | "arm64") {
            return Err(super::failure(
                PrerequisiteCode::UnsupportedArchitecture,
                "The approved image task architecture is unsupported",
                false,
            ));
        }
        if !is_lower_sha256(requested_engine_identity)
            || requested_engine_identity != authorization.engine_identity_fingerprint()
        {
            return Err(super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "The approved image task does not match its trusted engine identity",
                false,
            ));
        }
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
        if !matches_approval(self, &authorization, &before_pull, &image)? {
            return Err(super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed before the approved image pull",
                true,
            ));
        }
        if pull_control.is_cancelled() {
            return Err(pull_cancelled());
        }
        self.prepare_approved_image(&image, &authorization, pull_control)
            .await?;
        if pull_control.is_cancelled() {
            return Err(pull_cancelled());
        }
        let before_smoke = self.inspect_snapshot().await?;
        if !matches_approval(self, &authorization, &before_smoke, &image)? {
            return Err(super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed before the isolation smoke test",
                true,
            ));
        }
        if pull_control.is_cancelled() {
            return Err(pull_cancelled());
        }
        let name = format!("koolade-smoke-{}", uuid::Uuid::new_v4().simple());
        self.api
            .smoke_test(&image, &name, Some(authorization.engine_fingerprint()))
            .await?;
        if pull_control.is_cancelled() {
            return Err(pull_cancelled());
        }
        let after_smoke = self.inspect_snapshot().await?;
        if !matches_approval(self, &authorization, &after_smoke, &image)? {
            return Err(super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed during the isolation smoke test",
                true,
            ));
        }
        let available = self.api.image_digests().await?;
        verify_image_observation(&image, &available, "linux", architecture)?;
        self.verify_image_present(&image).await?;
        let after_inventory = self.inspect_snapshot().await?;
        if !matches_approval(self, &authorization, &after_inventory, &image)? {
            return Err(super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed during final image verification",
                true,
            ));
        }
        let final_engine_identity = crate::app::bootstrap_approval::engine_identity_fingerprint(
            &self.endpoint().fingerprint(),
            &after_inventory.daemon_id,
        )
        .ok_or_else(|| {
            super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "The final Docker daemon identity could not be verified",
                false,
            )
        })?;
        if final_engine_identity != requested_engine_identity
            || final_engine_identity != authorization.engine_identity_fingerprint()
        {
            return Err(super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker engine identity changed before image preparation completed",
                true,
            ));
        }
        if pull_control.is_cancelled() {
            return Err(pull_cancelled());
        }
        Ok(crate::app::bootstrap_approval::ImagePreparationReceipt {
            profile,
            architecture: architecture.to_owned(),
            image_digest: image.digest,
            engine_identity_fingerprint: final_engine_identity,
        })
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

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn pull_cancelled() -> PrerequisiteFailure {
    super::failure(
        PrerequisiteCode::ImageUnavailable,
        "Approved image preparation was cancelled",
        true,
    )
}
