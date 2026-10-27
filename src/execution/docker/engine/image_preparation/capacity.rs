use super::super::{
    EngineDiscovery, EngineSnapshot, approval_fingerprint, failure, operation_timeout,
};
use crate::execution::docker::{
    discovery::{
        PrerequisiteCode, PrerequisiteFailure, validate_snapshot, validate_snapshot_engine,
    },
    images::{ImageLock, RuntimeProfile},
};

const DESKTOP_CAPACITY_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

impl EngineDiscovery {
    /// Revalidate the engine before image consent. Only a Desktop General image
    /// proposal may bind while VM capacity is unknown; the bounded pull and
    /// isolated capacity sample run after the user grants its one-use action.
    pub(crate) async fn inspect_snapshot_for_image_preparation(
        &self,
        profile: RuntimeProfile,
    ) -> Result<EngineSnapshot, PrerequisiteFailure> {
        let mut snapshot = tokio::time::timeout(self.timeouts.operation, self.api.inspect())
            .await
            .map_err(|_| operation_timeout("Docker Engine identity inspection"))??;
        validate_snapshot_engine(&snapshot)?;
        if is_unmeasured_desktop_storage(
            self.desktop_transport_candidate,
            self.endpoint.is_desktop(),
            &snapshot,
        ) {
            let image_digests =
                tokio::time::timeout(self.timeouts.operation, self.api.image_digests())
                    .await
                    .map_err(|_| operation_timeout("Docker image inventory"))??;
            self.measure_desktop_capacity_if_available(&mut snapshot, &image_digests)
                .await?;
        }
        if profile == RuntimeProfile::General
            && is_unmeasured_desktop_storage(
                self.desktop_transport_candidate,
                self.endpoint.is_desktop(),
                &snapshot,
            )
        {
            return Ok(snapshot);
        }
        validate_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    /// Inspect the Docker engine without requiring storage telemetry. This is
    /// used only for post-side-effect identity fences after capacity has
    /// already been measured in the approved preparation flow.
    pub(crate) async fn inspect_snapshot_engine(
        &self,
    ) -> Result<EngineSnapshot, PrerequisiteFailure> {
        let snapshot = tokio::time::timeout(self.timeouts.operation, self.api.inspect())
            .await
            .map_err(|_| operation_timeout("Docker Engine identity inspection"))??;
        validate_snapshot_engine(&snapshot)?;
        Ok(snapshot)
    }

    /// Measure guest VM capacity only from the exact, already-approved General
    /// image. The result is fresh for this verified endpoint and daemon sample.
    pub(crate) async fn measure_desktop_storage_capacity(
        &self,
        snapshot: &mut EngineSnapshot,
        image_digests: &[String],
    ) -> Result<(), PrerequisiteFailure> {
        if !self.desktop_transport_candidate
            || !self.endpoint.is_desktop()
            || !snapshot.docker_desktop
        {
            return Err(failure(
                PrerequisiteCode::UntrustedEndpoint,
                "VM capacity probes are restricted to a verified local Docker Desktop endpoint",
                false,
            ));
        }
        validate_snapshot_engine(snapshot)?;
        let image =
            ImageLock::bundled()?.select(RuntimeProfile::General, &snapshot.architecture)?;
        if !image_digests.contains(&image.reference) {
            return Err(failure(
                PrerequisiteCode::InsufficientDisk,
                "Docker Desktop VM capacity cannot be measured until the bounded locked General image is prepared",
                true,
            ));
        }
        let expected = approval_fingerprint(self.endpoint(), snapshot, &image).map_err(|_| {
            failure(
                PrerequisiteCode::UntrustedEndpoint,
                "Could not bind the VM capacity probe to the verified Docker Desktop daemon",
                false,
            )
        })?;
        let available = tokio::time::timeout(
            DESKTOP_CAPACITY_PROBE_TIMEOUT,
            self.api.desktop_storage_capacity(&image, &expected),
        )
        .await
        .map_err(|_| operation_timeout("Docker Desktop VM storage measurement"))??;
        snapshot.disk_available_bytes = Some(available);
        snapshot.docker_storage_accessible = true;
        Ok(())
    }

    async fn measure_desktop_capacity_if_available(
        &self,
        snapshot: &mut EngineSnapshot,
        image_digests: &[String],
    ) -> Result<bool, PrerequisiteFailure> {
        if !is_unmeasured_desktop_storage(
            self.desktop_transport_candidate,
            self.endpoint.is_desktop(),
            snapshot,
        ) {
            return Ok(false);
        }
        let image =
            ImageLock::bundled()?.select(RuntimeProfile::General, &snapshot.architecture)?;
        if !image_digests.contains(&image.reference) {
            return Ok(false);
        }
        self.measure_desktop_storage_capacity(snapshot, image_digests)
            .await?;
        Ok(true)
    }
}

fn is_unmeasured_desktop_storage(
    transport_candidate: bool,
    desktop_endpoint: bool,
    snapshot: &EngineSnapshot,
) -> bool {
    transport_candidate
        && desktop_endpoint
        && snapshot.docker_desktop
        && !snapshot.docker_storage_accessible
        && snapshot.disk_available_bytes.is_none()
}
