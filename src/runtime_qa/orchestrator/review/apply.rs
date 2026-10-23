use super::{
    coverage::has_grounded_requirement_evidence,
    findings::append_review_finding,
    rejection_finding::rejection_finding,
    visual_coverage::{record_visual_inspection_status, resolve_visual_only_coverage},
};
use crate::runtime_qa::{
    QaEvidenceManifestV1, QaOutcome, QaResultV1, QaRunRequestV1, QaScenarioV1,
};

pub(in crate::runtime_qa::orchestrator) fn apply_review_verdict(
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    evidence: &mut QaEvidenceManifestV1,
    result: &mut QaResultV1,
    verdict: crate::harness::prompt_catalog::ReviewVerdictV1,
) -> Result<(), String> {
    let mut staged_evidence = evidence.clone();
    let mut staged_result = result.clone();
    apply_review_verdict_staged(
        request,
        scenarios,
        &mut staged_evidence,
        &mut staged_result,
        verdict,
    )?;
    *evidence = staged_evidence;
    *result = staged_result;
    Ok(())
}

fn apply_review_verdict_staged(
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    evidence: &mut QaEvidenceManifestV1,
    result: &mut QaResultV1,
    verdict: crate::harness::prompt_catalog::ReviewVerdictV1,
) -> Result<(), String> {
    let expected_hash = crate::runtime_qa::qa_review_subject_sha256(request, result, evidence)?;
    let inspected_screenshots = verdict
        .tested_evidence_ids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    if inspected_screenshots.len() != verdict.tested_evidence_ids.len()
        || verdict.tested_evidence_ids.iter().any(|id| {
            !evidence
                .screenshots
                .iter()
                .any(|screenshot| &screenshot.id == id)
        })
    {
        return Err(
            "RuntimeQaReviewer image inspection audit contains an unknown screenshot".into(),
        );
    }
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
                        && !evidence.actions.iter().any(|action| {
                            action.index.to_string() == *reference
                                || action.evidence_ids.contains(reference)
                        })
                })
        })
    {
        return Err(
            "RuntimeQaReviewer verdict does not bind exact requirements and observed evidence"
                .into(),
        );
    }
    resolve_visual_only_coverage(
        scenarios,
        evidence,
        result,
        &verdict,
        &inspected_screenshots,
    )?;
    let ungrounded_passes = verdict
        .coverage
        .iter()
        .filter(|coverage| {
            matches!(
                &coverage.status,
                crate::harness::prompt_catalog::CoverageStatus::Pass
            ) && !has_grounded_requirement_evidence(
                request,
                scenarios,
                coverage,
                evidence,
                &inspected_screenshots,
            )
        })
        .map(|coverage| coverage.requirement.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let reviewer_passes = verdict.pass
        && verdict.findings.is_empty()
        && verdict.untested.is_empty()
        && verdict.coverage.iter().all(|coverage| {
            matches!(
                &coverage.status,
                crate::harness::prompt_catalog::CoverageStatus::Pass
            )
        })
        && ungrounded_passes.is_empty();
    result.reviewed_screenshot_ids = verdict.tested_evidence_ids.clone();
    result.review_coverage = verdict
        .coverage
        .iter()
        .map(
            |coverage| crate::runtime_qa::QaReviewRequirementCoverageV1 {
                requirement_id: coverage.requirement.clone(),
                status: if ungrounded_passes.contains(&coverage.requirement) {
                    crate::runtime_qa::QaReviewCoverageStatusV1::Untested
                } else {
                    match &coverage.status {
                        crate::harness::prompt_catalog::CoverageStatus::Pass => {
                            crate::runtime_qa::QaReviewCoverageStatusV1::Pass
                        }
                        crate::harness::prompt_catalog::CoverageStatus::Fail => {
                            crate::runtime_qa::QaReviewCoverageStatusV1::Fail
                        }
                        crate::harness::prompt_catalog::CoverageStatus::Untested => {
                            crate::runtime_qa::QaReviewCoverageStatusV1::Untested
                        }
                    }
                },
                evidence_refs: coverage.evidence.clone(),
            },
        )
        .collect();
    let reviewer_completed_incomplete_run = reviewer_passes
        && result
            .cases
            .iter()
            .all(|case| case.status == crate::runtime_qa::QaCaseStatus::Passed)
        && result
            .coverage
            .requirements
            .iter()
            .all(|coverage| coverage.status == crate::runtime_qa::QaCoverageStatus::Observed)
        && evidence
            .assertions
            .iter()
            .all(|assertion| assertion.status == crate::runtime_qa::QaAssertionStatus::Passed)
        && result.findings.iter().all(|finding| !finding.blocking)
        && request.requirement_ids.iter().all(|requirement| {
            evidence.assertions.iter().any(|assertion| {
                assertion.status == crate::runtime_qa::QaAssertionStatus::Passed
                    && assertion.requirement_ids.contains(requirement)
            })
        });
    result.outcome = match (
        result.outcome,
        reviewer_passes,
        reviewer_completed_incomplete_run,
    ) {
        (QaOutcome::InfrastructureBlocked, _, _) => QaOutcome::InfrastructureBlocked,
        (QaOutcome::Incomplete, true, true) => QaOutcome::Pass,
        (QaOutcome::Incomplete, _, _) => QaOutcome::Incomplete,
        (QaOutcome::Pass, true, _) => QaOutcome::Pass,
        (QaOutcome::Pass, false, _) | (QaOutcome::Fail, _, _) => QaOutcome::Fail,
    };
    let mut rejected_requirements = verdict
        .coverage
        .iter()
        .filter(|coverage| {
            !matches!(&coverage.status, crate::harness::prompt_catalog::CoverageStatus::Pass)
                || ungrounded_passes.contains(&coverage.requirement)
        })
        .map(|coverage| (coverage.requirement.clone(), "independent reviewer could not verify the acceptance criterion from required runtime evidence".to_owned()))
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
    let detailed_finding_requirements = verdict
        .findings
        .iter()
        .map(|finding| finding.requirement.clone())
        .collect::<std::collections::BTreeSet<_>>();
    for (requirement, actual) in rejected_requirements {
        if detailed_finding_requirements.contains(&requirement)
            || result
                .findings
                .iter()
                .any(|finding| finding.blocking && finding.requirement_uids.contains(&requirement))
        {
            continue;
        }
        result.findings.push(rejection_finding(
            request,
            scenarios,
            evidence,
            &requirement,
            actual,
        )?);
    }
    for finding in verdict.findings {
        append_review_finding(request, scenarios, evidence, result, finding)?;
    }
    record_visual_inspection_status(scenarios, evidence, result);
    result.evidence_manifest_sha256 = evidence.sha256()?;
    Ok(())
}
