use crate::runtime_qa::{
    QaActionEvidenceV1, QaAssertionStatus, QaAssertionV1, QaDriverActionV1, QaHttpMethodV1,
    QaScenarioV1,
};
use serde_json::Value;

pub(super) fn service_assertions(
    scenario: &QaScenarioV1,
    raw_actions: &[Value],
    action_rows: &[QaActionEvidenceV1],
) -> Result<Vec<QaAssertionV1>, String> {
    let mut assertions = Vec::new();
    for (index, step) in scenario.steps.iter().enumerate() {
        let QaDriverActionV1::ApiRequest {
            method,
            path,
            expected_status,
            expected_body_contains,
            ..
        } = &step.driver_action
        else {
            continue;
        };
        let Some(raw) = raw_actions.get(index) else {
            assertions.push(QaAssertionV1 {
                id: uuid::Uuid::new_v4().hyphenated().to_string(),
                scenario_ids: vec![scenario.id.clone()],
                requirement_ids: scenario.requirement_ids.clone(),
                expected: step.expected_state.clone().unwrap_or_else(|| {
                    format!(
                        "{} {path} returns HTTP {expected_status}",
                        serde_json::to_string(method).unwrap_or_else(|_| "API".into())
                    )
                }),
                observed: "The service driver stopped before attempting this API request".into(),
                status: QaAssertionStatus::Blocked,
                evidence_refs: Vec::new(),
            });
            continue;
        };
        let args = raw
            .get("args_redacted")
            .and_then(Value::as_object)
            .ok_or("service driver omitted its redacted API request identity")?;
        let expected_method = match method {
            QaHttpMethodV1::Get => "GET",
            QaHttpMethodV1::Post => "POST",
            QaHttpMethodV1::Put => "PUT",
            QaHttpMethodV1::Patch => "PATCH",
            QaHttpMethodV1::Delete => "DELETE",
        };
        if args.get("method").and_then(Value::as_str) != Some(expected_method)
            || args.get("path").and_then(Value::as_str) != Some(path.as_str())
        {
            return Err("service driver API evidence differs from the approved request".into());
        }
        let response_status = raw.get("response_status").and_then(Value::as_u64);
        let response_hash = raw.get("response_sha256").and_then(Value::as_str);
        let body_matches = raw.get("body_matches").and_then(Value::as_bool);
        let evidence_refs = action_rows
            .get(index)
            .map_or_else(Vec::new, |row| row.evidence_ids.clone());
        let observed = match (response_status, response_hash, body_matches) {
            (Some(status), Some(hash), Some(matches))
                if (100..=599).contains(&status)
                    && crate::runtime_qa::contracts::is_sha256(hash) =>
            {
                format!("HTTP {status}; response sha256={hash}; expected body match={matches}")
            }
            _ => action_rows
                .get(index)
                .and_then(|row| row.post_state.clone())
                .unwrap_or_else(|| "The API request did not produce a response".into()),
        };
        let passed = raw.get("status").and_then(Value::as_str) == Some("observed")
            && response_status == Some(u64::from(*expected_status))
            && body_matches.unwrap_or(expected_body_contains.is_none());
        let status = if raw.get("status").and_then(Value::as_str) == Some("failed") {
            QaAssertionStatus::Failed
        } else if response_status.is_some() && response_hash.is_some() {
            if passed {
                QaAssertionStatus::Passed
            } else {
                QaAssertionStatus::Failed
            }
        } else {
            QaAssertionStatus::Blocked
        };
        assertions.push(QaAssertionV1 {
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            scenario_ids: vec![scenario.id.clone()],
            requirement_ids: scenario.requirement_ids.clone(),
            expected: step.expected_state.clone().unwrap_or_else(|| {
                format!(
                    "{} {path} returns HTTP {expected_status}{}",
                    serde_json::to_string(method).unwrap_or_else(|_| "API".into()),
                    if expected_body_contains.is_some() {
                        " with the approved response fragment"
                    } else {
                        ""
                    }
                )
            }),
            observed,
            status,
            evidence_refs,
        });
    }
    Ok(assertions)
}
