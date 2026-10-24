use super::{
    QaAssertionStatus, QaCoverageStatus, QaEvidenceManifestV1, QaOutcome, QaResultV1, QaScenarioV1,
    VISUAL_REVIEW_PENDING_LOG,
};

/// Runtime drivers record screenshots but cannot inspect pixels. Leave visual
/// criteria untested until the independent reviewer binds them to images.
pub(super) fn mark_unreviewed_visual_expectations(
    scenarios: &[QaScenarioV1],
    evidence: &mut QaEvidenceManifestV1,
    result: &mut QaResultV1,
) -> Result<(), String> {
    let requirements = scenarios
        .iter()
        .filter(|scenario| !scenario.visual_expectations.is_empty())
        .flat_map(|scenario| scenario.requirement_ids.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    if requirements.is_empty() {
        return Ok(());
    }
    for coverage in &mut result.coverage.requirements {
        if requirements.contains(&coverage.requirement_id) {
            coverage.status = QaCoverageStatus::Untested;
            coverage.evidence_refs.clear();
        }
    }
    for assertion in &mut evidence.assertions {
        if assertion.status == QaAssertionStatus::Passed
            && assertion
                .requirement_ids
                .iter()
                .any(|requirement| requirements.contains(requirement))
        {
            assertion.status = QaAssertionStatus::Untested;
            assertion.evidence_refs.clear();
        }
    }
    evidence.coverage = result
        .coverage
        .requirements
        .iter()
        .map(|coverage| format!("{}:{:?}", coverage.requirement_id, coverage.status))
        .collect();
    evidence.logs.truncate(255);
    evidence.logs.push(VISUAL_REVIEW_PENDING_LOG.into());
    if result.outcome == QaOutcome::Pass {
        result.outcome = QaOutcome::Incomplete;
    }
    result.evidence_manifest_sha256 = evidence.sha256()?;
    Ok(())
}
