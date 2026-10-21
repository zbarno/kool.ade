use super::*;

pub(super) fn is_approved_verdict(
    verdict: &crate::harness::prompt_catalog::ReviewVerdictV1,
) -> bool {
    verdict.pass
        && verdict.findings.iter().all(|finding| {
            finding.scope == Some(crate::harness::prompt_catalog::FindingScopeV1::NonBlocking)
        })
}

pub(super) fn uses_current_prompt(receipt: &CodeReviewReceiptV1) -> bool {
    receipt.prompt.role_version == crate::harness::prompt_catalog::PromptRole::CodeReviewer.id()
}

pub(super) fn same_code_failed_review_requires_manager_resolution(
    receipt: &CodeReviewReceiptV1,
    base_sha: &str,
    head_sha: &str,
    diff_hash: &str,
    authorized_arbitration: bool,
) -> bool {
    !authorized_arbitration
        && receipt.base_sha == base_sha
        && receipt.head_sha == head_sha
        && receipt.diff_hash == diff_hash
        && !is_approved_verdict(&receipt.verdict)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CodeReviewReceiptV1 {
    pub(super) schema_version: u16,
    pub(super) task_uid: String,
    pub(super) feature_uid: String,
    pub(super) subject_hash: String,
    pub(super) diff_hash: String,
    pub(super) base_sha: String,
    pub(super) head_sha: String,
    pub(super) author_run_id: String,
    pub(super) author_session_id: String,
    pub(super) reviewer_run_id: String,
    pub(super) reviewer_session_id: String,
    pub(super) prompt: crate::harness::prompt_catalog::PromptManifest,
    pub(super) requirements: Vec<String>,
    #[serde(default)]
    pub(super) affected_paths: Vec<String>,
    pub(super) evidence_ids: Vec<String>,
    pub(super) verdict: crate::harness::prompt_catalog::ReviewVerdictV1,
}

pub(super) fn load_receipt(
    dir: &Path,
    task_uid: &str,
) -> anyhow::Result<Option<CodeReviewReceiptV1>> {
    let path = dir.join(RECEIPT_FILE);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "CodeReviewer receipt is not a regular file"
    );
    let receipt: CodeReviewReceiptV1 = serde_json::from_slice(&fs::read(path)?)?;
    anyhow::ensure!(
        receipt.task_uid == task_uid,
        "CodeReviewer receipt belongs to a different task"
    );
    validate_receipt(&receipt)?;
    Ok(Some(receipt))
}

pub(super) fn validate_receipt(receipt: &CodeReviewReceiptV1) -> anyhow::Result<()> {
    anyhow::ensure!(
        receipt.schema_version == 1
            && is_uuid(&receipt.author_run_id)
            && is_uuid(&receipt.author_session_id)
            && is_uuid(&receipt.reviewer_run_id)
            && is_uuid(&receipt.reviewer_session_id)
            && receipt.author_run_id != receipt.reviewer_run_id
            && receipt.author_session_id != receipt.reviewer_session_id,
        "CodeReviewer receipt identities are invalid or not independent"
    );
    let reviewer_role = crate::harness::prompt_catalog::validate_prompt_manifest(&receipt.prompt)
        .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        matches!(
            reviewer_role,
            crate::harness::prompt_catalog::PromptRole::CodeReviewer
                | crate::harness::prompt_catalog::PromptRole::CodeReviewerV1
        ),
        "CodeReviewer receipt prompt has the wrong reviewer role"
    );
    anyhow::ensure!(
        receipt.verdict.subject_hash == receipt.subject_hash,
        "CodeReviewer receipt is bound to another subject"
    );
    if reviewer_role == crate::harness::prompt_catalog::PromptRole::CodeReviewer {
        let mut affected_paths = receipt.affected_paths.clone();
        let mut examined_files = receipt.verdict.examined_files.clone();
        affected_paths.sort();
        examined_files.sort();
        anyhow::ensure!(
            affected_paths == examined_files,
            "CodeReviewer receipt does not enumerate every affected file"
        );
        anyhow::ensure!(
            receipt.verdict.findings.iter().all(|finding| {
                finding
                    .location_detail
                    .as_ref()
                    .is_some_and(|location| receipt.affected_paths.contains(&location.file))
            }),
            "CodeReviewer finding location is outside the frozen diff"
        );
    }
    let context = crate::harness::prompt_catalog::OutputValidationContext {
        expected_subject_hash: Some(&receipt.subject_hash),
        evidence_ids: receipt.evidence_ids.iter().cloned().collect(),
        required_requirements: receipt.requirements.clone(),
        ..Default::default()
    };
    crate::harness::prompt_catalog::validate_output_with_context(
        reviewer_role,
        &serde_json::to_string(&receipt.verdict)?,
        &context,
    )
    .map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        !receipt.verdict.pass
            || (receipt.verdict.findings.iter().all(|finding| !matches!(
                finding.severity,
                crate::harness::prompt_catalog::Severity::Blocker
                    | crate::harness::prompt_catalog::Severity::Critical
                    | crate::harness::prompt_catalog::Severity::High
            )) && receipt.verdict.coverage.iter().all(|coverage| matches!(
                coverage.status,
                crate::harness::prompt_catalog::CoverageStatus::Pass
            )) && receipt.verdict.untested.is_empty()),
        "CodeReviewer receipt claims pass with blocking findings or untested coverage"
    );
    anyhow::ensure!(
        !receipt.verdict.pass
            || reviewer_role == crate::harness::prompt_catalog::PromptRole::CodeReviewerV1
            || (receipt.verdict.findings.iter().all(|finding| {
                finding.scope == Some(crate::harness::prompt_catalog::FindingScopeV1::NonBlocking)
            }) && receipt.verdict.coverage.iter().all(|coverage| {
                coverage
                    .evidence
                    .iter()
                    .any(|id| id.starts_with("verified-test-report-"))
            })),
        "CodeReviewer pass receipt has blocking findings or omits test evidence"
    );
    if reviewer_role == crate::harness::prompt_catalog::PromptRole::CodeReviewer {
        let evidence_ids = receipt
            .evidence_ids
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        anyhow::ensure!(
            receipt
                .verdict
                .tested_evidence_ids
                .iter()
                .all(|id| evidence_ids.contains(id)),
            "CodeReviewer tested evidence references are not in the immutable receipt"
        );
        anyhow::ensure!(
            !receipt.verdict.pass
                || receipt
                    .verdict
                    .tested_evidence_ids
                    .iter()
                    .any(|id| id.starts_with("verified-test-report-")),
            "CodeReviewer pass receipt omits tested verification evidence"
        );
    }
    anyhow::ensure!(
        receipt.diff_hash.len() == 64 && receipt.subject_hash.len() == 64,
        "CodeReviewer receipt hashes are malformed"
    );
    Ok(())
}
