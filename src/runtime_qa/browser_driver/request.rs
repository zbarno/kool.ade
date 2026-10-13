use super::MAX_DRIVER_EVIDENCE_BYTES;
use crate::runtime_qa::{QaDriverActionV1, QaRunRequestV1, QaScenarioV1};
use serde_json::{Value, json};

/// Serialize one validated web scenario into the pinned image's strict protocol.
pub fn build_browser_request(
    request: &QaRunRequestV1,
    scenario: &QaScenarioV1,
) -> Result<Vec<u8>, String> {
    request.validate()?;
    scenario.validate(request)?;
    let mut actions = Vec::with_capacity(scenario.steps.len());
    for step in &scenario.steps {
        if !matches!(
            &step.driver_action,
            QaDriverActionV1::BrowserOpen { .. }
                | QaDriverActionV1::BrowserClick { .. }
                | QaDriverActionV1::BrowserFill { .. }
                | QaDriverActionV1::BrowserPress { .. }
                | QaDriverActionV1::BrowserSelect { .. }
                | QaDriverActionV1::BrowserWaitForVisible { .. }
                | QaDriverActionV1::BrowserAssertText { .. }
                | QaDriverActionV1::BrowserAssertUrl { .. }
                | QaDriverActionV1::BrowserSetViewport { .. }
                | QaDriverActionV1::BrowserScreenshot
                | QaDriverActionV1::BrowserScroll { .. }
        ) {
            return Err(
                "browser driver received an action outside the typed web action set".into(),
            );
        }
        let mut action = serde_json::to_value(&step.driver_action)
            .map_err(|_| "browser QA action cannot be encoded")?;
        let object = action
            .as_object_mut()
            .ok_or("browser QA action did not encode as an object")?;
        object.insert(
            "screenshot_checkpoint".into(),
            Value::Bool(step.screenshot_checkpoint),
        );
        actions.push(action);
    }
    let max_evidence_bytes = request
        .bounded_limits
        .max_evidence_bytes
        .min(MAX_DRIVER_EVIDENCE_BYTES);
    let max_screenshots = request.bounded_limits.max_screenshots.min(64);
    let timeout_ms = request
        .bounded_limits
        .timeout_seconds
        .saturating_mul(1000)
        .min(600_000);
    serde_json::to_vec(&json!({
        "schema_version": 1,
        "qa_run_id": request.qa_run_id,
        "scenario_id": scenario.id,
        "task_uid": request.parent_task_uid,
        "code_sha": request.code_head_sha,
        "build_sha256": request.build_artifact_sha256,
        "approved_spec_hash": request.approved_spec_hash,
        "driver_profile_version": request.driver_profile_version,
        "origin": "http://qa-preview:8080",
        "viewport": { "width": 1280, "height": 720 },
        "locale": "en-US",
        "timezone": "UTC",
        "color_scheme": "light",
        "actions": actions,
        "max_screenshots": max_screenshots,
        "max_evidence_bytes": max_evidence_bytes,
        "timeout_ms": timeout_ms,
    }))
    .map_err(|_| "browser QA request cannot be encoded".into())
}
