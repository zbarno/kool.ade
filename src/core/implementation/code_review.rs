use super::{Implementation, Runner};
use crate::harness::{AiHarness, ExecutionMode, PlanningRequest, ReviewExecutionIdentity};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool, mpsc},
};

const MAX_DIFF_BYTES: u64 = 48 * 1024;
const MAX_EVIDENCE_ITEM: usize = 12 * 1024;
const RECEIPT_FILE: &str = "code-review-v1.json";

#[derive(Serialize)]
struct ReviewSubject<'a> {
    task_uid: &'a str,
    feature_uid: &'a str,
    approved_plan_hash: &'a str,
    task_spec_revision: &'a str,
    task_clone_head_sha: &'a str,
    base_sha: &'a str,
    full_diff_hash: &'a str,
    affected_paths: &'a [String],
    requirement_matrix: &'a [String],
    test_execution_receipts: &'a [&'a str],
}

pub(super) fn review_verified(
    dir: &Path,
    state: &Implementation,
    harness: &dyn AiHarness,
    runner: &Runner,
) -> anyhow::Result<()> {
    let head = state.verified_head.as_deref().ok_or_else(|| {
        anyhow::anyhow!("Independent code review is blocked: verified task head is missing")
    })?;
    let task_uid = state.task_uid.as_deref().ok_or_else(|| {
        anyhow::anyhow!("Independent code review is blocked: stable task identity is missing")
    })?;
    let feature_uid = state
        .ticket_text
        .lines()
        .find_map(|line| line.strip_prefix("Feature: "))
        .and_then(|value| {
            crate::core::workflow::feature_ids_in(value)
                .into_iter()
                .next()
        })
        .ok_or_else(|| {
            anyhow::anyhow!("Independent code review is blocked: feature identity is missing")
        })?;
    let spec = state
        .approved_specification
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Independent code review is blocked: approved task specification is missing"
            )
        })?;
    let author = harness.current_execution_identity().ok_or_else(|| {
        anyhow::anyhow!("Independent code review is blocked: controller author run/session receipt is unavailable")
    })?;
    ensure_identity(&author, "author")?;
    anyhow::ensure!(
        git_head(runner, &state.task_repository)? == head,
        "Independent code review is blocked: task clone head changed after verification"
    );
    anyhow::ensure!(
        runner
            .git(&state.task_repository, &["status", "--porcelain"])?
            .trim()
            .is_empty(),
        "Independent code review is blocked: task clone has uncommitted changes outside the immutable diff"
    );

    let diff_path = dir.join("code-review-subject.patch");
    runner.git_to_file_bounded(
        &state.task_repository,
        &[
            "diff",
            "--binary",
            &format!("{}...{}", state.base_commit, head),
        ],
        &diff_path,
        MAX_DIFF_BYTES,
    )?;
    let diff = fs::read(&diff_path)?;
    let diff_text = String::from_utf8(diff.clone()).map_err(|_| {
        anyhow::anyhow!(
            "Independent code review is blocked: complete diff is not UTF-8 review evidence"
        )
    })?;
    // A verified no-op task still receives an independent verdict. The empty
    // diff is itself the exact review subject; the CodeReviewer also sees the
    // approved criteria and validated test receipt before it can pass.
    let diff_hash = digest(&diff);
    let paths = runner
        .git(
            &state.task_repository,
            &[
                "diff",
                "--name-only",
                &format!("{}...{}", state.base_commit, head),
            ],
        )?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let requirements = requirement_matrix(&state.ticket_text, task_uid);
    let approved_plan_hash = digest(spec.as_bytes());
    let report_path = dir.join("verified-report.json");
    let report = fs::read_to_string(report_path).unwrap_or_default();
    anyhow::ensure!(
        !report.trim().is_empty(),
        "Independent code review is blocked: validated test execution receipts are missing"
    );
    let subject = ReviewSubject {
        task_uid,
        feature_uid: &feature_uid,
        approved_plan_hash: &approved_plan_hash,
        task_spec_revision: &approved_plan_hash,
        task_clone_head_sha: head,
        base_sha: &state.base_commit,
        full_diff_hash: &diff_hash,
        affected_paths: &paths,
        requirement_matrix: &requirements,
        test_execution_receipts: &[report.as_str()],
    };
    let subject_hash = digest(&serde_json::to_vec(&subject)?);
    if let Some(receipt) = load_receipt(dir, task_uid)? {
        if receipt.subject_hash == subject_hash
            && receipt.diff_hash == diff_hash
            && receipt.feature_uid == feature_uid
            && receipt.base_sha == state.base_commit
            && receipt.head_sha == head
            && receipt.author_run_id == author.execution_id
            && receipt.author_session_id == author.session_id
            && receipt.verdict.pass
            && receipt.verdict.findings.is_empty()
        {
            return Ok(());
        }
        if receipt.subject_hash == subject_hash
            && (!receipt.verdict.pass || !receipt.verdict.findings.is_empty())
        {
            anyhow::bail!(
                "Independent code review did not pass; Manager resolution is required before retry. Findings are persisted in {}",
                dir.join(RECEIPT_FILE).display()
            );
        }
    }

    let reviewer = ReviewExecutionIdentity {
        execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
    };
    ensure_identity(&reviewer, "reviewer")?;
    anyhow::ensure!(
        reviewer.execution_id != author.execution_id && reviewer.session_id != author.session_id,
        "Independent code review is blocked: reviewer identity is not independent"
    );
    let (request, manifest, evidence_ids) = build_request(CodeReviewRequestInput {
        state,
        feature_uid: &feature_uid,
        subject_hash: &subject_hash,
        approved_plan_hash: &approved_plan_hash,
        diff: &diff_text,
        spec,
        report: &report,
        requirements: &requirements,
        author: &author,
        reviewer: &reviewer,
    })?;
    runner.update("Running independent read-only CodeReviewer on the frozen task diff…");
    let output = harness.execute_review(&request, &reviewer)?;
    let context = crate::harness::prompt_catalog::OutputValidationContext {
        expected_subject_hash: Some(&subject_hash),
        evidence_ids: evidence_ids.clone(),
        required_requirements: requirements.clone(),
        ..Default::default()
    };
    let verdict = match crate::harness::prompt_catalog::validate_output_with_context(
        crate::harness::prompt_catalog::PromptRole::CodeReviewer,
        &output.final_text,
        &context,
    )
    .map_err(anyhow::Error::msg)?
    {
        crate::harness::prompt_catalog::RoleOutputV1::Review(verdict) => verdict,
        _ => anyhow::bail!("CodeReviewer returned an unexpected output schema"),
    };
    anyhow::ensure!(
        !verdict.pass
            || verdict.coverage.iter().all(|coverage| coverage
                .evidence
                .iter()
                .any(|id| id.starts_with("verified-test-report-"))),
        "Independent code review did not cite the application-validated test receipt for each acceptance check"
    );
    let receipt = CodeReviewReceiptV1 {
        schema_version: 1,
        task_uid: task_uid.to_owned(),
        feature_uid,
        subject_hash,
        diff_hash,
        base_sha: state.base_commit.clone(),
        head_sha: head.to_owned(),
        author_run_id: author.execution_id,
        author_session_id: author.session_id,
        reviewer_run_id: reviewer.execution_id,
        reviewer_session_id: reviewer.session_id,
        prompt: manifest,
        requirements,
        evidence_ids: evidence_ids.into_iter().collect(),
        verdict,
    };
    validate_receipt(&receipt)?;
    crate::artifacts::atomic_write_bytes(
        &dir.join(RECEIPT_FILE),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    if !receipt.verdict.pass || !receipt.verdict.findings.is_empty() {
        anyhow::bail!(
            "Independent code review found blocking issues; Manager resolution is required. Findings are persisted in {}",
            dir.join(RECEIPT_FILE).display()
        );
    }
    Ok(())
}

