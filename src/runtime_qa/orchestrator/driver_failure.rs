pub(super) use super::super::diagnostic_redaction::sanitized_reason;
use super::{QaAssertionStatus, QaDisplayV1, QaDriverOutputV1};
use crate::runtime_qa::QaAssertionV1;

#[cfg(test)]
#[path = "driver_failure/tests.rs"]
mod tests;

pub(super) fn blocked_output(
    request: &super::QaRunRequestV1,
    scenario: &super::QaScenarioV1,
    reason: &str,
) -> QaDriverOutputV1 {
    let reason = sanitized_reason(reason);
    let assertions = scenario
        .requirement_ids
        .iter()
        .map(|requirement_id| {
            let expected = request
                .requirement_ids
                .iter()
                .position(|candidate| candidate == requirement_id)
                .and_then(|index| request.acceptance_criteria.get(index))
                .cloned()
                .or_else(|| scenario.expected_results.first().cloned())
                .unwrap_or_else(|| "The approved QA requirement is observed".into());
            QaAssertionV1 {
                id: uuid::Uuid::new_v4().hyphenated().to_string(),
                scenario_ids: vec![scenario.id.clone()],
                requirement_ids: vec![requirement_id.clone()],
                expected,
                observed: format!("No runtime observation was produced: {reason}"),
                status: QaAssertionStatus::Blocked,
                evidence_refs: Vec::new(),
            }
        })
        .collect();
    QaDriverOutputV1 {
        driver_version: request.driver_profile_version.clone(),
        completed: false,
        failure: Some(reason.clone()),
        display: QaDisplayV1 {
            width: Some(1280),
            height: Some(720),
            dpi: Some(96),
            theme: "unobserved".into(),
            locale: "unknown".into(),
        },
        actions: Vec::new(),
        screenshots: Vec::new(),
        assertions,
        logs: vec![reason],
        crashes: Vec::new(),
        browser_console: Vec::new(),
        failed_requests: Vec::new(),
    }
}
