use serde::{Deserialize, Serialize};

/// Durations measured by the application around the named operation. Missing
/// values mean that operation was not measured for this record.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPhaseTimingsV1 {
    #[serde(default)]
    pub image_preparation_ms: Option<u64>,
    #[serde(default)]
    pub workspace_stage_in_ms: Option<u64>,
    #[serde(default)]
    pub workspace_stage_out_ms: Option<u64>,
    #[serde(default)]
    pub dependency_cache_stage_ms: Option<u64>,
    #[serde(default)]
    pub qa_ms: Option<u64>,
    #[serde(default)]
    pub cleanup_ms: Option<u64>,
}

impl ExecutionPhaseTimingsV1 {
    pub(super) fn validate(&self) -> bool {
        const MAX_MEASURED_PHASE_MS: u64 = 24 * 60 * 60 * 1000;
        [
            self.image_preparation_ms,
            self.workspace_stage_in_ms,
            self.workspace_stage_out_ms,
            self.dependency_cache_stage_ms,
            self.qa_ms,
            self.cleanup_ms,
        ]
        .into_iter()
        .flatten()
        .all(|duration| duration <= MAX_MEASURED_PHASE_MS)
    }
}
