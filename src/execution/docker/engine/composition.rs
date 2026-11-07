//! Content-addressed, verified composite construction from locked sources.
//!
//! The Docker daemon's tags are persistent cache hints, never authorization.
//! Every use must revalidate the recipe, exact source images, new image ID,
//! architecture, app-owned labels and real tool binary versions.

use super::{EngineDiscovery, PrerequisiteCode, PrerequisiteFailure, failure, operation_timeout};
use crate::execution::docker::images::{
    CompositionCandidate, ImageLock, SelectedImage, verified_composition_context,
};
use tokio::sync::Mutex;

// One trusted builder at a time per app instance. The cache's deterministic
// digest and Docker label checks also reject a mismatched concurrent writer.
static COMPOSITE_BUILD: Mutex<()> = Mutex::const_new(());

impl EngineDiscovery {
    pub(crate) async fn compose_verified_runtime(
        &self,
        plan: &CompositionCandidate,
    ) -> Result<SelectedImage, PrerequisiteFailure> {
        let _guard = COMPOSITE_BUILD.lock().await;
        // Recheck the canonical plan and platform before any Docker operation.
        let (context, mut selected) = verified_composition_context(plan).map_err(|_| {
            failure(
                PrerequisiteCode::ProfileUnsupported,
                "Project runtime composition sources no longer satisfy the immutable lock",
                false,
            )
        })?;
        let lock = ImageLock::bundled()?;
        let architecture = plan.platform.strip_prefix("linux/").ok_or_else(|| {
            failure(
                PrerequisiteCode::UnsupportedArchitecture,
                "Project runtime composition requires a supported Linux platform",
                false,
            )
        })?;
        let snapshot = self.inspect_snapshot().await?;
        if snapshot.architecture != architecture {
            return Err(failure(
                PrerequisiteCode::ImageArchitectureMismatch,
                "Docker daemon architecture changed before project runtime composition",
                false,
            ));
        }
        for source in std::iter::once(&plan.foundation).chain(plan.components.iter()) {
            if lock.select(source.profile, architecture)? != *source {
                return Err(failure(
                    PrerequisiteCode::ImageDigestMismatch,
                    "Composite source changed after the approved recipe was resolved",
                    false,
                ));
            }
            self.verify_image_present(source).await?;
        }
        // This backend accepts only its own generated context. BuildKit has
        // networkmode=none and no secret/session providers or project context.
        let image_id = tokio::time::timeout(
            self.timeouts.image_preparation,
            self.api.build_authorized_image(&context, &plan.platform),
        )
        .await
        .map_err(|_| operation_timeout("trusted project runtime image composition"))??;
        if !crate::execution::docker::images::is_digest(&image_id) {
            return Err(failure(
                PrerequisiteCode::ImageDigestMismatch,
                "Docker returned an invalid composite image ID",
                false,
            ));
        }
        selected.reference = image_id.clone();
        selected.digest = image_id;
        self.prepare_derived_image(
            &selected,
            &context.content_identity,
            &context.base_digest,
        )
        .await?;
        tokio::time::timeout(
            self.timeouts.image_preparation,
            self.api.verify_composite_image(&selected),
        )
        .await
        .map_err(|_| operation_timeout("offline composite toolchain smoke verification"))??;
        Ok(selected)
    }
}
