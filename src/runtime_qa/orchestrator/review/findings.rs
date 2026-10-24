use crate::runtime_qa::{
    QaEvidenceManifestV1, QaFindingCategory, QaFindingSeverity, QaFindingV1, QaResultV1,
    QaRunRequestV1, QaScenarioV1,
};

pub(super) fn append_review_finding(
    request: &QaRunRequestV1,
    scenarios: &[QaScenarioV1],
    evidence: &QaEvidenceManifestV1,
    result: &mut QaResultV1,
    finding: crate::harness::prompt_catalog::ReviewFindingV1,
) -> Result<(), String> {
    let scenario = scenarios
        .iter()
        .find(|scenario| {
            let has_requirement = evidence.assertions.iter().any(|assertion| {
                assertion.scenario_ids.contains(&scenario.id)
                    && assertion.requirement_ids.contains(&finding.requirement)
            });
            let has_cited_evidence = finding.evidence.iter().any(|reference| {
                evidence.screenshots.iter().any(|screenshot| {
                    screenshot.scenario_id == scenario.id && screenshot.id == *reference
                }) || evidence.actions.iter().any(|action| {
                    action.scenario_id == scenario.id
                        && (action.index.to_string() == *reference
                            || action.evidence_ids.contains(reference))
                })
            });
            has_requirement && has_cited_evidence
        })
        .ok_or_else(|| {
            "RuntimeQaReviewer finding does not map to an observed scenario".to_owned()
        })?;
    let category = match finding.kind {
        crate::harness::prompt_catalog::FindingKind::Security => QaFindingCategory::Security,
        crate::harness::prompt_catalog::FindingKind::Regression => QaFindingCategory::Regression,
        crate::harness::prompt_catalog::FindingKind::TestGap => QaFindingCategory::Environment,
        _ => QaFindingCategory::Functional,
    };
    let severity = match finding.severity {
        crate::harness::prompt_catalog::Severity::Critical
        | crate::harness::prompt_catalog::Severity::Blocker => QaFindingSeverity::Critical,
        crate::harness::prompt_catalog::Severity::High => QaFindingSeverity::High,
        crate::harness::prompt_catalog::Severity::Medium => QaFindingSeverity::Medium,
        crate::harness::prompt_catalog::Severity::Low
        | crate::harness::prompt_catalog::Severity::Informational => QaFindingSeverity::Low,
    };
    let smallest_fix = finding.proposed_correction.clone();
    let reviewer_finding = QaFindingV1 {
        finding_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        code_sha: request.code_head_sha.clone(),
        scenario_uid: scenario.id.clone(),
        requirement_uids: vec![finding.requirement.clone()],
        category,
        severity,
        expected: "The approved acceptance criterion is satisfied.".into(),
        actual: smallest_fix.clone(),
        reproducible_steps: finding
            .repro
            .clone()
            .or_else(|| Some(finding.location.clone()))
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
    };

    let repeats_failed_assertion = finding.kind
        == crate::harness::prompt_catalog::FindingKind::Correctness
        && evidence.assertions.iter().any(|assertion| {
            assertion.status == crate::runtime_qa::QaAssertionStatus::Failed
                && assertion.scenario_ids.contains(&scenario.id)
                && assertion.requirement_ids.contains(&finding.requirement)
        });
    if repeats_failed_assertion
        && let Some(existing) = result.findings.iter_mut().find(|existing| {
            existing.blocking
                && existing.category == QaFindingCategory::Functional
                && existing.scenario_uid == scenario.id
                && existing.requirement_uids.contains(&finding.requirement)
        })
    {
        existing.severity = more_severe(existing.severity, reviewer_finding.severity);
        existing.smallest_fix = reviewer_finding.smallest_fix;
        for step in reviewer_finding.reproducible_steps {
            if existing.reproducible_steps.len() < 128
                && !existing.reproducible_steps.contains(&step)
            {
                existing.reproducible_steps.push(step);
            }
        }
        return Ok(());
    }

    result.findings.push(reviewer_finding);
    Ok(())
}

fn more_severe(left: QaFindingSeverity, right: QaFindingSeverity) -> QaFindingSeverity {
    if severity_rank(right) > severity_rank(left) {
        right
    } else {
        left
    }
}

fn severity_rank(severity: QaFindingSeverity) -> u8 {
    match severity {
        QaFindingSeverity::Critical => 4,
        QaFindingSeverity::High => 3,
        QaFindingSeverity::Medium => 2,
        QaFindingSeverity::Low => 1,
        QaFindingSeverity::Informational => 0,
    }
}
