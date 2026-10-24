use std::collections::{BTreeMap, BTreeSet};

use crate::{
    harness::prompt_catalog::ReviewVerdictV1,
    runtime_qa::{
        QaAssertionStatus, QaCaseStatus, QaCoverageStatus, QaEvidenceManifestV1, QaResultV1,
        QaScenarioV1,
    },
};

use super::coverage::inspected_visual_screenshots;

pub(super) fn record_visual_inspection_status(
    scenarios: &[QaScenarioV1],
    evidence: &mut QaEvidenceManifestV1,
    result: &QaResultV1,
) {
    let visual_targets = scenarios
        .iter()
        .filter(|scenario| !scenario.visual_expectations.is_empty())
        .flat_map(|scenario| {
            scenario
                .requirement_ids
                .iter()
                .map(move |requirement| (requirement.as_str(), scenario.id.as_str()))
        })
        .collect::<BTreeSet<_>>();
    if visual_targets.is_empty() {
        return;
    }
    let every_visual_scenario_inspected =
        visual_targets.iter().all(|(requirement, scenario_id)| {
            result
                .review_coverage
                .iter()
                .find(|coverage| coverage.requirement_id == *requirement)
                .is_some_and(|coverage| {
                    evidence.screenshots.iter().any(|screenshot| {
                        screenshot.scenario_id == *scenario_id
                            && result.reviewed_screenshot_ids.contains(&screenshot.id)
                            && coverage.evidence_refs.contains(&screenshot.id)
                    })
                })
        });
    evidence
        .logs
        .retain(|line| line != super::super::VISUAL_REVIEW_PENDING_LOG);
    evidence.logs.truncate(255);
    evidence.logs.push(if every_visual_scenario_inspected {
        "independent reviewer inspected screenshot pixels for every visual scenario".into()
    } else {
        "some visual expectations lack cited, reviewer-inspected screenshot evidence".into()
    });
}

pub(super) fn resolve_visual_only_coverage(
    scenarios: &[QaScenarioV1],
    evidence: &mut QaEvidenceManifestV1,
    result: &mut QaResultV1,
    verdict: &ReviewVerdictV1,
    inspected_screenshots: &BTreeSet<&str>,
) -> Result<(), String> {
    let reviewed_visual = verdict
        .coverage
        .iter()
        .filter(|coverage| {
            !verdict.untested.contains(&coverage.requirement)
                && !verdict
                    .findings
                    .iter()
                    .any(|finding| finding.requirement == coverage.requirement)
        })
        .filter_map(|coverage| {
            inspected_visual_screenshots(
                &coverage.requirement,
                coverage,
                scenarios,
                evidence,
                inspected_screenshots,
            )
            .map(|screenshots| (coverage.requirement.clone(), screenshots))
        })
        .collect::<BTreeMap<_, _>>();
    if reviewed_visual.is_empty() {
        return Ok(());
    }

    for assertion in &mut evidence.assertions {
        if assertion.status != QaAssertionStatus::Untested
            || assertion
                .requirement_ids
                .iter()
                .any(|requirement| !reviewed_visual.contains_key(requirement))
        {
            continue;
        }
        let mut screenshot_ids = BTreeSet::new();
        let mut fully_inspected = true;
        for scenario_id in &assertion.scenario_ids {
            let mut scenario_screenshots = Vec::new();
            for requirement in &assertion.requirement_ids {
                scenario_screenshots.extend(
                    reviewed_visual
                        .get(requirement)
                        .and_then(|by_scenario| by_scenario.get(scenario_id))
                        .into_iter()
                        .flatten()
                        .cloned(),
                );
            }
            if scenario_screenshots.is_empty() {
                fully_inspected = false;
                break;
            }
            screenshot_ids.extend(scenario_screenshots);
        }
        if !fully_inspected || screenshot_ids.is_empty() {
            continue;
        }
        assertion.status = QaAssertionStatus::Passed;
        assertion.observed =
            "The independent reviewer inspected the redacted screenshot and confirmed the visual expectation.".into();
        let existing_refs = assertion
            .evidence_refs
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        assertion
            .evidence_refs
            .extend(screenshot_ids.difference(&existing_refs).cloned());
    }

    let resolved = reviewed_visual
        .keys()
        .filter(|requirement| {
            let requirement = requirement.as_str();
            let assertions = evidence
                .assertions
                .iter()
                .filter(|assertion| assertion.requirement_ids.iter().any(|id| id == requirement))
                .collect::<Vec<_>>();
            !assertions.is_empty()
                && assertions
                    .iter()
                    .all(|assertion| assertion.status == QaAssertionStatus::Passed)
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    if resolved.is_empty() {
        return Ok(());
    }

    for coverage in &mut result.coverage.requirements {
        if !resolved.contains(&coverage.requirement_id) {
            continue;
        }
        let mut screenshot_ids = BTreeSet::new();
        if let Some(scenarios) = reviewed_visual.get(&coverage.requirement_id) {
            for ids in scenarios.values() {
                screenshot_ids.extend(ids.iter().cloned());
            }
        }
        if screenshot_ids.is_empty() {
            return Err(
                "visual QA review resolved a requirement without inspected screenshots".into(),
            );
        }
        coverage.status = QaCoverageStatus::Observed;
        coverage.evidence_refs = screenshot_ids.into_iter().collect();
    }

    for case in &mut result.cases {
        if case.status != QaCaseStatus::Skipped {
            continue;
        }
        let Some(scenario) = scenarios
            .iter()
            .find(|scenario| scenario.id == case.scenario_id)
        else {
            return Err("visual QA case refers to an unknown scenario".into());
        };
        let assertions = evidence
            .assertions
            .iter()
            .filter(|assertion| assertion.scenario_ids.contains(&scenario.id))
            .collect::<Vec<_>>();
        if !assertions.is_empty()
            && assertions
                .iter()
                .all(|assertion| assertion.status == QaAssertionStatus::Passed)
        {
            case.status = QaCaseStatus::Passed;
            case.observations
                .push("The independent reviewer inspected the scenario's visual evidence.".into());
        }
    }

    evidence.coverage = result
        .coverage
        .requirements
        .iter()
        .map(|coverage| format!("{}:{:?}", coverage.requirement_id, coverage.status))
        .collect();
    result.coverage.states = evidence
        .assertions
        .iter()
        .map(|assertion| assertion.observed.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    result.evidence_manifest_sha256 = evidence.sha256()?;
    Ok(())
}
