use super::{
    MAX_DRIVER_EVIDENCE_BYTES, MAX_DRIVER_SCREENSHOT_BYTES, base64::decode_base64, fields::*,
};
use crate::runtime_qa::{
    QaActionEvidenceV1, QaAssertionStatus, QaAssertionV1, QaDriverActionV1, QaDriverOutputV1,
    QaDriverScreenshotV1, QaRunRequestV1, QaScenarioV1,
};
use serde_json::Value;

/// Validate the image driver's JSON and bind every returned item to its input.
pub fn parse_browser_output(
    bytes: &[u8],
    request: &QaRunRequestV1,
    scenario: &QaScenarioV1,
) -> Result<QaDriverOutputV1, String> {
    let max_report_bytes = request
        .bounded_limits
        .max_evidence_bytes
        .min(MAX_DRIVER_EVIDENCE_BYTES)
        .saturating_mul(4)
        .saturating_div(3)
        .saturating_add(4 * 1024 * 1024);
    if bytes.len() as u64 > max_report_bytes {
        return Err("browser driver report exceeds its evidence bound".into());
    }
    let report: Value =
        serde_json::from_slice(bytes).map_err(|_| "browser driver returned malformed JSON")?;
    if number(&report, "schema_version")? != 1
        || text(&report, "qa_run_id")? != request.qa_run_id.as_str()
        || text(&report, "scenario_id")? != scenario.id.as_str()
        || text(&report, "task_uid")? != request.parent_task_uid.as_str()
        || text(&report, "code_sha")? != request.code_head_sha.as_str()
        || text(&report, "build_sha256")? != request.build_artifact_sha256.as_str()
        || text(&report, "approved_spec_hash")? != request.approved_spec_hash.as_str()
        || text(&report, "driver_profile_version")? != request.driver_profile_version.as_str()
        || text(&report, "origin")? != "http://qa-preview:8080"
    {
        return Err("browser driver report does not match its immutable request".into());
    }
    let outcome = text(&report, "outcome")?;
    let policy_violation = report
        .get("policy_violation")
        .and_then(Value::as_str)
        .map(crate::runtime_qa::diagnostic_redaction::sanitized_reason);
    let raw_actions = array(&report, "actions")?;
    let has_failed_action = raw_actions
        .iter()
        .any(|action| action.get("status").and_then(Value::as_str) == Some("failed"));
    let interaction_failure =
        outcome == "failed" && has_failed_action && policy_violation.is_none();
    let completed = (outcome == "completed" || interaction_failure) && policy_violation.is_none();
    if outcome != "completed" && outcome != "failed" {
        return Err("browser driver reported an unsupported outcome".into());
    }
    if raw_actions.len() > scenario.steps.len() {
        return Err("browser driver returned more actions than the scenario requested".into());
    }
    let mut action_rows = Vec::with_capacity(raw_actions.len());
    let mut screenshot_links = Vec::with_capacity(raw_actions.len());
    let now = now_unix_ms();
    for (expected_index, raw) in raw_actions.iter().enumerate() {
        let status = text(raw, "status")?;
        if number(raw, "index")? != expected_index as u64
            || !matches!(status, "observed" | "failed")
            || text(raw, "action")? != action_name(&scenario.steps[expected_index].driver_action)
        {
            return Err("browser driver action trace differs from the requested scenario".into());
        }
        let args = object(raw, "args_redacted")?
            .iter()
            .filter_map(|(key, value)| {
                let raw_value = match value {
                    Value::String(text) => text.clone(),
                    Value::Null => return None,
                    _ => value.to_string(),
                };
                let (key, value) =
                    crate::runtime_qa::diagnostic_redaction::sanitized_field(key, &raw_value);
                Some((
                    key.chars().take(128).collect(),
                    value.chars().take(512).collect(),
                ))
            })
            .collect();
        let before = state_text(raw, "pre_state")?;
        let after = state_text(raw, "post_state")?;
        let ids = string_array(raw, "screenshot_ids", 64)?;
        let command = action_command(&scenario.steps[expected_index].driver_action);
        action_rows.push(QaActionEvidenceV1 {
            index: expected_index as u32,
            scenario_id: scenario.id.clone(),
            command: command.into(),
            args_redacted: args,
            pre_state: before,
            post_state: after,
            evidence_ids: ids.clone(),
            occurred_at_unix_ms: now,
        });
        screenshot_links.push(ids);
    }
    let raw_screenshots = array(&report, "screenshots")?;
    if raw_screenshots.len() > request.bounded_limits.max_screenshots.min(64) as usize {
        return Err("browser driver exceeded its screenshot count bound".into());
    }
    let mut screenshots = Vec::with_capacity(raw_screenshots.len());
    let mut total_bytes = 0_u64;
    for raw in raw_screenshots {
        let id = text(raw, "id")?.to_owned();
        if !uuid::Uuid::parse_str(&id).is_ok_and(|parsed| parsed.hyphenated().to_string() == id) {
            return Err("browser driver screenshot identity is invalid".into());
        }
        let source_action = screenshot_links
            .iter()
            .position(|ids| ids.iter().any(|candidate| candidate == &id))
            .ok_or("browser driver returned an unreferenced screenshot")?;
        let encoded = text(raw, "bytes_base64")?;
        let remaining = request
            .bounded_limits
            .max_evidence_bytes
            .min(MAX_DRIVER_EVIDENCE_BYTES)
            .saturating_sub(total_bytes);
        let image = decode_base64(encoded, MAX_DRIVER_SCREENSHOT_BYTES.min(remaining as usize))?;
        total_bytes = total_bytes.saturating_add(image.len() as u64);
        screenshots.push(QaDriverScreenshotV1 {
            id,
            scenario_id: scenario.id.clone(),
            sha256: text(raw, "sha256")?.to_owned(),
            width: bounded_u16(number(raw, "width")?)?,
            height: bounded_u16(number(raw, "height")?)?,
            source_action: source_action as u32,
            redaction_masks: Vec::new(),
            bytes: image,
        });
    }
    if total_bytes
        > request
            .bounded_limits
            .max_evidence_bytes
            .min(MAX_DRIVER_EVIDENCE_BYTES)
    {
        return Err("browser driver exceeded its total evidence byte bound".into());
    }
    let mut assertions = Vec::new();
    for (index, step) in scenario.steps.iter().enumerate() {
        let expected = match &step.driver_action {
            QaDriverActionV1::BrowserAssertText { expected_text, .. } => expected_text,
            QaDriverActionV1::BrowserAssertUrl { url } => url,
            _ => continue,
        };
        let observed = action_rows
            .get(index)
            .filter(|row| row.command == "evaluate_limited_assertions");
        let failed = raw_actions
            .get(index)
            .and_then(|action| action.get("status"))
            .and_then(Value::as_str)
            == Some("failed");
        let evidence_refs = observed.map_or_else(Vec::new, |row| row.evidence_ids.clone());
        assertions.push(QaAssertionV1 {
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            scenario_ids: vec![scenario.id.clone()],
            requirement_ids: scenario.requirement_ids.clone(),
            expected: expected.clone(),
            observed: if observed.is_some() && !failed {
                "Playwright DOM text assertion completed".into()
            } else if failed {
                "The browser driver observed that the expected interaction state was not reached"
                    .into()
            } else {
                "The browser driver did not reach this assertion".into()
            },
            status: if observed.is_some() && !failed {
                QaAssertionStatus::Passed
            } else if failed || completed {
                QaAssertionStatus::Failed
            } else {
                QaAssertionStatus::Blocked
            },
            evidence_refs,
        });
    }
    for (index, raw) in raw_actions.iter().enumerate() {
        if text(raw, "status")? != "failed"
            || matches!(
                scenario.steps[index].driver_action,
                QaDriverActionV1::BrowserAssertText { .. }
                    | QaDriverActionV1::BrowserAssertUrl { .. }
            )
        {
            continue;
        }
        let expected = scenario.steps[index]
            .expected_state
            .clone()
            .or_else(|| scenario.expected_results.first().cloned())
            .unwrap_or_else(|| "The approved interaction completes".into());
        let observed = action_rows[index]
            .post_state
            .clone()
            .unwrap_or_else(|| "The approved interaction did not complete".into());
        assertions.push(QaAssertionV1 {
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            scenario_ids: vec![scenario.id.clone()],
            requirement_ids: scenario.requirement_ids.clone(),
            expected,
            observed,
            status: QaAssertionStatus::Failed,
            evidence_refs: action_rows[index].evidence_ids.clone(),
        });
    }
    let viewport = object(&report, "viewport")?;
    let width = bounded_u16(number_map(viewport, "width")?)?;
    let height = bounded_u16(number_map(viewport, "height")?)?;
    let driver =
        crate::runtime_qa::diagnostic_redaction::sanitized_reason(text(&report, "driver_version")?);
    let mut logs = vec![driver];
    let report_error = report
        .get("error")
        .and_then(Value::as_str)
        .map(crate::runtime_qa::diagnostic_redaction::sanitized_reason);
    let failure = if interaction_failure {
        None
    } else {
        report_error.as_deref().or(policy_violation.as_deref())
    };
    if let Some(error) = &report_error {
        logs.push(error.chars().take(4096).collect());
    }
    Ok(QaDriverOutputV1 {
        driver_version: request.driver_profile_version.clone(),
        completed,
        failure: failure.map(str::to_owned),
        display: crate::runtime_qa::QaDisplayV1 {
            width,
            height,
            dpi: 96,
            theme: "light".into(),
            locale: "en-US".into(),
        },
        actions: action_rows,
        screenshots,
        assertions,
        logs,
        crashes: Vec::new(),
        browser_console: string_array(&report, "console_errors", 128)?
            .iter()
            .map(|diagnostic| crate::runtime_qa::diagnostic_redaction::sanitized_reason(diagnostic))
            .collect(),
        failed_requests: string_array(&report, "failed_requests", 128)?
            .iter()
            .map(|diagnostic| crate::runtime_qa::diagnostic_redaction::sanitized_reason(diagnostic))
            .collect(),
    })
}
