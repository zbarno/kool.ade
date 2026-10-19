use super::identity;
mod stage;
use super::naming::{batch_slug, ensure_unique_task_names, list, render, task_name};
use super::progress::progress_batches;
use super::workflow::safe_directory;
use crate::artifacts::planning_store::PlanningRoot;
use crate::core::workflow::{TaskBatch, TaskBatchRef, Workflow};

/// Finalize a matching incremental batch after full validation, or stage a new
/// complete batch for callers without a generation checkpoint. Preserve revisions.
pub fn write_batch<R: PlanningRoot + ?Sized>(
    repo: &R,
    batch: &TaskBatch,
    workflow: &mut Workflow,
) -> anyhow::Result<Vec<String>> {
    let expected_revision = repo.planning_store().revision()?;
    write_batch_expected(repo, batch, workflow, &expected_revision)
}

pub fn write_batch_expected<R: PlanningRoot + ?Sized>(
    repo: &R,
    batch: &TaskBatch,
    workflow: &mut Workflow,
    expected_revision: &str,
) -> anyhow::Result<Vec<String>> {
    write_batch_expected_with_revision(repo, batch, workflow, expected_revision)
        .map(|(paths, _)| paths)
}

pub fn write_batch_expected_with_revision<R: PlanningRoot + ?Sized>(
    repo: &R,
    batch: &TaskBatch,
    workflow: &mut Workflow,
    expected_revision: &str,
) -> anyhow::Result<(Vec<String>, String)> {
    write_batch_expected_with_revision_and_checks(repo, batch, workflow, expected_revision, &[])
}

pub(crate) fn write_batch_expected_with_revision_and_checks<R: PlanningRoot + ?Sized>(
    repo: &R,
    batch: &TaskBatch,
    workflow: &mut Workflow,
    expected_revision: &str,
    additional_checks: &[crate::artifacts::planning_store::RecordRevisionCheck],
) -> anyhow::Result<(Vec<String>, String)> {
    let store = repo.planning_store();
    let actual_revision = store.revision()?;
    if actual_revision != expected_revision {
        return Err(
            crate::artifacts::planning_store::StoreError::StaleRevision {
                expected: expected_revision.to_owned(),
                actual: actual_revision,
            }
            .into(),
        );
    }
    let layout = repo.planning_layout();
    let task_dir = crate::artifacts::koolade::task_dir(repo);
    safe_directory(repo, &task_dir)?;
    if let Some((directory, progress)) = progress_batches(repo).into_iter().find(|(_, p)| {
        p.total == batch.stories.len()
            && p.brief == batch.brief
            && p.specification == batch.specification
            && p.task_routing == batch.task_routing
            && serde_json::to_value(&p.stories).ok() == serde_json::to_value(&batch.stories).ok()
    }) {
        let batch_identity = progress
            .identity
            .or(identity::read_identity_in(
                repo,
                &layout
                    .canonical_path(&directory)
                    .ok_or_else(|| anyhow::anyhow!("Task directory is outside the planning root"))?
                    .join("README.md"),
            )?)
            .ok_or_else(|| anyhow::anyhow!("Saved task batch has no stable identity"))?;
        let mut next = workflow.clone();
        next.task_batches.push(TaskBatchRef {
            identity: Some(batch_identity),
            feature: batch.brief.feature_name.clone(),
            directory: store_relative_directory(&directory),
            count: batch.stories.len(),
            created_at_ms: super::record_files::next_batch_timestamp(&next),
        });
        let index_path = layout
            .canonical_path(&directory)
            .ok_or_else(|| anyhow::anyhow!("Task directory is outside the planning root"))?
            .join("README.md");
        let index = String::from_utf8(repo.read_planning_path(&index_path)?)?;
        let finalized_index = index
            .replace(
                &format!(
                    "Status: In progress — {} of {} stories saved.",
                    batch.stories.len(),
                    batch.stories.len()
                ),
                "Status: Complete — all stories and batch checks validated.",
            )
            .replace(
                "Batch coverage and dependencies are not yet finalized.",
                "Batch coverage and dependencies are validated.",
            );
        let progress_path = layout
            .canonical_path(&directory)
            .ok_or_else(|| anyhow::anyhow!("Task directory is outside the planning root"))?
            .join(".koolade-progress.json");
        let progress_relative = progress_path.strip_prefix(layout.root())?;
        let (mut changes, workflow_checks, _) = super::record_files::changes(repo, &next)?;
        let index_relative = index_path.strip_prefix(layout.root())?;
        changes.push((
            index_relative.to_string_lossy().replace('\\', "/"),
            finalized_index.into_bytes(),
        ));
        let mut record_checks = workflow_checks;
        let task_paths: Vec<_> = batch
            .stories
            .iter()
            .enumerate()
            .map(|(index, story)| task_name(progress.feature_id.as_deref(), index, story))
            .collect();
        for task_path in &task_paths {
            let text = String::from_utf8(repo.read_planning(&format!("{directory}/{task_path}"))?)?;
            let metadata = super::metadata::parse(&text)?
                .ok_or_else(|| anyhow::anyhow!("Generated task {task_path} has no metadata"))?;
            let (path, bytes, check) = super::task_state::create(&metadata)?;
            changes.push((path, bytes));
            record_checks.push(check);
        }
        let (committed, revision) = store.transaction_with_removals_and_record_revisions(
            &changes,
            &[progress_relative.to_string_lossy().replace('\\', "/")],
            Some(expected_revision),
            &record_checks,
        )?;
        *workflow = super::record_files::load(repo)?;
        let directory_relative = store_relative_directory(&directory);
        let mut paths = committed
            .iter()
            .map(|path| store.git_path(path))
            .collect::<std::collections::BTreeSet<_>>();
        for path in std::iter::once("README.md".to_owned())
            .chain(std::iter::once("specification.md".to_owned()))
            .chain(task_paths)
            .chain(batch.contract.as_ref().map(|_| "contract.json".to_owned()))
        {
            paths.insert(store.git_path(&format!("{directory_relative}/{path}")));
        }
        return Ok((paths.into_iter().collect(), revision));
    }

    stage::write_new_batch(repo, batch, workflow, expected_revision, additional_checks)
}

fn store_relative_directory(directory: &str) -> String {
    directory
        .strip_prefix(".koolade-packet/")
        .unwrap_or(directory)
        .to_owned()
}
