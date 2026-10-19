use super::git;
use super::receipt::{IntegratedSubtask, RECEIPT_FILE, read_receipt};
use crate::core::implementation::{Implementation, Runner};
use crate::core::lifecycle::TaskRecord;
use std::path::Path;

pub fn validate_for_pr(
    dir: &Path,
    parent: &TaskRecord,
    included: &[TaskRecord],
    state: &Implementation,
    runner: &Runner,
) -> anyhow::Result<()> {
    if parent.subtask_uids.is_empty() {
        return Ok(());
    }
    let receipt = read_receipt(&dir.join(RECEIPT_FILE))?
        .ok_or_else(|| anyhow::anyhow!("Parent PR has no subtask integration receipt"))?;
    let expected = included
        .iter()
        .filter(|task| task.task_uid != parent.task_uid)
        .map(|task| {
            Ok(IntegratedSubtask {
                task_uid: task.task_uid.clone(),
                reviewed_head_sha: task
                    .reviewed_code_head_sha
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Included subtask has no reviewed code head"))?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let mut expected = expected;
    let mut recorded = receipt.subtasks.clone();
    expected.sort_by(|left, right| left.task_uid.cmp(&right.task_uid));
    recorded.sort_by(|left, right| left.task_uid.cmp(&right.task_uid));
    anyhow::ensure!(
        receipt.parent_task_uid == parent.task_uid
            && receipt.subtasks.len() == parent.subtask_uids.len()
            && recorded == expected,
        "Parent PR subtask integration receipt differs from the approved children and reviewed heads"
    );
    let verified_head = state.verified_head.as_deref().ok_or_else(|| {
        anyhow::anyhow!("Parent task has no verified head for subtask receipt validation")
    })?;
    let current_head = runner.git(&state.task_repository, &["rev-parse", "HEAD"])?;
    anyhow::ensure!(
        current_head == verified_head,
        "Parent clone changed after the verified head was recorded"
    );
    git::validate_receipt_binding(
        &state.task_repository,
        &receipt.base_sha,
        &state.base_commit,
        &receipt.integration_head_sha,
        verified_head,
        &expected,
        runner,
    )?;
    Ok(())
}
