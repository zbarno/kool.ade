use super::super::super::{
    QaAssertionStatus, QaEvidenceManifestV1, QaPlatform, QaRunRequestV1, QaScenarioV1,
};
use super::super::{QaResultV1, QaReviewCoverageStatusV1};

pub(super) fn validate_review_coverage(
    result: &QaResultV1,
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    manifest: &QaEvidenceManifestV1,
    inspected_screenshots: &std::collections::BTreeSet<&str>,
) -> Result<(), String> {
    if result.review_coverage.is_empty() {
        return Ok(());
    }
    let requirement_ids = result
        .review_coverage
        .iter()
        .map(|item| item.requirement_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if requirement_ids.len() != result.review_coverage.len()
        || result.review_coverage.len() != request.requirement_ids.len()
        || request
            .requirement_ids
            .iter()
            .any(|id| !requirement_ids.contains(id.as_str()))
    {
        return Err(
            "runtime QA reviewer coverage does not bind every requirement exactly once".into(),
        );
    }
    for coverage in &result.review_coverage {
        if coverage.evidence_refs.is_empty()
            || coverage.evidence_refs.iter().any(|reference| {
                !manifest
                    .screenshots
                    .iter()
                    .any(|screenshot| screenshot.id == *reference)
                    && !manifest.actions.iter().any(|action| {
                        action.index.to_string() == *reference
                            || action.evidence_ids.contains(reference)
                    })
            })
        {
            return Err("runtime QA reviewer coverage cites missing runtime evidence".into());
        }
        if coverage.status == QaReviewCoverageStatusV1::Pass {
            let visual_requirement = scenarios.iter().any(|scenario| {
                scenario.requirement_ids.contains(&coverage.requirement_id)
                    && !scenario.visual_expectations.is_empty()
            });
            let headless_functional_requirement = request.relevant_platforms.as_slice()
                == [QaPlatform::HeadlessService]
                && !visual_requirement;
            if headless_functional_requirement {
                let grounded_action = manifest.actions.iter().any(|action| {
                    coverage.evidence_refs.contains(&action.index.to_string())
                        && action.evidence_ids.iter().any(|id| {
                            coverage.evidence_refs.contains(id)
                                && manifest.assertions.iter().any(|assertion| {
                                    assertion.requirement_ids.contains(&coverage.requirement_id)
                                        && assertion.status == QaAssertionStatus::Passed
                                        && assertion.scenario_ids.contains(&action.scenario_id)
                                        && assertion.evidence_refs.contains(id)
                                })
                        })
                });
                if !grounded_action {
                    return Err("passed headless runtime QA review lacks action evidence or a matching assertion".into());
                }
                continue;
            }
            let reviewed_images = coverage
                .evidence_refs
                .iter()
                .filter_map(|reference| {
                    manifest
                        .screenshots
                        .iter()
                        .find(|screenshot| screenshot.id == *reference)
                })
                .filter(|screenshot| inspected_screenshots.contains(screenshot.id.as_str()))
                .collect::<Vec<_>>();
            let matched_scenario = reviewed_images.iter().any(|screenshot| {
                manifest.actions.iter().any(|action| {
                    action.scenario_id == screenshot.scenario_id
                        && coverage.evidence_refs.contains(&action.index.to_string())
                        && manifest.assertions.iter().any(|assertion| {
                            assertion.requirement_ids.contains(&coverage.requirement_id)
                                && assertion.status == QaAssertionStatus::Passed
                                && assertion.scenario_ids.contains(&action.scenario_id)
                                && assertion
                                    .evidence_refs
                                    .iter()
                                    .any(|reference| coverage.evidence_refs.contains(reference))
                        })
                })
            });
            if !matched_scenario {
                return Err("passed runtime QA review lacks an inspected screenshot, observed action, or matching assertion".into());
            }
        }
    }
    Ok(())
}
