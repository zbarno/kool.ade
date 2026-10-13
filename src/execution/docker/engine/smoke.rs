use std::{path::Path, time::Duration};

use crate::execution::docker::{
    discovery::{PrerequisiteCode, PrerequisiteFailure},
    images::{ImageLock, RuntimeProfile},
};

use super::EngineDiscovery;

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
        tokio::time::timeout(Duration::from_secs(120), self.prepare_image(&image))
            .await
            .map_err(|_| {
                super::failure(
                    PrerequisiteCode::ImageUnavailable,
                    "Approved image preparation timed out during the environment test",
                    true,
                )
            })??;
        let name = format!("koolade-smoke-{}", uuid::Uuid::new_v4().simple());
        self.api.smoke_test(&image, &name).await
    }
}
