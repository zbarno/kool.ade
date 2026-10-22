use super::{
    ExecutionPhaseTimingsV1,
    events::{ArtifactManifestEntryV1, ResourceMetricsV1},
};
use crate::execution::errors::{ExecutionError, ExecutionErrorKind};
use serde::{Deserialize, Serialize};

#[cfg(test)]
#[path = "result/tests.rs"]
mod tests;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionResultV1 {
    pub schema_version: u16,
    pub execution_id: String,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub metrics: ResourceMetricsV1,
    #[serde(default, skip_serializing_if = "ExecutionPhaseTimingsV1::is_empty")]
    pub phase_timings: ExecutionPhaseTimingsV1,
    pub artifacts: Vec<ArtifactManifestEntryV1>,
    pub failure: Option<ExecutionError>,
}

impl ExecutionResultV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported execution result schema"
        );
        super::ids::validate_identifier(&self.execution_id)?;
        self.metrics.validate()?;
        anyhow::ensure!(
            self.phase_timings.validate(),
            "execution phase timing is invalid"
        );
        anyhow::ensure!(
            !(self.timed_out && self.cancelled),
            "execution cannot be both timed out and cancelled"
        );
        anyhow::ensure!(
            self.exit_code.is_none() || self.signal.is_none(),
            "execution cannot have both an exit code and a signal"
        );
        anyhow::ensure!(
            self.artifacts.len() <= 256,
            "artifact manifest exceeds entry limit"
        );
        let total_artifact_bytes = self.artifacts.iter().try_fold(0_u64, |total, artifact| {
            total.checked_add(artifact.size_bytes)
        });
        anyhow::ensure!(
            total_artifact_bytes.is_some_and(|total| total <= 1024 * 1024 * 1024),
            "artifact manifest exceeds aggregate size limit"
        );
        for artifact in &self.artifacts {
            artifact.validate()?;
        }
        if self.timed_out {
            anyhow::ensure!(
                self.failure
                    .as_ref()
                    .is_some_and(|failure| failure.kind == ExecutionErrorKind::DeadlineExceeded),
                "timed out result must report its deadline failure"
            );
        }
        if let Some(failure) = &self.failure {
            failure.validate()?;
        }
        if self.cancelled {
            anyhow::ensure!(
                self.failure
                    .as_ref()
                    .is_some_and(|failure| failure.kind == ExecutionErrorKind::Cancelled),
                "cancelled result must report cancellation"
            );
        }
        Ok(())
    }
}
