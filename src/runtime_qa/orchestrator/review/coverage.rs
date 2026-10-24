use crate::{
    harness::prompt_catalog::{CoverageStatus, CoverageV1},
    runtime_qa::{
        QaAssertionStatus, QaEvidenceManifestV1, QaPlatform, QaRunRequestV1, QaScenarioV1,
    },
};
use std::collections::BTreeSet;

pub(super) fn has_inspected_screenshot_trace_and_assertion(
    coverage: &CoverageV1,
    evidence: &QaEvidenceManifestV1,
    inspected_screenshots: &BTreeSet<&str>,
) -> bool {
    let cited_screenshots = coverage
        .evidence
        .iter()
        .filter_map(|reference| {
            evidence
                .screenshots
                .iter()
                .find(|screenshot| screenshot.id == **reference)
        })
        .collect::<Vec<_>>();
    let cited_actions = evidence
        .actions
        .iter()
        .filter(|action| coverage.evidence.contains(&action.index.to_string()))
        .collect::<Vec<_>>();
    matches!(coverage.status, CoverageStatus::Pass)
        && cited_screenshots.iter().any(|screenshot| {
            inspected_screenshots.contains(screenshot.id.as_str())
                && cited_actions.iter().any(|action| {
                    action.scenario_id == screenshot.scenario_id
                        && evidence.assertions.iter().any(|assertion| {
                            assertion.requirement_ids.contains(&coverage.requirement)
                                && assertion.status == QaAssertionStatus::Passed
                                && assertion.scenario_ids.contains(&action.scenario_id)
                                && assertion
                                    .evidence_refs
                                    .iter()
                                    .any(|reference| coverage.evidence.contains(reference))
                        })
                })
        })
}

pub(super) fn has_grounded_requirement_evidence(
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    coverage: &CoverageV1,
    evidence: &QaEvidenceManifestV1,
    inspected_screenshots: &BTreeSet<&str>,
) -> bool {
    let visual_requirement = scenarios.iter().any(|scenario| {
        scenario.requirement_ids.contains(&coverage.requirement)
            && !scenario.visual_expectations.is_empty()
    });
    if visual_requirement {
        return inspected_visual_screenshots(
            &coverage.requirement,
            coverage,
            scenarios,
            evidence,
            inspected_screenshots,
        )
        .is_some();
    }
    let headless_functional_requirement =
        request.relevant_platforms.as_slice() == [QaPlatform::HeadlessService];
    if !headless_functional_requirement {
        return has_inspected_screenshot_trace_and_assertion(
            coverage,
            evidence,
            inspected_screenshots,
        );
    }

    let cited_actions = evidence
        .actions
        .iter()
        .filter(|action| coverage.evidence.contains(&action.index.to_string()))
        .collect::<Vec<_>>();
    matches!(coverage.status, CoverageStatus::Pass)
        && cited_actions.iter().any(|action| {
            action.evidence_ids.iter().any(|id| {
                coverage.evidence.contains(id)
                    && evidence.assertions.iter().any(|assertion| {
                        assertion.status == QaAssertionStatus::Passed
                            && assertion.requirement_ids.contains(&coverage.requirement)
                            && assertion.scenario_ids.contains(&action.scenario_id)
                            && assertion.evidence_refs.contains(id)
                    })
            })
        })
}

pub(super) fn inspected_visual_screenshots(
    requirement: &str,
    coverage: &CoverageV1,
    scenarios: &[QaScenarioV1],
    evidence: &QaEvidenceManifestV1,
    inspected_screenshots: &BTreeSet<&str>,
) -> Option<std::collections::BTreeMap<String, Vec<String>>> {
    if coverage.status != CoverageStatus::Pass || coverage.requirement != requirement {
        return None;
    }
    let visual_scenarios = scenarios
        .iter()
        .filter(|scenario| {
            scenario.requirement_ids.iter().any(|id| id == requirement)
                && !scenario.visual_expectations.is_empty()
        })
        .collect::<Vec<_>>();
    let assertions = evidence
        .assertions
        .iter()
        .filter(|assertion| assertion.requirement_ids.iter().any(|id| id == requirement))
        .collect::<Vec<_>>();
    if visual_scenarios.is_empty()
        || assertions.is_empty()
        || assertions.iter().any(|assertion| {
            matches!(
                assertion.status,
                QaAssertionStatus::Failed | QaAssertionStatus::Blocked
            ) || (assertion.status == QaAssertionStatus::Untested
                && assertion.scenario_ids.iter().any(|scenario_id| {
                    !visual_scenarios
                        .iter()
                        .any(|scenario| scenario.id.as_str() == scenario_id.as_str())
                }))
        })
    {
        return None;
    }
    let mut reviewed = std::collections::BTreeMap::new();
    for scenario in visual_scenarios {
        let screenshots = evidence
            .screenshots
            .iter()
            .filter(|screenshot| {
                screenshot.scenario_id == scenario.id
                    && coverage.evidence.contains(&screenshot.id)
                    && inspected_screenshots.contains(screenshot.id.as_str())
                    && coverage
                        .evidence
                        .contains(&screenshot.source_action.to_string())
                    && evidence
                        .actions
                        .get(screenshot.source_action as usize)
                        .is_some_and(|action| action.scenario_id == scenario.id)
            })
            .map(|screenshot| screenshot.id.clone())
            .collect::<Vec<_>>();
        if screenshots.is_empty() {
            return None;
        }
        reviewed.insert(scenario.id.clone(), screenshots);
    }
    Some(reviewed)
}
