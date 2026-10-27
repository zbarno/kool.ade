use super::{EngineDiscovery, ImagePullControl, approval_fingerprint, failure, operation_timeout};
use crate::execution::docker::images::{ImageLock, RuntimeProfile, SelectedImage};
use crate::{
    app::{bootstrap_approval::ApprovedAction, bootstrap_repair_policy::Operation},
    execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure},
};

mod capacity;

impl EngineDiscovery {
    /// Pull only after rechecking the consent-bound daemon identity. The
    /// caller supplies a one-operation control shared with the UI worker.
    pub(super) async fn prepare_approved_image(
        &self,
        image: &SelectedImage,
        authorization: &ApprovedAction,
        control: &ImagePullControl,
    ) -> Result<(), PrerequisiteFailure> {
        verify_authorized_image(image, authorization)?;
        if control.is_cancelled() {
            return Err(cancelled());
        }
        let snapshot = self
            .inspect_snapshot_for_image_preparation(image.profile)
            .await?;
        let actual_identity = crate::app::bootstrap_approval::engine_identity_fingerprint(
            &self.endpoint().fingerprint(),
            &snapshot.daemon_id,
        )
        .ok_or_else(|| {
            failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Could not verify the approved Docker daemon identity",
                false,
            )
        })?;
        if actual_identity != authorization.engine_identity_fingerprint() {
            return Err(failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed before the approved image pull",
                true,
            ));
        }
        let actual = approval_fingerprint(self.endpoint(), &snapshot, image).map_err(|_| {
            failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Could not verify the approved Docker daemon binding",
                false,
            )
        })?;
        if actual != authorization.engine_fingerprint() {
            return Err(failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Docker daemon identity changed before the approved image pull",
                true,
            ));
        }
        if authorization.capacity_probe_required() {
            if image.profile != RuntimeProfile::General
                || !self.desktop_transport_candidate
                || !self.endpoint().is_desktop()
                || !snapshot.docker_desktop
                || snapshot.docker_storage_accessible
                || snapshot.disk_available_bytes.is_some()
            {
                return Err(failure(
                    PrerequisiteCode::UntrustedEndpoint,
                    "The approved Desktop capacity bootstrap no longer matches the verified engine state",
                    true,
                ));
            }
        } else {
            verify_image_storage_capacity(image, &snapshot)?;
        }
        if control.is_cancelled() {
            return Err(cancelled());
        }
        let observed = tokio::time::timeout(
            self.timeouts.image_preparation,
            self.api.ensure_image_with_control(
                image,
                authorization.engine_fingerprint(),
                snapshot
                    .disk_available_bytes
                    .filter(|_| snapshot.docker_storage_accessible),
                control,
            ),
        )
        .await
        .map_err(|_| operation_timeout("approved runtime image preparation"))??;
        if control.is_cancelled() {
            return Err(cancelled());
        }
        verify_observation(image, &observed)
    }
}

fn verify_image_storage_capacity(
    image: &SelectedImage,
    snapshot: &super::EngineSnapshot,
) -> Result<(), PrerequisiteFailure> {
    let estimate = ImageLock::bundled()?.size_estimate(image.profile, &snapshot.architecture)?;
    let available = match snapshot
        .disk_available_bytes
        .filter(|_| snapshot.docker_storage_accessible)
    {
        Some(available) => available,
        None => {
            return Err(failure(
                PrerequisiteCode::InsufficientDisk,
                "Docker image storage capacity could not be verified before the approved pull",
                false,
            ));
        }
    };
    let required = snapshot
        .minimum_disk_bytes
        .checked_add(estimate.estimated_storage_bytes)
        .ok_or_else(|| {
            failure(
                PrerequisiteCode::InsufficientDisk,
                "Approved image storage requirement is outside the supported size bound",
                false,
            )
        })?;
    if available < required {
        return Err(failure(
            PrerequisiteCode::InsufficientDisk,
            "Docker image storage does not have enough free space for the approved runtime image",
            true,
        ));
    }
    Ok(())
}

fn verify_authorized_image(
    image: &SelectedImage,
    authorization: &ApprovedAction,
) -> Result<(), PrerequisiteFailure> {
    if authorization.operation() != Operation::PrepareLockedImage {
        return Err(failure(
            PrerequisiteCode::ImageDigestMismatch,
            "Image preparation requires a one-use approved action",
            false,
        ));
    }
    let profile_name = authorization.image_profile().ok_or_else(|| {
        failure(
            PrerequisiteCode::ProfileUnsupported,
            "The approved action does not name an image profile",
            false,
        )
    })?;
    let profile = match profile_name {
        "general" => crate::execution::docker::images::RuntimeProfile::General,
        "rust" => crate::execution::docker::images::RuntimeProfile::Rust,
        "node" => crate::execution::docker::images::RuntimeProfile::Node,
        "python" => crate::execution::docker::images::RuntimeProfile::Python,
        "go" => crate::execution::docker::images::RuntimeProfile::Go,
        _ => {
            return Err(failure(
                PrerequisiteCode::ProfileUnsupported,
                "Only approved General, Rust, Node, Python, and Go images can be prepared",
                false,
            ));
        }
    };
    let lock = ImageLock::bundled()?;
    let architecture = image.platform.strip_prefix("linux/").unwrap_or_default();
    let expected = lock.select(profile, architecture)?;
    let task = crate::app::bootstrap_repair_policy::parse_locked_image_task_id(
        authorization.prerequisite_id(),
    );
    let valid_task = task.is_some_and(|(task_profile, task_architecture, engine_identity)| {
        task_profile == profile_name
            && task_architecture == architecture
            && engine_identity == authorization.engine_identity_fingerprint()
    });
    if !valid_task
        || expected != *image
        || image.profile != profile
        || authorization.image_reference() != image.reference
        || authorization.image_digest() != image.digest
        || authorization.image_platform() != image.platform
    {
        return Err(failure(
            PrerequisiteCode::ImageDigestMismatch,
            "Requested image does not match the one-use approved action and bundled digest lock",
            false,
        ));
    }
    Ok(())
}

fn verify_observation(
    image: &SelectedImage,
    observed: &crate::execution::docker::images::ImageObservation,
) -> Result<(), PrerequisiteFailure> {
    crate::execution::docker::images::verify_image_observation(
        image,
        &observed.repo_digests,
        &observed.os,
        &observed.architecture,
    )
}

fn cancelled() -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::ImageUnavailable,
        "Approved image preparation was cancelled",
        true,
    )
}

#[cfg(test)]
mod tests;
