use super::super::{QaEvidenceManifestV1, QaOutcome, QaResultV1, QaRunRequestV1, QaScenarioV1};

pub(super) fn apply_review_verdict(
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    evidence: &QaEvidenceManifestV1,
    result: &mut QaResultV1,
    verdict: crate::harness::prompt_catalog::ReviewVerdictV1,
) -> Result<(), String> {
    let expected_hash = evidence.sha256()?;
    if verdict.subject_hash != expected_hash
        || verdict.coverage.len() != request.requirement_ids.len()
        || request.requirement_ids.iter().any(|requirement| {
            verdict
                .coverage
                .iter()
                .filter(|entry| &entry.requirement == requirement)
                .count()
                != 1
        })
        || verdict.coverage.iter().any(|coverage| {
            !request.requirement_ids.contains(&coverage.requirement)
                || coverage.evidence.is_empty()
                || coverage.evidence.iter().any(|reference| {
                    !evidence
                        .screenshots
                        .iter()
                        .any(|shot| shot.id == *reference)
                        && !evidence
                            .actions
                            .iter()
                            .any(|action| action.index.to_string() == *reference)
                })
        })
    {
        return Err(
            "RuntimeQaReviewer verdict does not bind exact requirements and observed evidence"
                .into(),
        );
    }
    let reviewer_passes = verdict.pass
        && verdict.findings.is_empty()
        && verdict.untested.is_empty()
        && verdict.coverage.iter().all(|coverage| {
            matches!(
                &coverage.status,
                crate::harness::prompt_catalog::CoverageStatus::Pass
            )
        });
    result.outcome = match (result.outcome, reviewer_passes) {
        (QaOutcome::InfrastructureBlocked, _) => QaOutcome::InfrastructureBlocked,
        (QaOutcome::Incomplete, true) => QaOutcome::Incomplete,
        (QaOutcome::Pass, true) => QaOutcome::Pass,
        (QaOutcome::Pass | QaOutcome::Fail | QaOutcome::Incomplete, false)
        | (QaOutcome::Fail, true) => QaOutcome::Fail,
    };
    let mut rejected_requirements = verdict
        .coverage
        .iter()
        .filter(|coverage| {
            !matches!(&coverage.status, crate::harness::prompt_catalog::CoverageStatus::Pass)
        })
        .map(|coverage| (coverage.requirement.clone(), "independent reviewer could not verify the acceptance criterion from observed runtime evidence".to_owned()))
        .collect::<Vec<_>>();
    rejected_requirements.extend(verdict.untested.iter().map(|requirement| {
        (
            requirement.clone(),
            "independent reviewer marked the acceptance criterion untested".to_owned(),
        )
    }));
    if !verdict.pass && rejected_requirements.is_empty() && verdict.findings.is_empty() {
        rejected_requirements.push((
            request.requirement_ids[0].clone(),
            "independent reviewer rejected the runtime QA result".into(),
        ));
    }
    for (requirement, actual) in rejected_requirements {
        result.findings.push(rejection_finding(
            request,
            scenarios,
            evidence,
            &requirement,
            actual,
        )?);
    }
    for finding in verdict.findings {
        let scenario = scenarios
            .iter()
            .find(|scenario| {
                evidence.assertions.iter().any(|assertion| {
                    assertion.scenario_ids.contains(&scenario.id)
                        && assertion.requirement_ids.contains(&finding.requirement)
                })
            })
            .ok_or_else(|| {
                "RuntimeQaReviewer finding does not map to an observed scenario".to_owned()
            })?;
        let category = match finding.kind {
            crate::harness::prompt_catalog::FindingKind::Security => {
                super::super::QaFindingCategory::Security
            }
            crate::harness::prompt_catalog::FindingKind::Regression => {
                super::super::QaFindingCategory::Regression
            }
            crate::harness::prompt_catalog::FindingKind::TestGap => {
                super::super::QaFindingCategory::Environment
            }
            _ => super::super::QaFindingCategory::Functional,
        };
        let severity = match finding.severity {
            crate::harness::prompt_catalog::Severity::Blocker => {
                super::super::QaFindingSeverity::Critical
            }
            crate::harness::prompt_catalog::Severity::High => super::super::QaFindingSeverity::High,
            crate::harness::prompt_catalog::Severity::Medium => {
                super::super::QaFindingSeverity::Medium
            }
            crate::harness::prompt_catalog::Severity::Low => super::super::QaFindingSeverity::Low,
        };
        let smallest_fix = finding.proposed_correction.clone();
        result.findings.push(super::super::QaFindingV1 {
            finding_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            code_sha: request.code_head_sha.clone(),
            scenario_uid: scenario.id.clone(),
            requirement_uids: vec![finding.requirement],
            category,
            severity,
            expected: "The approved acceptance criterion is satisfied.".into(),
            actual: smallest_fix.clone(),
            reproducible_steps: finding
                .repro
                .or(Some(finding.location))
                .into_iter()
                .collect(),
            screenshot_ids: evidence
                .screenshots
                .iter()
                .filter(|shot| shot.scenario_id == scenario.id)
                .map(|shot| shot.id.clone())
                .collect(),
            trace_ids: evidence
                .actions
                .iter()
                .filter(|action| action.scenario_id == scenario.id)
                .map(|action| action.index.to_string())
                .collect(),
            smallest_fix,
            blocking: true,
        });
    }
    Ok(())
}

