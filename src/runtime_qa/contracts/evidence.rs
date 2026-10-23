use super::QaPlatform;
use serde::{Deserialize, Serialize};

mod artifacts;
mod validation;
pub use artifacts::{QaScreenshotArtifactV1, validate_screenshot_artifact};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaDisplayV1 {
    #[serde(default)]
    pub width: Option<u16>,
    #[serde(default)]
    pub height: Option<u16>,
    #[serde(default)]
    pub dpi: Option<u16>,
    pub theme: String,
    pub locale: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaActionEvidenceV1 {
    pub index: u32,
    pub scenario_id: String,
    pub command: String,
    pub args_redacted: std::collections::BTreeMap<String, String>,
    pub pre_state: Option<String>,
    pub post_state: Option<String>,
    pub evidence_ids: Vec<String>,
    pub occurred_at_unix_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaScreenshotV1 {
    pub id: String,
    pub scenario_id: String,
    pub sha256: String,
    pub mime: String,
    pub width: u16,
    pub height: u16,
    pub source_action: u32,
    pub storage_ref: String,
    pub redaction_masks: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QaAssertionStatus {
    Passed,
    Failed,
    Untested,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaAssertionV1 {
    pub id: String,
    pub scenario_ids: Vec<String>,
    pub requirement_ids: Vec<String>,
    pub expected: String,
    pub observed: String,
    pub status: QaAssertionStatus,
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaEvidenceManifestV1 {
    pub schema_version: u16,
    pub qa_run_id: String,
    pub task_uid: String,
    pub code_sha: String,
    pub approved_spec_hash: String,
    pub build_hash: String,
    pub platform: QaPlatform,
    pub driver_version: String,
    pub display: QaDisplayV1,
    pub scenario_ids: Vec<String>,
    pub actions: Vec<QaActionEvidenceV1>,
    pub screenshots: Vec<QaScreenshotV1>,
    pub assertions: Vec<QaAssertionV1>,
    pub logs: Vec<String>,
    pub crashes: Vec<String>,
    pub browser_console: Vec<String>,
    pub failed_requests: Vec<String>,
    pub coverage: Vec<String>,
    pub retention_policy: String,
}

/// Per-scenario result returned by a concrete app-owned platform driver.
/// Screenshot paths are assigned later by the controller during persistence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaDriverOutputV1 {
    pub driver_version: String,
    pub completed: bool,
    pub failure: Option<String>,
    pub display: QaDisplayV1,
    pub actions: Vec<QaActionEvidenceV1>,
    pub screenshots: Vec<QaDriverScreenshotV1>,
    pub assertions: Vec<QaAssertionV1>,
    pub logs: Vec<String>,
    pub crashes: Vec<String>,
    pub browser_console: Vec<String>,
    pub failed_requests: Vec<String>,
}

/// Screenshot metadata and bytes emitted by a platform driver before storage.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QaDriverScreenshotV1 {
    pub id: String,
    pub scenario_id: String,
    pub sha256: String,
    pub width: u16,
    pub height: u16,
    pub source_action: u32,
    pub redaction_masks: Vec<String>,
    pub bytes: Vec<u8>,
}
