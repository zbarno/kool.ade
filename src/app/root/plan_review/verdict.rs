use crate::harness::prompt_catalog::{
    CoverageStatus, CoverageV1, ReviewOutcomeV1, ReviewVerdictV1,
};

pub(crate) fn catch_execution_panic<T>(
    execute: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(execute))
        .unwrap_or_else(|_| Err("independent reviewer execution panicked".into()))
}

pub(super) fn unable_verdict(
    subject_hash: &str,
    requirements: &[String],
    evidence_ref: &str,
) -> ReviewVerdictV1 {
    ReviewVerdictV1 {
        subject_hash: subject_hash.to_owned(),
        examined_files: Vec::new(),
        tested_evidence_ids: Vec::new(),
        unexamined_areas: Vec::new(),
        pass: false,
        outcome: Some(ReviewOutcomeV1::ReviewUnable),
        coverage: requirements
            .iter()
            .map(|requirement| CoverageV1 {
                requirement: requirement.clone(),
                status: CoverageStatus::Untested,
                evidence: vec![evidence_ref.to_owned()],
            })
            .collect(),
        findings: Vec::new(),
        untested: vec!["Reviewer execution or output validation did not complete.".into()],
    }
}

pub(super) fn sanitize_verdict(mut verdict: ReviewVerdictV1) -> ReviewVerdictV1 {
    for finding in &mut verdict.findings {
        finding.location = sanitize_text(&finding.location, 512);
        finding.repro = finding
            .repro
            .as_deref()
            .map(|value| sanitize_text(value, 2048));
        finding.proposed_correction = sanitize_text(&finding.proposed_correction, 4096);
    }
    verdict.untested = verdict
        .untested
        .iter()
        .map(|value| sanitize_text(value, 512))
        .collect();
    verdict
}

pub(crate) fn sanitize_text(value: &str, limit: usize) -> String {
    value
        .split_whitespace()
        .map(|word| {
            let normalized = word.replace('\\', "/").to_ascii_lowercase();
            if [
                "/home/",
                "/mnt/",
                "/users/",
                "/private/",
                "/tmp/",
                "c:/users/",
            ]
            .iter()
            .any(|prefix| normalized.contains(prefix))
            {
                "[local path]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}