fn rejection_finding(
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    evidence: &QaEvidenceManifestV1,
    requirement: &str,
    actual: String,
) -> Result<super::super::QaFindingV1, String> {
    let scenario = scenarios
        .iter()
        .find(|scenario| {
            evidence.assertions.iter().any(|assertion| {
                assertion.scenario_ids.contains(&scenario.id)
                    && assertion.requirement_ids.iter().any(|id| id == requirement)
            })
        })
        .ok_or_else(|| "review rejection does not map to observed scenario evidence".to_owned())?;
    let screenshot_ids = evidence
        .screenshots
        .iter()
        .filter(|shot| shot.scenario_id == scenario.id)
        .map(|shot| shot.id.clone())
        .collect::<Vec<_>>();
    let trace_ids = evidence
        .actions
        .iter()
        .filter(|action| action.scenario_id == scenario.id)
        .map(|action| action.index.to_string())
        .collect::<Vec<_>>();
    if screenshot_ids.is_empty() && trace_ids.is_empty() {
        return Err("review rejection has no observed screenshot or action evidence".into());
    }
    Ok(super::super::QaFindingV1 {
        finding_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        code_sha: request.code_head_sha.clone(),
        scenario_uid: scenario.id.clone(),
        requirement_uids: vec![requirement.to_owned()],
        category: super::super::QaFindingCategory::Functional,
        severity: super::super::QaFindingSeverity::High,
        expected: "The approved acceptance criterion is satisfied.".into(),
        actual: actual.clone(),
        reproducible_steps: vec![actual.clone()],
        screenshot_ids,
        trace_ids,
        smallest_fix: actual,
        blocking: true,
    })
}

pub(super) fn failed_assertion_findings(
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    evidence: &QaEvidenceManifestV1,
) -> Vec<super::super::QaFindingV1> {
    evidence
        .assertions
        .iter()
        .filter(|assertion| assertion.status == super::super::QaAssertionStatus::Failed)
        .filter_map(|assertion| {
            let scenario_id = assertion.scenario_ids.first()?;
            let scenario = scenarios.iter().find(|item| &item.id == scenario_id)?;
            let screenshot_ids = evidence
                .screenshots
                .iter()
                .filter(|shot| &shot.scenario_id == scenario_id)
                .map(|shot| shot.id.clone())
                .collect();
            let trace_ids = evidence
                .actions
                .iter()
                .filter(|action| &action.scenario_id == scenario_id)
                .map(|action| action.index.to_string())
                .collect();
            Some(super::super::QaFindingV1 {
                finding_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
                code_sha: request.code_head_sha.clone(),
                scenario_uid: scenario.id.clone(),
                requirement_uids: assertion.requirement_ids.clone(),
                category: super::super::QaFindingCategory::Functional,
                severity: super::super::QaFindingSeverity::High,
                expected: assertion.expected.clone(),
                actual: assertion.observed.clone(),
                reproducible_steps: scenario
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(index, step)| {
                        format!("Step {}: {}", index + 1, action_label(&step.driver_action))
                    })
                    .collect(),
                screenshot_ids,
                trace_ids,
                smallest_fix: "Investigate and correct the observed acceptance-criteria mismatch."
                    .into(),
                blocking: true,
            })
        })
        .collect()
}

fn action_label(action: &super::super::QaDriverActionV1) -> &'static str {
    use super::super::QaDriverActionV1 as Action;
    match action {
        Action::LaunchValidatedTarget => "launch the validated application",
        Action::WaitForWindow => "wait for the application window",
        Action::ListWindows => "inspect available windows",
        Action::FocusWindow { .. } => "focus the application window",
        Action::ResizeWindow { .. } => "resize the application window",
        Action::Click { .. } => "click the application window",
        Action::DoubleClick { .. } => "double-click the application window",
        Action::Drag { .. } => "drag within the application window",
        Action::TypeText { .. } => "type approved test input",
        Action::KeyChord { .. } => "send an approved keyboard chord",
        Action::Scroll { .. } => "scroll the application window",
        Action::WaitForVisible { .. } => "wait for a visible element",
        Action::AssertRegionVisible { .. } => "assert native control region is visible",
        Action::NavigateAllowedUrl { .. } | Action::BrowserOpen { .. } => "open an allowed app URL",
        Action::FindByRole { .. } => "find an element by role",
        Action::FindByTestId { .. } => "find an element by test ID",
        Action::Fill { .. } | Action::BrowserFill { .. } => "fill an approved field",
        Action::Press { .. } | Action::BrowserPress { .. } => "press an approved key",
        Action::Select { .. } | Action::BrowserSelect { .. } => "select an approved option",
        Action::Screenshot | Action::BrowserScreenshot => "capture a screenshot",
        Action::ReadAppLogs => "read application logs",
        Action::CollectConsole => "collect browser console output",
        Action::CollectNetworkFailures => "collect failed browser requests",
        Action::BrowserClick { .. } => "click an approved browser locator",
        Action::BrowserWaitForVisible { .. } => "wait for a browser locator",
        Action::BrowserAssertText { .. } => "assert visible browser text",
        Action::BrowserAssertUrl { .. } => "assert the allowed browser URL",
        Action::BrowserSetViewport { .. } => "set the approved browser viewport",
        Action::BrowserScroll { .. } => "scroll to an approved browser locator",
        Action::CloseWindow { .. } => "close the application window",
    }
}
