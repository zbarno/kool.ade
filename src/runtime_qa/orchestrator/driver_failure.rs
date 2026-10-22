use super::{QaAssertionStatus, QaDisplayV1, QaDriverOutputV1};
use crate::runtime_qa::QaAssertionV1;

#[cfg(test)]
#[path = "driver_failure/tests.rs"]
mod tests;

pub(super) fn sanitized_reason(reason: &str) -> String {
    let mut output = String::new();
    let mut length = 0;
    for token in reason.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        let sensitive_header = token.split_once(':').is_some_and(|(name, _)| {
            matches!(
                name.trim_matches(|character: char| {
                    !character.is_ascii_alphanumeric() && character != '-'
                })
                .to_ascii_lowercase()
                .as_str(),
                "authorization"
                    | "proxy-authorization"
                    | "cookie"
                    | "set-cookie"
                    | "x-api-key"
                    | "x-auth-token"
            )
        });
        let sensitive_marker = [
            "token=",
            "secret=",
            "password=",
            "authorization=",
            "api_key=",
        ]
        .iter()
        .any(|key| lower.contains(key));
        let redact_remainder = sensitive_header || sensitive_marker || lower == "bearer";
        let sensitive = sensitive_header
            || token.starts_with('/')
            || token.contains('/')
            || token.contains('\\')
            || token.contains('@')
            || token.contains("://")
            || sensitive_marker
            || lower == "bearer";
        let safe = if sensitive { "[redacted]" } else { token };
        if !output.is_empty() {
            output.push(' ');
            length += 1;
        }
        for character in safe.chars().filter(|character| !character.is_control()) {
            if length >= 512 {
                break;
            }
            output.push(character);
            length += 1;
        }
        if redact_remainder {
            break;
        }
        if length >= 512 {
            break;
        }
    }
    if output.is_empty() {
        "isolated QA driver failed before producing observations".into()
    } else {
        output
    }
}

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
            width: 1280,
            height: 720,
            dpi: 96,
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
