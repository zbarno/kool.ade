use std::path::Path;

use crate::app::{bootstrap_approval::ApprovedAction, bootstrap_repair_policy::Operation};
use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    images::{ImageLock, RuntimeProfile, verify_image_observation},
    prerequisites::inspect_workspace_capacity,
};

use super::super::{DEFAULT_MINIMUM_FREE_BYTES, EngineDiscovery};
mod verification;
use verification::{is_lower_sha256, matches_approval, pull_cancelled};

impl EngineDiscovery {
    /// Pull and smoke-test the exact lock entry previously shown for consent.
    /// Reinspect the daemon on this same EngineDiscovery before each side
    /// effect so endpoint changes cannot redirect the approved operation.
    pub(crate) async fn test_approved_image(
        &self,
        workspace: &Path,
        authorization: ApprovedAction,
        pull_control: &super::super::ImagePullControl,
    ) -> Result<crate::app::bootstrap_approval::ImagePreparationReceipt, PrerequisiteFailure> {
        let profile = match authorization.image_profile() {
            Some("general") => RuntimeProfile::General,
            Some("rust") => RuntimeProfile::Rust,
            Some("node") => RuntimeProfile::Node,
            Some("python") => RuntimeProfile::Python,
            Some("go") => RuntimeProfile::Go,
            _ => {
                return Err(super::super::failure(
                    PrerequisiteCode::ProfileUnsupported,
                    "Only approved General, Rust, Node, Python, and Go images can be prepared",
                    false,
                ));
            }
        };
        if authorization.operation() != Operation::PrepareLockedImage {
            return Err(super::super::failure(
                PrerequisiteCode::ImageDigestMismatch,
                "Image preparation requires a one-use approved action",
                false,
            ));
        }
        let capacity_probe_required = authorization.capacity_probe_required();
        if capacity_probe_required
            && (profile != RuntimeProfile::General
                || !self.endpoint().is_desktop()
                || !self.desktop_transport_candidate)
        {
            return Err(super::super::failure(
                PrerequisiteCode::InsufficientDisk,
                "Only a verified Docker Desktop General image may use the bounded capacity bootstrap",
                false,
            ));
        }
        let lock = ImageLock::bundled()?;
        let profile_name = authorization.image_profile().ok_or_else(|| {
            super::super::failure(
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
            super::super::failure(
                PrerequisiteCode::ImageDigestMismatch,
                "The approved image task identity is incomplete",
                false,
            )
        })?;
        let requested_engine_identity = task_parts.next().ok_or_else(|| {
            super::super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "The approved image task is missing its trusted engine identity",
                false,
            )
        })?;
        if !canonical || task_parts.next().is_some() {
            return Err(super::super::failure(
                PrerequisiteCode::ImageDigestMismatch,
                "The approved image task identity is not canonical",
                false,
            ));
        }
        if !matches!(architecture, "amd64" | "arm64") {
            return Err(super::super::failure(
                PrerequisiteCode::UnsupportedArchitecture,
                "The approved image task architecture is unsupported",
                false,
            ));
        }
        if !is_lower_sha256(requested_engine_identity)
            || requested_engine_identity != authorization.engine_identity_fingerprint()
        {
            return Err(super::super::failure(
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
            return Err(super::super::failure(
                PrerequisiteCode::ImageDigestMismatch,
                "The approved action does not match the bundled image lock",
                false,
            ));
        }
        let readiness = self.probe(workspace).await?;
        let bootstrap_workspace_ready = if capacity_probe_required {
            let capacity = inspect_workspace_capacity(workspace);
            capacity.accessible
                && capacity
                    .available_bytes
                    .is_some_and(|bytes| bytes >= DEFAULT_MINIMUM_FREE_BYTES)
        } else {
            false
        };
        let capacity_bootstrap_is_only_blocker = capacity_probe_required
            && profile == RuntimeProfile::General
            && readiness.desktop_transport_verified
            && readiness.workspace_staging_available
            && bootstrap_workspace_ready
            && readiness.disk_available_bytes.is_none()
            && readiness
                .failure
                .as_ref()
                .is_some_and(|failure| failure.code == PrerequisiteCode::InsufficientDisk)
            && !readiness.available_images.contains(&image.reference);
        if !readiness.ready && !capacity_bootstrap_is_only_blocker {
            return Err(readiness.failure.unwrap_or_else(|| {
                super::super::failure(
                    PrerequisiteCode::CapabilitiesIncompatible,
                    "Docker host capabilities are not ready for execution",
                    false,
                )
            }));
        }
        if readiness.ready && capacity_probe_required {
            return Err(super::super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker Desktop capacity changed after approval; review the General image action again",
                true,
            ));
        }
        let architecture = readiness.architecture.as_deref().ok_or_else(|| {
            super::super::failure(
                PrerequisiteCode::UnsupportedArchitecture,
                "Docker did not report a supported Linux architecture",
                false,
            )
        })?;
        if image.platform != format!("linux/{architecture}") {
            return Err(super::super::failure(
                PrerequisiteCode::ImageArchitectureMismatch,
                "The approved image architecture no longer matches the Docker daemon",
                false,
            ));
        }
        let before_pull = self.inspect_snapshot_for_image_preparation(profile).await?;
        if capacity_probe_required
            && (before_pull.docker_storage_accessible || before_pull.disk_available_bytes.is_some())
        {
            return Err(super::super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker Desktop capacity changed after approval; review the General image action again",
                true,
            ));
        }
        if !matches_approval(self, &authorization, &before_pull, &image)? {
            return Err(super::super::failure(
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
        let after_pull = self.inspect_snapshot_for_image_preparation(profile).await?;
        if !matches_approval(self, &authorization, &after_pull, &image)? {
            return Err(super::super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed before VM capacity verification",
                true,
            ));
        }
        if !after_pull.docker_storage_accessible
            || after_pull
                .disk_available_bytes
                .is_none_or(|bytes| bytes < after_pull.minimum_disk_bytes)
        {
            return Err(super::super::failure(
                PrerequisiteCode::InsufficientDisk,
                "Docker Desktop VM storage does not retain the required free space after image preparation",
                true,
            ));
        }
        let before_smoke = self.inspect_snapshot_engine().await?;
        if !matches_approval(self, &authorization, &before_smoke, &image)? {
            return Err(super::super::failure(
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
        let after_smoke = self.inspect_snapshot_engine().await?;
        if !matches_approval(self, &authorization, &after_smoke, &image)? {
            return Err(super::super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed during the isolation smoke test",
                true,
            ));
        }
        let available = self.api.image_digests().await?;
        verify_image_observation(&image, &available, "linux", architecture)?;
        self.verify_image_present(&image).await?;
        let after_inventory = self.inspect_snapshot_engine().await?;
        if !matches_approval(self, &authorization, &after_inventory, &image)? {
            return Err(super::super::failure(
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
            super::super::failure(
                PrerequisiteCode::UntrustedEndpoint,
                "The final Docker daemon identity could not be verified",
                false,
            )
        })?;
        if final_engine_identity != requested_engine_identity
            || final_engine_identity != authorization.engine_identity_fingerprint()
        {
            return Err(super::super::failure(
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
