use std::path::Path;

use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    images::ImageLock,
};

mod approved;
use super::{EngineDiscovery, RuntimeProfile};

impl EngineDiscovery {
    /// Run a bounded smoke container with no host workspace, network, or grants.
    pub async fn test_environment(&self, workspace: &Path) -> Result<(), PrerequisiteFailure> {
        let readiness = self.probe(workspace).await?;
        if !readiness.ready {
            return Err(super::failure(
                PrerequisiteCode::CapabilitiesIncompatible,
                "Docker host capabilities are not ready for execution",
                false,
            ));
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
}
