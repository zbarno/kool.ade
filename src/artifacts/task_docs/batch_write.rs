use super::identity;
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

    let feature = batch_slug(batch);
    let mut directory = format!("{task_dir}/{feature}");
    let mut revision = 2;
    while layout
        .canonical_path(&directory)
        .is_some_and(|path| std::fs::symlink_metadata(path).is_ok())
    {
        directory = format!("{task_dir}/{feature}-{revision:02}");
        revision += 1;
    }
    let stage = std::env::temp_dir().join(format!("koolade-task-batch-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&stage)?;
    let mut batch_identity = identity::new_batch_identity(&batch.brief.feature_name);
    identity::link_batch_to_feature(repo, batch.feature_id.as_deref(), &mut batch_identity)?;
    let names: Vec<_> = batch
        .stories
        .iter()
        .enumerate()
        .map(|(i, s)| task_name(batch.feature_id.as_deref(), i, s))
        .collect();
    ensure_unique_task_names(&names)?;
    let result = (|| -> anyhow::Result<(Vec<String>, String)> {
        let mut index = format!(
            "# {} — task stories\n\nGenerated after user approval of the interview and specification.\n\n## Goal\n\n{}\n\n## Intended users\n\n{}\n\n## Intended outcome\n\n{}\n\n[Approved specification](specification.md)\n\n## Implementation order\n\n",
            batch.brief.feature_name,
            batch.brief.goal,
            batch.brief.target_users,
            batch.brief.intended_outcome
        );
        let mut identified = Vec::with_capacity(batch.stories.len());
        let mut task_uids = Vec::with_capacity(batch.stories.len());
        for (i, story) in batch.stories.iter().enumerate() {
            let path = stage.join(&names[i]);
            let base = render(batch, i, story, &names);
            let identified_text = identity::with_path_identity(
                &path,
                &base,
                names[i].trim_end_matches(".md"),
                &story.title,
                Some(&batch_identity.uid),
            )?;
            let task_identity =
                crate::domain::ArtifactIdentity::from_markdown(&identified_text)?
                    .ok_or_else(|| anyhow::anyhow!("Generated task has no Koolade identity"))?;
            task_uids.push(task_identity.uid.clone());
            identified.push((identified_text, task_identity));
        }
        for (i, story) in batch.stories.iter().enumerate() {
            let dependency_uids = story
                .dependencies
                .iter()
                .map(|dependency| {
                    let index = dependency.checked_sub(1).ok_or_else(|| {
                        anyhow::anyhow!("Task dependency index {dependency} is invalid")
                    })?;
                    task_uids.get(index).cloned().ok_or_else(|| {
                        anyhow::anyhow!("Task dependency index {dependency} is invalid")
                    })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            let metadata = super::metadata::TaskMetadata::new(
                &identified[i].1,
                if story.target_repository.is_empty() {
                    "root"
                } else {
                    &story.target_repository
                },
                dependency_uids,
            )?
            .with_branch_targets(batch.branch_targets.as_ref())?
            .with_task_routing(
                &batch.task_routing.overrides,
                batch.task_routing.source_work_uid.as_deref(),
                batch.task_routing.inherited_from.as_deref(),
            )?;
            let contents = super::metadata::embed(&identified[i].0, &metadata)?;
            std::fs::write(stage.join(&names[i]), contents)?;
            index.push_str(&format!("{}. [{}]({})\n", i + 1, story.title, names[i]));
        }
        list(&mut index, "Approved scope", &batch.brief.in_scope, true);
        list(
            &mut index,
            "Success criteria",
            &batch.brief.success_criteria,
            true,
        );
        let index = identity::embed_identity(&index, &batch_identity)?;
        std::fs::write(stage.join("README.md"), index)?;
        std::fs::write(stage.join("specification.md"), &batch.specification)?;
        if let Some(contract) = &batch.contract {
            std::fs::write(
                stage.join("contract.json"),
                serde_json::to_string_pretty(contract)?,
            )?;
        }
        let destination = layout
            .canonical_path(&directory)
            .ok_or_else(|| anyhow::anyhow!("Task destination is outside the planning root"))?;
        anyhow::ensure!(
            !destination.exists(),
            "Task destination changed during generation"
        );
        let mut next = workflow.clone();
        next.task_batches.push(TaskBatchRef {
            identity: Some(batch_identity),
            feature: batch.brief.feature_name.clone(),
            directory: store_relative_directory(&directory),
            count: names.len(),
            created_at_ms: super::record_files::next_batch_timestamp(&next),
        });
        let directory_relative = store_relative_directory(&directory);
        let (mut changes, workflow_checks, _) = super::record_files::changes(repo, &next)?;
        let mut record_checks = workflow_checks;
        let mut staged_task_state = Vec::new();
        for name in names.iter().take(identified.len()) {
            let task_text = std::fs::read_to_string(stage.join(name))?;
            let metadata = super::metadata::parse(&task_text)?
                .ok_or_else(|| anyhow::anyhow!("Generated task {name} has no metadata"))?;
            let (path, bytes, check) = super::task_state::create(&metadata)?;
            staged_task_state.push((path, bytes));
            record_checks.push(check);
        }
        changes.extend(
            std::fs::read_dir(&stage)?
                .map(|entry| {
                    let entry = entry?;
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let relative = format!("{directory_relative}/{name}");
                    Ok((relative, std::fs::read(entry.path())?))
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
        );
        changes.extend(staged_task_state);
        let (committed, revision) = store.transaction_with_revision_and_record_revisions(
            &changes,
            Some(expected_revision),
            &record_checks,
        )?;
        if committed.is_empty() {
            anyhow::bail!("Validated task batch produced no planning changes");
        }
        *workflow = super::record_files::load(repo)?;
        Ok((
            committed.iter().map(|path| store.git_path(path)).collect(),
            revision,
        ))
    })();
    if stage.exists() {
        let _ = std::fs::remove_dir_all(stage);
    }
    result
}

fn store_relative_directory(directory: &str) -> String {
    directory
        .strip_prefix(".koolade-packet/")
        .unwrap_or(directory)
        .to_owned()
}
