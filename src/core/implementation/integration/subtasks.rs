use super::super::*;
use crate::core::lifecycle::{LifecycleStore, TaskPhase, TaskRecord};
use std::{collections::BTreeSet, path::Path};
mod git;
mod pull_request;
mod receipt;
#[cfg(test)]
#[path = "subtasks/tests.rs"]
mod tests;
use git::{ensure_clean, integrated_children};
pub(super) use pull_request::validate_for_pr;
use receipt::{IntegratedSubtask, RECEIPT_FILE, SubtaskIntegrationReceiptV1, read_receipt};

struct ChildState {
    task: TaskRecord,
    state: Implementation,
}

/// Integrate every verified subtask into the controlled parent clone before
/// the parent implementer, reviewer, QA worker, and spec reconciler run.
pub fn integrate_subtasks(
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    state_root: &Path,
    dir: &Path,
    parent_state: &Implementation,
    runner: &Runner,
) -> anyhow::Result<()> {
    let Some(planning_uid) = parent_state.task_uid.as_deref() else {
        return Ok(());
    };
    let lifecycle = LifecycleStore::new(planning_store);
    let Some(parent) = lifecycle.task_for_planning_item(planning_uid)? else {
        return Ok(());
    };
    if parent.subtask_uids.is_empty() {
        return Ok(());
    }
    anyhow::ensure!(
        parent.parent_task_uid.is_none()
            && parent.planning_item_uid.as_deref() == Some(planning_uid)
            && parent.task_uid == planning_uid,
        "Only a standalone lifecycle task can integrate subtasks"
    );
    anyhow::ensure!(
        parent_state.task_repository_kind == TaskRepositoryKind::Clone
            && parent_state.task_repository.exists(),
        "Subtasks must be integrated into a fresh, controlled parent clone before implementation verification"
    );
    task_repository::validate_clone_path_at(parent_state, &parent_state.task_repository)?;
    let children = load_children(state_root, planning_store, &parent, parent_state, runner)?;
    let planned = children
        .iter()
        .map(|child| IntegratedSubtask {
            task_uid: child.task.task_uid.clone(),
            reviewed_head_sha: child
                .state
                .verified_head
                .clone()
                .expect("child validation requires a verified head"),
        })
        .collect::<Vec<_>>();
    let path = dir.join(RECEIPT_FILE);
    let existing = read_receipt(&path)?;
    if let Some(receipt) = existing.as_ref() {
        anyhow::ensure!(
            receipt.parent_task_uid == parent.task_uid
                && receipt.base_sha == parent_state.base_commit
                && receipt.subtasks == planned,
            "Saved subtask integration receipt differs from the approved child heads; preserve the parent clone for review"
        );
        anyhow::ensure!(
            runner
                .git(
                    &parent_state.task_repository,
                    &[
                        "merge-base",
                        "--is-ancestor",
                        &receipt.integration_head_sha,
                        "HEAD",
                    ],
                )
                .is_ok(),
            "Saved subtask integration is not present in the parent clone"
        );
        if let Some(verified_head) = parent_state.verified_head.as_deref() {
            anyhow::ensure!(
                runner
                    .git(
                        &parent_state.task_repository,
                        &[
                            "merge-base",
                            "--is-ancestor",
                            &receipt.integration_head_sha,
                            verified_head
                        ],
                    )
                    .is_ok(),
                "Parent verification head does not contain its recorded subtask integration"
            );
        }
        return Ok(());
    }
    anyhow::ensure!(
        parent_state.verified_head.is_none(),
        "Parent task was verified before required subtask integration; preserve it for review"
    );
    ensure_clean(&parent_state.task_repository, runner)?;
    let integrated = integrated_children(
        &parent_state.task_repository,
        &parent_state.base_commit,
        "HEAD",
        &planned,
        runner,
    )?;

    for child in &children {
        let uid = &child.task.task_uid;
        let head = child
            .state
            .verified_head
            .as_deref()
            .expect("child validation requires a verified head");
        if integrated.contains(uid) {
            continue;
        }
        let child_repo = child
            .state
            .task_repository
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Subtask repository path is not UTF-8"))?;
        git::squash_reviewed_child(
            &parent_state.task_repository,
            child_repo,
            uid,
            child.task.planning_item_uid.as_deref().unwrap_or(uid),
            head,
            runner,
        )?;
    }

    let receipt = SubtaskIntegrationReceiptV1 {
        schema_version: 1,
        parent_task_uid: parent.task_uid,
        base_sha: parent_state.base_commit.clone(),
        integration_head_sha: runner
            .git(&parent_state.task_repository, &["rev-parse", "HEAD"])?
            .to_owned(),
        subtasks: planned,
    };
    receipt.validate()?;
    crate::artifacts::atomic_write_bytes(&path, &serde_json::to_vec_pretty(&receipt)?)?;
    runner.update(format!(
        "Integrated {} independently reviewed subtasks into the parent clone before final verification…",
        receipt.subtasks.len()
    ));
    Ok(())
}

