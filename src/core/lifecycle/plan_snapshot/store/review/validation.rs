use crate::core::lifecycle::{GateDecision, GateReceipt};

use super::super::PlanSnapshotV1;

pub(super) fn validate_verdict(
    snapshot: &PlanSnapshotV1,
    receipt: &GateReceipt,
    verdict: &crate::harness::prompt_catalog::ReviewVerdictV1,
) -> Result<(), String> {
    use crate::harness::prompt_catalog::{CoverageStatus, ReviewOutcomeV1};

    if verdict.subject_hash != snapshot.content_hash {
        return Err("persisted plan review verdict refers to a different snapshot".into());
    }
    if verdict.pass != (receipt.decision == GateDecision::Approved) {
        return Err("plan review verdict and gate receipt disagree".into());
    }
    match verdict.outcome {
        Some(ReviewOutcomeV1::Pass) if verdict.pass && verdict.findings.is_empty() => {}
        Some(ReviewOutcomeV1::BlockingFindings)
            if !verdict.pass && !verdict.findings.is_empty() => {}
        Some(ReviewOutcomeV1::ReviewUnable)
            if !verdict.pass
                && verdict.findings.is_empty()
                && !verdict.untested.is_empty()
                && verdict
                    .coverage
                    .iter()
                    .all(|item| matches!(item.status, CoverageStatus::Untested)) => {}
        _ => return Err("persisted plan review outcome is missing or inconsistent".into()),
    }
    if verdict.pass
        && (!verdict.findings.is_empty()
            || !verdict.untested.is_empty()
            || verdict
                .coverage
                .iter()
                .any(|item| !matches!(item.status, CoverageStatus::Pass)))
    {
        return Err(
            "passing plan review cannot contain findings, failed coverage, or untested items"
                .into(),
        );
    }
    let requirement_ids = snapshot
        .requirements
        .iter()
        .map(|requirement| requirement.stable_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let finding_ids = verdict
        .findings
        .iter()
        .map(|finding| finding.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    if finding_ids.len() != verdict.findings.len()
        || verdict.findings.iter().any(|finding| {
            finding.id.trim().is_empty()
                || !requirement_ids.contains(finding.requirement.as_str())
                || finding.location.trim().is_empty()
                || finding.evidence.is_empty()
                || finding
                    .evidence
                    .iter()
                    .any(|reference| !receipt.evidence_refs.contains(reference))
                || finding.repro.as_deref().is_none_or(str::is_empty)
                || finding.proposed_correction.trim().is_empty()
        })
    {
        return Err("persisted plan review finding is not unique and actionable".into());
    }
    let expected: std::collections::HashSet<_> = snapshot
        .requirements
        .iter()
        .map(|requirement| requirement.stable_id.as_str())
        .collect();
    let covered: std::collections::HashSet<_> = verdict
        .coverage
        .iter()
        .map(|item| item.requirement.as_str())
        .collect();
    if covered.len() != verdict.coverage.len()
        || covered != expected
        || verdict.coverage.iter().any(|item| {
            (matches!(item.status, CoverageStatus::Pass) && item.evidence.is_empty())
                || item
                    .evidence
                    .iter()
                    .any(|reference| !receipt.evidence_refs.contains(reference))
        })
    {
        return Err(
            "persisted plan review must cover each snapshot requirement exactly once".into(),
        );
    }
    Ok(())
}
