use super::*;

pub(super) fn validate(
    decoded: &RoleOutputV1,
    role: PromptRole,
    context: &OutputValidationContext<'_>,
) -> Result<(), String> {
    if let RoleOutputV1::Review(review) = decoded {
        if let Some(expected) = context.expected_subject_hash
            && review.subject_hash != expected
        {
            return Err("review subject hash does not match the immutable subject".into());
        }
        if role == PromptRole::CodeReviewer {
            if review.findings.len() > 64
                || review.untested.len() > 64
                || review.examined_files.len() > 2048
                || review.tested_evidence_ids.len() > 256
                || review.unexamined_areas.len() > 64
            {
                return Err("code review exceeds a bounded findings or coverage limit".into());
            }
            let mut examined_files = HashSet::new();
            if review.examined_files.iter().any(|path| {
                path.trim().is_empty()
                    || path.len() > 1024
                    || path.starts_with('/')
                    || path.contains('\\')
                    || path.split('/').any(|part| part == "..")
                    || !examined_files.insert(path.as_str())
            }) {
                return Err("code review examined files are invalid or duplicated".into());
            }
            let mut tested = HashSet::new();
            if review
                .tested_evidence_ids
                .iter()
                .any(|id| id.trim().is_empty() || !tested.insert(id.as_str()))
            {
                return Err("code review tested evidence IDs are invalid or duplicated".into());
            }
            if review
                .unexamined_areas
                .iter()
                .any(|area| area.trim().is_empty() || area.len() > 2048)
            {
                return Err("code review unexamined-area entries are invalid".into());
            }
            validate_refs(&review.tested_evidence_ids, &context.evidence_ids)?;
            if review.pass && !review.unexamined_areas.is_empty() {
                return Err("CodeReviewer cannot pass with unexamined areas".into());
            }
            if review.pass
                && !review
                    .tested_evidence_ids
                    .iter()
                    .any(|id| id.starts_with("verified-test-report-"))
            {
                return Err("CodeReviewer cannot pass without testing verified evidence".into());
            }
            let consistent_outcome = match review.outcome {
                Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass) => review.pass,
                Some(crate::harness::prompt_catalog::ReviewOutcomeV1::BlockingFindings) => {
                    !review.pass
                        && review.findings.iter().any(|finding| {
                            finding.scope
                                == Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking)
                        })
                }
                Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable) => {
                    !review.pass
                        && review.findings.is_empty()
                        && (!review.untested.is_empty() || !review.unexamined_areas.is_empty())
                }
                None => false,
            };
            if !consistent_outcome {
                return Err(
                    "CodeReviewer outcome is missing or inconsistent with its verdict".into(),
                );
            }
            let mut finding_ids = HashSet::new();
            for finding in &review.findings {
                if matches!(
                    finding.kind,
                    crate::harness::prompt_catalog::FindingKind::Requirements
                        | crate::harness::prompt_catalog::FindingKind::TestGap
                        | crate::harness::prompt_catalog::FindingKind::Scope
                ) {
                    return Err(
                        "code review finding category is not supported by the current contract"
                            .into(),
                    );
                }
                let Some(location) = finding.location_detail.as_ref() else {
                    return Err("code review finding requires a structured file location".into());
                };
                let mut affected_requirements = HashSet::new();
                if finding.id.trim().is_empty()
                    || !finding_ids.insert(finding.id.as_str())
                    || finding.location.trim().is_empty()
                    || location.file.trim().is_empty()
                    || location.file.len() > 1024
                    || location.file.starts_with('/')
                    || location.file.contains('\\')
                    || location.file.split('/').any(|part| part == "..")
                    || location.line_start == Some(0)
                    || location.line_end == Some(0)
                    || matches!((location.line_start, location.line_end), (Some(start), Some(end)) if end < start)
                    || finding.proposed_correction.trim().is_empty()
                    || finding.rationale.trim().len() < 16
                    || finding.rationale.len() > 4096
                    || finding.affected_requirement_ids.is_empty()
                    || finding.affected_requirement_ids.iter().any(|id| {
                        id.trim().is_empty() || !affected_requirements.insert(id.as_str())
                    })
                    || !finding
                        .affected_requirement_ids
                        .contains(&finding.requirement)
                    || finding.suggested_action.is_none()
                    || finding.scope.is_none()
                    || !matches!(
                        (finding.scope, finding.suggested_action),
                        (
                            Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking),
                            Some(
                                crate::harness::prompt_catalog::ReviewFindingActionV1::Repair
                                    | crate::harness::prompt_catalog::ReviewFindingActionV1::RePlan
                            )
                        ) | (
                            Some(crate::harness::prompt_catalog::FindingScopeV1::NonBlocking),
                            Some(
                                crate::harness::prompt_catalog::ReviewFindingActionV1::FollowUp
                                    | crate::harness::prompt_catalog::ReviewFindingActionV1::AcceptRisk
                            )
                        )
                    )
                    || (matches!(
                        finding.severity,
                        Severity::Critical | Severity::Blocker | Severity::High
                    ) && finding.scope
                        != Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking))
                    || (!context.required_requirements.is_empty()
                        && (finding
                            .affected_requirement_ids
                            .iter()
                            .any(|id| !context.required_requirements.contains(id))
                            || !context.required_requirements.contains(&finding.requirement)))
                {
                    return Err(
                        "code review findings must be unique, actionable, and tied to a requirement".into(),
                    );
                }
            }
            if review.pass
                && review.findings.iter().any(|finding| {
                    finding.scope == Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking)
                })
            {
                return Err("CodeReviewer cannot pass with blocking findings".into());
            }
        }
        if role == PromptRole::PlanReviewer {
            match review.outcome {
                Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass)
                    if review.pass && review.findings.is_empty() => {}
                Some(crate::harness::prompt_catalog::ReviewOutcomeV1::BlockingFindings)
                    if !review.pass && !review.findings.is_empty() => {}
                Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable)
                    if !review.pass
                        && review.findings.is_empty()
                        && !review.untested.is_empty() => {}
                _ => {
                    return Err(
                        "plan review outcome is missing, inconsistent, or lacks evidence".into(),
                    );
                }
            }
            if review.findings.len() > 64 || review.untested.len() > 64 {
                return Err("plan review exceeds the bounded findings or untested limit".into());
            }
            let mut finding_ids = HashSet::new();
            for finding in &review.findings {
                if finding.id.trim().is_empty()
                    || !finding_ids.insert(finding.id.as_str())
                    || finding.location.trim().is_empty()
                    || finding.repro.as_deref().is_none_or(str::is_empty)
                    || finding.proposed_correction.trim().is_empty()
                    || (!context.required_requirements.is_empty()
                        && !context.required_requirements.contains(&finding.requirement))
                {
                    return Err(
                        "plan review findings must be unique, actionable, and tied to a requirement".into(),
                    );
                }
            }
            if review.outcome == Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable)
                && review
                    .coverage
                    .iter()
                    .any(|item| !matches!(item.status, CoverageStatus::Untested))
            {
                return Err("ReviewUnable must leave every requirement untested".into());
            }
        }
        if review.pass
            && review.findings.iter().any(|finding| {
                matches!(
                    finding.severity,
                    Severity::Critical | Severity::Blocker | Severity::High
                )
            })
        {
            return Err("review cannot pass with blocking or high-severity findings".into());
        }
        if review.pass
            && (review
                .coverage
                .iter()
                .any(|coverage| !matches!(coverage.status, CoverageStatus::Pass))
                || !review.untested.is_empty())
        {
            return Err("review cannot pass with failed or untested requirements".into());
        }
        let actual: HashSet<_> = review
            .coverage
            .iter()
            .map(|coverage| coverage.requirement.as_str())
            .collect();
        if actual.len() != review.coverage.len()
            || context
                .required_requirements
                .iter()
                .any(|requirement| !actual.contains(requirement.as_str()))
        {
            return Err("review coverage is duplicate or omits a required acceptance check".into());
        }
        if !context.required_requirements.is_empty()
            && review.coverage.iter().any(|coverage| {
                !context
                    .required_requirements
                    .contains(&coverage.requirement)
            })
        {
            return Err(
                "review coverage includes a requirement outside the approved matrix".into(),
            );
        }
        for coverage in &review.coverage {
            if matches!(coverage.status, CoverageStatus::Pass) && coverage.evidence.is_empty() {
                return Err("passed review coverage requires cited evidence".into());
            }
            if matches!(coverage.status, CoverageStatus::Fail)
                && !review.findings.iter().any(|finding| {
                    finding.requirement == coverage.requirement
                        && (role != PromptRole::CodeReviewer
                            || finding.scope
                                == Some(crate::harness::prompt_catalog::FindingScopeV1::Blocking))
                })
            {
                return Err(
                    "failed coverage requires a traceable finding; zero-findings review rejected"
                        .into(),
                );
            }
            validate_refs(&coverage.evidence, &context.evidence_ids)?;
        }
        for finding in &review.findings {
            if finding.evidence.is_empty() {
                return Err("review findings require cited evidence".into());
            }
            validate_refs(&finding.evidence, &context.evidence_ids)?;
        }
    }
    Ok(())
}
