use super::{CodeReviewReceiptV1, Implementation, Runner, digest};
use crate::harness::ReviewExecutionIdentity;
use crate::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, LifecycleEventKind, LifecycleStore, TaskEvent,
        TaskPhase,
    },
};
use sha2::Digest as _;
use std::{fs, path::Path, time::SystemTime};

pub(super) fn record_lifecycle_review(
    planning_store: &PlanningStore,
    dir: &Path,
    state: &Implementation,
    runner: &Runner,
    expected_author: Option<&ReviewExecutionIdentity>,
    receipt: &CodeReviewReceiptV1,
) -> anyhow::Result<()> {
    let planning_uid = state
        .task_uid
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("CodeReviewer receipt has no planning task identity"))?;
    anyhow::ensure!(
        receipt.task_uid == planning_uid,
        "CodeReviewer receipt belongs to a different planning task"
    );
    let feature_id = state
        .ticket_text
        .lines()
        .find_map(|line| line.strip_prefix("Feature: "))
        .and_then(|value| {
            crate::core::workflow::feature_ids_in(value)
                .into_iter()
                .next()
        })
        .ok_or_else(|| anyhow::anyhow!("CodeReviewer task has no stable feature identity"))?;
    anyhow::ensure!(
        receipt.feature_uid == feature_id,
        "CodeReviewer receipt belongs to a different approved feature"
    );
    let author = expected_author.ok_or_else(|| {
        anyhow::anyhow!("CodeReviewer lifecycle receipt has no current author execution identity")
    })?;
    anyhow::ensure!(
        receipt.author_run_id == author.execution_id
            && receipt.author_session_id == author.session_id,
        "CodeReviewer receipt belongs to a stale author execution"
    );
    let store = LifecycleStore::new(planning_store);
    let mut task = store
        .task_for_planning_item(planning_uid)?
        .ok_or_else(|| anyhow::anyhow!("planning task has no lifecycle task record"))?;
    let expected_spec_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&(
            1_u16,
            task.approved_plan_sha256.as_str(),
            state.ticket_text.as_str()
        ))?)
    );
    anyhow::ensure!(
        task.task_spec_sha256.as_deref() == Some(expected_spec_hash.as_str()),
        "CodeReviewer task contents differ from the approved lifecycle task"
    );
    let verified_head = state
        .verified_head
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("verified task head is missing"))?;
    anyhow::ensure!(
        receipt.head_sha == verified_head
            && super::git_head(runner, &state.task_repository)? == verified_head,
        "CodeReviewer receipt is stale for the current task clone head"
    );
    let diff_path = dir.join("code-review-subject.patch");
    runner.git_to_file_bounded(
        &state.task_repository,
        &[
            "diff",
            "--binary",
            &format!("{}...{}", state.base_commit, verified_head),
        ],
        &diff_path,
        super::MAX_DIFF_BYTES,
    )?;
    let diff_sha = digest(&fs::read(diff_path)?);
    anyhow::ensure!(
        receipt.diff_hash == diff_sha,
        "CodeReviewer receipt is not bound to the current immutable diff"
    );
    let gate = gate_receipt(receipt, &task.task_uid)?;
    if task.review_history.iter().any(|prior| {
        prior.receipt_uid == gate.receipt_uid
            && prior.subject_sha256 == gate.subject_sha256
            && prior.reviewer_run_id == gate.reviewer_run_id
            && prior.reviewer_session_id == gate.reviewer_session_id
    }) {
        return Ok(());
    }
    if task.phase == TaskPhase::AwaitingRuntimeQa
        && task.implementation_sha256.as_deref() == Some(receipt.diff_hash.as_str())
        && task.reviewed_code_head_sha.as_deref() == Some(receipt.head_sha.as_str())
        && task.code_review.as_ref().is_some_and(|prior| {
            prior.decision == GateDecision::Approved && prior.subject_sha256 == receipt.diff_hash
        })
    {
        // A resumed verification can re-review the same immutable tree. Keep
        // the existing approval and QA request instead of replaying a review
        // transition against a task whose phase has already advanced.
        return Ok(());
    }
    let lifecycle_task_uid = task.task_uid.clone();
    let event_id = |stage: &str| {
        uuid::Uuid::new_v5(
            &uuid::Uuid::parse_str(&lifecycle_task_uid).expect("validated lifecycle task UUID"),
            format!(
                "code-review:v1:{}:{stage}:{}",
                receipt.subject_hash, receipt.reviewer_run_id
            )
            .as_bytes(),
        )
        .hyphenated()
        .to_string()
    };
    if task.phase == TaskPhase::Ready {
        task = apply(&store, task, &event_id("start"), TaskEvent::Start)?;
    }
    if task.phase == TaskPhase::Repairing {
        task = apply(&store, task, &event_id("repair"), TaskEvent::BeginRepair)?;
    }
    if task.phase == TaskPhase::Running {
        task = apply(
            &store,
            task,
            &event_id("implementation-finished"),
            TaskEvent::ImplementationFinished {
                code_sha256: receipt.diff_hash.clone(),
            },
        )?;
    }
    if task.code_review.as_ref().is_some_and(|prior| {
        prior.receipt_uid == gate.receipt_uid
            && prior.subject_sha256 == gate.subject_sha256
            && prior.reviewer_run_id == gate.reviewer_run_id
            && prior.reviewer_session_id == gate.reviewer_session_id
    }) && task.implementation_sha256.as_deref() == Some(receipt.diff_hash.as_str())
    {
        return Ok(());
    }
    if !matches!(
        task.phase,
        TaskPhase::AwaitingReview | TaskPhase::ReReview | TaskPhase::AwaitingRuntimeQa
    ) {
        anyhow::bail!(
            "CodeReviewer result cannot advance lifecycle task in {:?} phase",
            task.phase
        );
    }
    anyhow::ensure!(
        task.implementation_sha256.as_deref() == Some(receipt.diff_hash.as_str()),
        "CodeReviewer diff differs from the lifecycle task's current implementation hash"
    );
    apply(
        &store,
        task,
        &event_id("review"),
        TaskEvent::CodeReview {
            receipt: gate,
            reviewed_head_sha: receipt.head_sha.clone(),
            review_subject_sha256: receipt.subject_hash.clone(),
        },
    )?;
    Ok(())
}