fn load_children(
    state_root: &Path,
    planning_store: &crate::artifacts::planning_store::PlanningStore,
    parent: &TaskRecord,
    parent_state: &Implementation,
    runner: &Runner,
) -> anyhow::Result<Vec<ChildState>> {
    let lifecycle = LifecycleStore::new(planning_store);
    let mut children = parent
        .subtask_uids
        .iter()
        .map(|uid| lifecycle.load_task(uid))
        .collect::<Result<Vec<_>, _>>()?;
    anyhow::ensure!(
        children.len() == parent.subtask_uids.len()
            && children.iter().all(|child| {
                child.parent_task_uid.as_deref() == Some(parent.task_uid.as_str())
                    && child.feature_uid == parent.feature_uid
                    && child.phase == TaskPhase::Done
                    && child.code_review.as_ref().is_some_and(|receipt| {
                        receipt.decision == crate::core::lifecycle::GateDecision::Approved
                    })
                    && child.runtime_qa.as_ref().is_some_and(|receipt| {
                        receipt.decision == crate::core::lifecycle::GateDecision::Approved
                    })
                    && child.spec_reconciliation.is_some()
            }),
        "Parent task cannot start until every owned subtask has current review, QA, and specification receipts"
    );
    let mut ordered = Vec::with_capacity(children.len());
    while !children.is_empty() {
        let completed = ordered
            .iter()
            .map(|child: &TaskRecord| child.task_uid.as_str())
            .collect::<BTreeSet<_>>();
        let ready = children.iter().position(|child| {
            child
                .dependency_task_uids
                .iter()
                .filter(|dependency| parent.subtask_uids.contains(dependency))
                .all(|dependency| completed.contains(dependency.as_str()))
        });
        let Some(index) = ready else {
            anyhow::bail!("Validated subtask graph has no topological integration order");
        };
        ordered.push(children.remove(index));
    }

    ordered
        .into_iter()
        .map(|task| {
            let planning_uid = task
                .planning_item_uid
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Subtask has no planning item identity"))?;
            let state_dir =
                super::super::state_paths::state_dir_by_task_uid(state_root, planning_uid)?
                    .ok_or_else(|| {
                        anyhow::anyhow!("Verified subtask implementation state is missing")
                    })?;
            let state = read_state_file(&state_dir.join("state.json"))?;
            anyhow::ensure!(
                state.task_uid.as_deref() == Some(planning_uid)
                    && state.status == ImplementationStatus::AwaitingApproval
                    && state.pr_url.is_none()
                    && state.pr_state.is_none()
                    && state.merged_commit.is_none()
                    && state.verified_head.as_deref() == task.reviewed_code_head_sha.as_deref()
                    && state.repository_id == parent_state.repository_id,
                "Subtask implementation state is not locally verified for this parent repository"
            );
            anyhow::ensure!(
                task_repository::validate_clone_path_at(&state, &state.task_repository).is_ok(),
                "Subtask clone path is outside its Kool.ad/e allocation"
            );
            anyhow::ensure!(
                state.task_repository_kind == TaskRepositoryKind::Clone
                    && state.base_commit == parent_state.base_commit
                    && state.source_branch == parent_state.source_branch
                    && state.base == parent_state.base
                    && state.repository_identity == parent_state.repository_identity
                    && state.repository_cache == parent_state.repository_cache,
                "Subtask and parent clones do not share the approved repository baseline"
            );
            let head = state.verified_head.as_deref().unwrap_or_default();
            anyhow::ensure!(
                runner.git(&state.task_repository, &["rev-parse", "HEAD"])? == head
                    && runner.git(&state.task_repository, &["symbolic-ref", "--short", "HEAD"])?
                        == state.branch
                    && runner
                        .git(
                            &state.task_repository,
                            &["merge-base", "--is-ancestor", &state.base_commit, head],
                        )
                        .is_ok(),
                "Subtask clone no longer matches its reviewed head and base"
            );
            ensure_clean(&state.task_repository, runner)?;
            Ok(ChildState { task, state })
        })
        .collect()
}
