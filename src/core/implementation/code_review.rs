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

mod subject;
use subject::ReviewSubject;

pub(super) fn review_verified(
    dir: &Path,
    state: &Implementation,
    lifecycle_task: Option<&crate::core::lifecycle::TaskRecord>,
    harness: &dyn AiHarness,
    runner: &Runner,
) -> anyhow::Result<()> {
    review_verified_inner(dir, state, lifecycle_task, harness, runner, None, None)
}

#[derive(Clone, Debug)]
pub(crate) struct CodeReviewArbitrationContext {
    pub(crate) task_uid: String,
    pub(crate) finding_id: String,
    pub(crate) subject_hash: String,
    pub(crate) head_sha: String,
    pub(crate) diff_hash: String,
    pub(crate) prior_reviewer_run_id: String,
    pub(crate) finding_fingerprint: String,
    pub(crate) finding_record: String,
    pub(crate) rationale: String,
}

pub(super) fn arbitrate_persisted_finding(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    state_root: &Path,
    ticket: &str,
    harness: &dyn AiHarness,
    runner: &Runner,
    context: &CodeReviewArbitrationContext,
) -> anyhow::Result<ManagerCodeReviewFindings> {
    let dir = super::state_dir_for_task(state_root, ticket, Some(&context.task_uid))?;
    let state = super::read_state_file(&dir.join("state.json"))?;
    anyhow::ensure!(
        state.task_uid.as_deref() == Some(context.task_uid.as_str()),
        "Code-review arbitration task identity changed"
    );
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(planning_store);
    let task = lifecycle
        .task_for_planning_item(&context.task_uid)?
        .ok_or_else(|| anyhow::anyhow!("Contested code-review task no longer exists"))?;
    anyhow::ensure!(
        task.phase == crate::core::lifecycle::TaskPhase::Repairing
            && task.reviewed_code_subject_sha256.as_deref() == Some(&context.subject_hash)
            && task.reviewed_code_head_sha.as_deref() == state.verified_head.as_deref(),
        "Contested code-review subject is no longer the current repair subject"
    );
    let prior = load_receipt(&dir, &context.task_uid)?
        .ok_or_else(|| anyhow::anyhow!("Contested code finding has no validated prior review"))?;
    let prior_reviewer_run_id = prior.reviewer_run_id.clone();
    anyhow::ensure!(
        prior.subject_hash == context.subject_hash
            && prior.head_sha == context.head_sha
            && prior.diff_hash == context.diff_hash
            && prior.reviewer_run_id == context.prior_reviewer_run_id,
        "Code-review arbitration request does not match the current validated review"
    );
    let author = ReviewExecutionIdentity {
        execution_id: prior.author_run_id.clone(),
        session_id: prior.author_session_id.clone(),
    };
    let _review_result =
        review_verified_for_arbitration(&dir, &state, Some(&task), harness, runner, context);
    let reviewed = load_receipt(&dir, &context.task_uid)?
        .ok_or_else(|| anyhow::anyhow!("Code-review arbitration did not persist a receipt"))?;
    anyhow::ensure!(
        reviewed.reviewer_run_id != prior_reviewer_run_id
            && reviewed.subject_hash != prior.subject_hash
            && reviewed.head_sha == prior.head_sha
            && reviewed.diff_hash == prior.diff_hash,
        "Code-review arbitration did not create a new independent current-subject receipt"
    );
    persist_lifecycle_review(planning_store, &dir, &state, runner, Some(&author))?;
    manager_findings_for_task(state_root, ticket, &context.task_uid)?
        .ok_or_else(|| anyhow::anyhow!("Code-review arbitration receipt could not be loaded"))
}

pub(super) fn review_verified_for_arbitration(
    dir: &Path,
    state: &Implementation,
    lifecycle_task: Option<&crate::core::lifecycle::TaskRecord>,
    harness: &dyn AiHarness,
    runner: &Runner,
    context: &CodeReviewArbitrationContext,
) -> anyhow::Result<()> {
    let prior =
        load_receipt(dir, state.task_uid.as_deref().unwrap_or_default())?.ok_or_else(|| {
            anyhow::anyhow!("Contested code finding has no validated prior review receipt")
        })?;
    anyhow::ensure!(
        prior.subject_hash == context.subject_hash
            && prior
                .verdict
                .findings
                .iter()
                .any(|finding| finding.id == context.finding_id
                    && finding_fingerprint(finding) == context.finding_fingerprint),
        "Contested code finding no longer matches the current independent review receipt"
    );
    let author = ReviewExecutionIdentity {
        execution_id: prior.author_run_id,
        session_id: prior.author_session_id,
    };
    review_verified_inner(
        dir,
        state,
        lifecycle_task,
        harness,
        runner,
        Some(context),
        Some(&author),
    )
}

mod review;
use review::review_verified_inner;

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
use receipt::{
    CodeReviewReceiptV1, is_approved_verdict, load_receipt,
    same_code_failed_review_requires_manager_resolution, uses_current_prompt, validate_receipt,
};
mod manager_findings;
pub(crate) use manager_findings::{
    ManagerCodeReviewFindings, finding_fingerprint, manager_findings_for_task,
};
