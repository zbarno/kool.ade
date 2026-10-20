use super::*;

pub(super) fn validate(
    decoded: &RoleOutputV1,
    role: PromptRole,
    context: &OutputValidationContext<'_>,
) -> Result<(), String> {
    if let RoleOutputV1::Review(review) = decoded {
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
        if let Some(expected) = context.expected_subject_hash
            && review.subject_hash != expected
        {
            return Err("review subject hash does not match the immutable subject".into());
        }
        if review.pass
            && review
                .findings
                .iter()
                .any(|finding| matches!(finding.severity, Severity::Blocker | Severity::High))
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
                && !review
                    .findings
                    .iter()
                    .any(|finding| finding.requirement == coverage.requirement)
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
