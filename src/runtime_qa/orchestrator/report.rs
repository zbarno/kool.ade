use super::*;
use crate::runtime_qa::QaCoverageV1;

mod redaction;

pub(super) struct ReportAssembly {
    pub evidence: QaEvidenceManifestV1,
    pub screenshots: Vec<QaScreenshotArtifactV1>,
    pub cases: Vec<QaCaseResultV1>,
    pub coverage: QaCoverageV1,
    pub outcome: QaOutcome,
}

pub(super) fn assemble_report(
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    outputs: Vec<QaDriverOutputV1>,
) -> Result<ReportAssembly, String> {
    let platform = request.relevant_platforms[0];
    let mut actions = Vec::<QaActionEvidenceV1>::new();
    let mut screenshots = Vec::<QaScreenshotArtifactV1>::new();
    let mut screenshot_metadata = Vec::<QaScreenshotV1>::new();
    let mut assertions = Vec::new();
    let mut cases = Vec::with_capacity(scenarios.len());
    let mut logs = Vec::new();
    let mut crashes = Vec::new();
    let mut browser_console = Vec::new();
    let mut failed_requests = Vec::new();
    let mut display: Option<QaDisplayV1> = None;
    let mut any_infrastructure_failure = false;
    let mut any_unfinished = false;

    for (scenario, output) in scenarios.iter().zip(outputs) {
        if display
            .as_ref()
            .is_some_and(|prior| prior != &output.display)
        {
            return Err("QA driver changed display profile during one run".into());
        }
        display.get_or_insert_with(|| output.display.clone());
        let action_start = actions.len();
        let mut scenario_evidence = Vec::new();
        for mut action in output.actions {
            if action.scenario_id != scenario.id {
                return Err("QA driver returned an action for a different scenario".into());
            }
            action.index = actions.len() as u32;
            if platform == QaPlatform::HeadlessService {
                scenario_evidence.extend(action.evidence_ids.iter().cloned());
            }
            actions.push(action);
        }
        for artifact in output.screenshots {
            if artifact.scenario_id != scenario.id {
                return Err("QA driver returned a screenshot for a different scenario".into());
            }
            let Some(action) = actions.get(action_start + artifact.source_action as usize) else {
                return Err("QA screenshot does not refer to an observed action".into());
            };
            if !artifact.redaction_masks.is_empty() {
                return Err(
                    "QA driver cannot supply unverifiable screenshot redaction metadata".into(),
                );
            }
            let display_width = output
                .display
                .width
                .ok_or("QA driver screenshot has no reported display width")?;
            let display_height = output
                .display
                .height
                .ok_or("QA driver screenshot has no reported display height")?;
            let redacted = redaction::apply(
                &artifact,
                &scenario.redaction_regions,
                request.bounded_limits.max_evidence_bytes,
                display_width,
                display_height,
            )?;
            let source_action = action.index;
            let storage_ref = format!(
                "qa-evidence/{}/screenshots/{}.png",
                request.qa_run_id, artifact.id
            );
            scenario_evidence.push(artifact.id.clone());
            screenshot_metadata.push(QaScreenshotV1 {
                id: artifact.id.clone(),
                scenario_id: scenario.id.clone(),
                sha256: redacted.sha256,
                mime: "image/png".into(),
                width: artifact.width,
                height: artifact.height,
                source_action,
                storage_ref,
                redaction_masks: redacted.mask_descriptions,
            });
            screenshots.push(QaScreenshotArtifactV1 {
                id: artifact.id,
                bytes: redacted.bytes,
            });
        }
        let scenario_assertions = output.assertions;
        if scenario_assertions.iter().any(|assertion| {
            assertion.scenario_ids != [scenario.id.clone()]
                || assertion
                    .requirement_ids
                    .iter()
                    .any(|id| !scenario.requirement_ids.contains(id))
        }) {
            return Err("QA driver returned assertions outside the scenario requirements".into());
        }
        let mut scenario_failed = false;
        let mut scenario_untested = false;
        for assertion in &scenario_assertions {
            if assertion.status == QaAssertionStatus::Passed && assertion.evidence_refs.is_empty() {
                return Err("QA assertion cannot pass without observed evidence".into());
            }
            scenario_failed |= assertion.status == QaAssertionStatus::Failed;
            scenario_untested |= assertion.status != QaAssertionStatus::Passed;
            assertions.push(assertion.clone());
        }
        let completed = output.completed && output.failure.is_none();
        any_infrastructure_failure |= !completed && output.failure.is_some();
        any_unfinished |= !completed || scenario_untested || scenario_assertions.is_empty();
        let status = if !completed {
            QaCaseStatus::Blocked
        } else if scenario_failed {
            QaCaseStatus::Failed
        } else if scenario_untested || scenario_assertions.is_empty() {
            QaCaseStatus::Skipped
        } else {
            QaCaseStatus::Passed
        };
        if matches!(status, QaCaseStatus::Passed | QaCaseStatus::Failed)
            && scenario_evidence.is_empty()
        {
            return Err("completed QA scenarios require platform evidence".into());
        }
        scenario_evidence.sort();
        scenario_evidence.dedup();
        let mut observations = output.failure.into_iter().collect::<Vec<_>>();
        observations.extend(output.logs.iter().take(32).cloned());
        cases.push(QaCaseResultV1 {
            scenario_id: scenario.id.clone(),
            status,
            evidence_refs: scenario_evidence,
            observations,
        });
        logs.extend(output.logs);
        crashes.extend(output.crashes);
        browser_console.extend(output.browser_console);
        failed_requests.extend(output.failed_requests);
    }
    for requirement in &request.requirement_ids {
        if assertions
            .iter()
            .any(|assertion| assertion.requirement_ids.contains(requirement))
        {
            continue;
        }
        let scenario = scenarios
            .iter()
            .find(|scenario| scenario.requirement_ids.contains(requirement))
            .ok_or_else(|| {
                "approved requirement has no scenario during report assembly".to_owned()
            })?;
        let evidence_refs = screenshot_metadata
            .iter()
            .filter(|screenshot| screenshot.scenario_id == scenario.id)
            .map(|screenshot| screenshot.id.clone())
            .collect();
        let index = request
            .requirement_ids
            .iter()
            .position(|item| item == requirement)
            .expect("requirement came from request");
        assertions.push(super::super::QaAssertionV1 {
            id: uuid::Uuid::new_v4().hyphenated().to_string(),
            scenario_ids: vec![scenario.id.clone()],
            requirement_ids: vec![requirement.clone()],
            expected: request.acceptance_criteria[index].clone(),
            observed: "No deterministic observation was recorded for this requirement".into(),
            status: QaAssertionStatus::Untested,
            evidence_refs,
        });
    }
    let display = display.ok_or_else(|| "runtime QA produced no driver observations".to_owned())?;
    if actions.len() > request.bounded_limits.max_total_actions as usize {
        return Err("runtime QA action trace exceeds its approved bound".into());
    }
    let mut requirement_coverage = Vec::with_capacity(request.requirement_ids.len());
    for requirement in &request.requirement_ids {
        let matching = assertions
            .iter()
            .filter(|item| item.requirement_ids.contains(requirement))
            .collect::<Vec<_>>();
        let evidence_refs = matching
            .iter()
            .flat_map(|item| item.evidence_refs.iter().cloned())
            .collect::<Vec<_>>();
        let status = if matching
            .iter()
            .any(|item| item.status == QaAssertionStatus::Blocked)
        {
            QaCoverageStatus::Blocked
        } else if matching.iter().any(|item| {
            item.status == QaAssertionStatus::Passed || item.status == QaAssertionStatus::Failed
        }) {
            QaCoverageStatus::Observed
        } else {
            QaCoverageStatus::Untested
        };
        requirement_coverage.push(QaRequirementCoverageV1 {
            requirement_id: requirement.clone(),
            status,
            evidence_refs,
        });
    }
    let scenario_ids = scenarios
        .iter()
        .map(|scenario| scenario.id.clone())
        .collect::<Vec<_>>();
    let evidence = QaEvidenceManifestV1 {
        schema_version: 1,
        qa_run_id: request.qa_run_id.clone(),
        task_uid: request.parent_task_uid.clone(),
        target_id: request.target_id.clone(),
        code_sha: request.code_head_sha.clone(),
        approved_spec_hash: request.approved_spec_hash.clone(),
        build_hash: request.build_artifact_sha256.clone(),
        platform,
        driver_version: request.driver_profile_version.clone(),
        display,
        scenario_ids: scenario_ids.clone(),
        actions,
        screenshots: screenshot_metadata,
        assertions: assertions.clone(),
        logs: bounded(logs, 256),
        crashes: bounded(crashes, 256),
        browser_console: bounded(browser_console, 4096),
        failed_requests: bounded(failed_requests, 4096),
        coverage: requirement_coverage
            .iter()
            .map(|coverage| format!("{}:{:?}", coverage.requirement_id, coverage.status))
            .collect(),
        retention_policy: "managed_local_immutable".into(),
    };
    let outcome = if any_infrastructure_failure {
        QaOutcome::InfrastructureBlocked
    } else if cases.iter().any(|case| case.status == QaCaseStatus::Failed)
        || assertions
            .iter()
            .any(|assertion| assertion.status == QaAssertionStatus::Failed)
    {
        QaOutcome::Fail
    } else if any_unfinished
        || requirement_coverage
            .iter()
            .any(|item| item.status != QaCoverageStatus::Observed)
    {
        QaOutcome::Incomplete
    } else {
        QaOutcome::Pass
    };
    Ok(ReportAssembly {
        evidence,
        screenshots,
        cases,
        coverage: QaCoverageV1 {
            requirements: requirement_coverage,
            scenario_ids,
            states: assertions
                .iter()
                .map(|item| item.observed.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
        },
        outcome,
    })
}

fn bounded(mut values: Vec<String>, max: usize) -> Vec<String> {
    values.truncate(max);
    values
}