fn gate_receipt(
    receipt: &CodeReviewReceiptV1,
    lifecycle_task_uid: &str,
) -> anyhow::Result<GateReceipt> {
    super::validate_receipt(receipt)?;
    let manifest_hash = receipt.prompt.identity_sha256();
    anyhow::ensure!(
        !receipt.evidence_ids.is_empty(),
        "validated CodeReviewer receipt has no evidence references"
    );
    Ok(GateReceipt {
        receipt_uid: uuid::Uuid::new_v5(
            &uuid::Uuid::parse_str(lifecycle_task_uid)?,
            format!(
                "code-review-receipt:v1:{}:{}",
                receipt.subject_hash, receipt.reviewer_run_id
            )
            .as_bytes(),
        )
        .hyphenated()
        .to_string(),
        subject_uid: lifecycle_task_uid.to_owned(),
        subject_sha256: receipt.diff_hash.clone(),
        actor: GateActor::IndependentReviewer,
        decision: if receipt.verdict.pass && receipt.verdict.findings.is_empty() {
            GateDecision::Approved
        } else {
            GateDecision::ChangesRequested
        },
        recorded_at_unix_ms: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(anyhow::Error::new)?
            .as_millis()
            .try_into()
            .map_err(|_| anyhow::anyhow!("system clock timestamp overflow"))?,
        evidence_refs: receipt.evidence_ids.clone(),
        prompt_sha256: Some(manifest_hash),
        prompt_manifest: Some(receipt.prompt.clone()),
        reviewer_run_id: Some(receipt.reviewer_run_id.clone()),
        author_run_id: Some(receipt.author_run_id.clone()),
        reviewer_session_id: Some(receipt.reviewer_session_id.clone()),
        author_session_id: Some(receipt.author_session_id.clone()),
    })
}

fn apply(
    store: &LifecycleStore<'_>,
    task: crate::core::lifecycle::TaskRecord,
    event_uid: &str,
    event: TaskEvent,
) -> anyhow::Result<crate::core::lifecycle::TaskRecord> {
    let existing = store.events_for_feature(&task.feature_uid)?;
    if let Some(record) = existing.iter().find(|record| record.uid == event_uid) {
        anyhow::ensure!(
            record.task_uid.as_deref() == Some(task.task_uid.as_str())
                && record.event == LifecycleEventKind::Task(event.clone()),
            "CodeReviewer lifecycle event identity was reused with different content"
        );
        return Ok(store.load_task(&task.task_uid)?);
    }
    store
        .apply_task_event(&task.task_uid, task.revision, event_uid, event)
        .map_err(anyhow::Error::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewer_receipt_projects_to_exact_lifecycle_task_and_run_identity() {
        let source = super::super::tests::receipt();
        let lifecycle_uid = "00000000-0000-4000-8000-000000000009";
        let gate = gate_receipt(&source, lifecycle_uid).unwrap();

        assert_eq!(gate.subject_uid, lifecycle_uid);
        assert_eq!(gate.subject_sha256, source.diff_hash);
        assert_eq!(gate.actor, GateActor::IndependentReviewer);
        assert_eq!(gate.decision, GateDecision::Approved);
        assert_eq!(
            gate.reviewer_run_id.as_deref(),
            Some(source.reviewer_run_id.as_str())
        );
        assert_eq!(
            gate.reviewer_session_id.as_deref(),
            Some(source.reviewer_session_id.as_str())
        );
        assert_eq!(
            gate.prompt_role(),
            Some(crate::harness::prompt_catalog::PromptRole::CodeReviewer)
        );
        assert_eq!(gate.evidence_refs, source.evidence_ids);
        gate.validate().unwrap();
    }
}
