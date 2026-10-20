use super::load_receipt;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Clone, Debug)]
pub(crate) struct ManagerCodeReviewFindings {
    pub(crate) task_uid: String,
    pub(crate) subject_hash: String,
    pub(crate) diff_hash: String,
    pub(crate) head_sha: String,
    pub(crate) prompt_hash: String,
    pub(crate) reviewer_run_id: String,
    pub(crate) reviewer_session_id: String,
    pub(crate) pass: bool,
    pub(crate) outcome: Option<crate::harness::prompt_catalog::ReviewOutcomeV1>,
    pub(crate) coverage: Vec<crate::harness::prompt_catalog::CoverageV1>,
    pub(crate) untested: Vec<String>,
    pub(crate) examined_files: Vec<String>,
    pub(crate) tested_evidence_ids: Vec<String>,
    pub(crate) unexamined_areas: Vec<String>,
    pub(crate) findings: Vec<crate::harness::prompt_catalog::ReviewFindingV1>,
}

pub(crate) fn finding_fingerprint(
    finding: &crate::harness::prompt_catalog::ReviewFindingV1,
) -> String {
    let file = finding.location_detail.as_ref().map_or_else(
        || {
            finding
                .location
                .split_once(':')
                .map_or(finding.location.as_str(), |(file, _)| file)
                .trim()
        },
        |location| location.file.trim(),
    );
    let symptom = finding
        .repro
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .unwrap_or(&finding.proposed_correction)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let normalized = format!(
        "code-review-finding-v1\0{:?}\0{}\0{}\0{}",
        finding.kind,
        finding.requirement.trim().to_lowercase(),
        file.to_lowercase(),
        symptom
    );
    format!("{:x}", Sha256::digest(normalized.as_bytes()))
}

/// Load findings from the validated receipt for the task's current verified head.
pub(crate) fn manager_findings_for_task(
    repo: &Path,
    ticket: &str,
    task_uid: &str,
) -> anyhow::Result<Option<ManagerCodeReviewFindings>> {
    let dir = super::super::state_paths::state_dir_for_task(repo, ticket, Some(task_uid))?;
    if !dir.join("state.json").is_file() {
        return Ok(None);
    }
    let state = super::super::read_state_file(&dir.join("state.json"))?;
    anyhow::ensure!(
        state.task_uid.as_deref() == Some(task_uid),
        "implementation record has a different task identity"
    );
    let Some(receipt) = load_receipt(&dir, task_uid)? else {
        return Ok(None);
    };
    if state.verified_head.as_deref() != Some(receipt.head_sha.as_str()) {
        return Ok(None);
    }
    Ok(Some(ManagerCodeReviewFindings {
        task_uid: receipt.task_uid,
        subject_hash: receipt.subject_hash,
        diff_hash: receipt.diff_hash,
        head_sha: receipt.head_sha,
        prompt_hash: receipt.prompt.identity_sha256(),
        reviewer_run_id: receipt.reviewer_run_id,
        reviewer_session_id: receipt.reviewer_session_id,
        pass: receipt.verdict.pass,
        outcome: receipt.verdict.outcome,
        coverage: receipt.verdict.coverage,
        untested: receipt.verdict.untested,
        examined_files: receipt.verdict.examined_files,
        tested_evidence_ids: receipt.verdict.tested_evidence_ids,
        unexamined_areas: receipt.verdict.unexamined_areas,
        findings: receipt.verdict.findings,
    }))
}