mod lifecycle;
mod request;
use request::{CodeReviewRequestInput, build_request};
pub(super) fn persist_lifecycle_review(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    dir: &Path,
    state: &Implementation,
    runner: &Runner,
    expected_author: Option<&ReviewExecutionIdentity>,
) -> anyhow::Result<bool> {
    let Some(task_uid) = state.task_uid.as_deref() else {
        return Ok(false);
    };
    let Some(receipt) = load_receipt(dir, task_uid)? else {
        return Ok(false);
    };
    // Older tasks can have a stable Markdown identity and independent review
    // receipt without a lifecycle record. Publication uses the same explicit
    // legacy fallback when no lifecycle task exists; keep review persistence
    // consistent with that contract. Existing records remain strictly gated.
    let lifecycle_store = crate::core::lifecycle::LifecycleStore::new(planning_store);
    if lifecycle_store.task_for_planning_item(task_uid)?.is_none() {
        return Ok(false);
    }
    lifecycle::record_lifecycle_review(
        planning_store,
        dir,
        state,
        runner,
        expected_author,
        &receipt,
    )?;
    Ok(true)
}
#[cfg(test)]
#[path = "code_review/tests.rs"]
mod tests;

mod support;
use support::{digest, ensure_identity, git_head, is_uuid, requirement_matrix};

mod receipt;
use receipt::{CodeReviewReceiptV1, load_receipt, validate_receipt};
