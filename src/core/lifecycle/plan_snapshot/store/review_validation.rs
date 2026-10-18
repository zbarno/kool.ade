use crate::core::lifecycle::{GateActor, GateReceipt, PlanSnapshotV1};
use crate::harness::prompt_catalog::{CoverageStatus, ReviewVerdictV1, Severity};
use std::collections::HashSet;

pub(super) fn validate_verdict(
    snapshot: &PlanSnapshotV1,
    receipt: &GateReceipt,
    verdict: &ReviewVerdictV1,
) -> Result<(), String> {
    let expected = snapshot
        .requirements
        .iter()
        .map(|requirement| requirement.stable_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let actual = verdict
        .coverage
        .iter()
        .map(|coverage| coverage.requirement.as_str())
        .collect::<std::collections::HashSet<_>>();
    let finding_ids = verdict
        .findings
        .iter()
        .map(|finding| finding.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let allowed_evidence = receipt.evidence_refs.iter().collect::<HashSet<_>>();
    if verdict.subject_hash != snapshot.content_hash
        || actual != expected
        || actual.len() != verdict.coverage.len()
        || finding_ids.len() != verdict.findings.len()
        || verdict.findings.iter().any(|item| {
            item.id.trim().is_empty()
                || item.location.trim().is_empty()
                || item
                    .repro
                    .as_deref()
                    .is_none_or(|repro| repro.trim().is_empty())
        })
        || (!verdict.pass && verdict.findings.is_empty())
        || (verdict.pass
            && (!verdict.findings.is_empty()
                || !verdict.untested.is_empty()
                || verdict
                    .coverage
                    .iter()
                    .any(|item| !matches!(item.status, CoverageStatus::Pass))))
        || !receipt
            .evidence_refs
            .contains(&format!("plan:{}", snapshot.uid))
        || verdict.coverage.iter().any(|item| {
            item.evidence.is_empty()
                || !item
                    .evidence
                    .contains(&format!("requirement:{}", item.requirement))
                || item
                    .evidence
                    .iter()
                    .any(|reference| !allowed_evidence.contains(reference))
                || (matches!(item.status, CoverageStatus::Fail)
                    && !verdict
                        .findings
                        .iter()
                        .any(|finding| finding.requirement == item.requirement))
        })
        || verdict.findings.iter().any(|item| {
            item.evidence.is_empty()
                || !item
                    .evidence
                    .contains(&format!("requirement:{}", item.requirement))
                || item.proposed_correction.trim().is_empty()
                || !expected.contains(item.requirement.as_str())
                || item
                    .evidence
                    .iter()
                    .any(|reference| !allowed_evidence.contains(reference))
                || (verdict.pass && matches!(item.severity, Severity::Blocker | Severity::High))
        })
    {
        return Err("plan review verdict is incomplete or inconsistent with its snapshot".into());
    }
    Ok(())
}

pub(super) fn validate_plan_review(
    snapshot: &PlanSnapshotV1,
    receipt: &GateReceipt,
) -> Result<(), String> {
    receipt.validate()?;
    if receipt.actor != GateActor::IndependentReviewer
        || receipt.subject_uid != snapshot.uid
        || receipt.subject_sha256 != snapshot.content_hash
        || receipt.prompt_role() != Some(crate::harness::prompt_catalog::PromptRole::PlanReviewer)
        || receipt.author_run_id.as_deref() != snapshot.author_run_id.as_deref()
        || receipt.author_session_id.as_deref() != snapshot.author_session_id.as_deref()
        || !receipt
            .evidence_refs
            .contains(&format!("plan:{}", snapshot.uid))
    {
        return Err(
            "plan review must be an independent PlanReviewer receipt for this exact snapshot"
                .into(),
        );
    }
    Ok(())
}
