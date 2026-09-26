use super::generation::save_progress;
use super::identity;
use super::naming::{batch_slug, ensure_unique_task_names, list, render, task_name};
use super::progress::{progress_batches, replace_progress_file};
use super::workflow::{safe_directory, save_workflow};
use crate::core::workflow::{TaskBatch, TaskBatchRef, WORKFLOW_FILE, Workflow};
use std::path::Path;

/// Finalize a matching incremental batch after full validation, or stage a new
/// complete batch for callers without a generation checkpoint. Preserve revisions.
pub fn write_batch(
    repo: &Path,
    batch: &TaskBatch,
    workflow: &mut Workflow,
) -> anyhow::Result<Vec<String>> {
    let task_dir = crate::artifacts::packet::task_dir(repo);
    safe_directory(repo, &task_dir)?;
    if let Some((directory, progress)) = progress_batches(repo).into_iter().find(|(_, p)| {
        p.total == batch.stories.len()
            && p.brief == batch.brief
            && p.specification == batch.specification
            && serde_json::to_value(&p.stories).ok() == serde_json::to_value(&batch.stories).ok()
    }) {
        let batch_identity = save_progress(repo, &progress.run, batch, progress.total)?;
        let mut next = workflow.clone();
        next.task_batches.push(TaskBatchRef {
            identity: Some(batch_identity),
            feature: batch.brief.feature_name.clone(),
            directory: directory.clone(),
            count: batch.stories.len(),
        });
        save_workflow(repo, &next)?;
        let index_path = repo.join(&directory).join("README.md");
        let index = std::fs::read_to_string(&index_path)?;
        replace_progress_file(
            &index_path,
            &index
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
                ),
        )?;
        std::fs::remove_file(repo.join(&directory).join(".packet-progress.json"))?;
        *workflow = next;
        let mut paths: Vec<_> = batch
            .stories
            .iter()
            .enumerate()
            .map(|(i, s)| {
                format!(
                    "{directory}/{}",
                    task_name(progress.feature_id.as_deref(), i, s)
                )
            })
            .collect();
        paths.extend([
            format!("{directory}/README.md"),
            format!("{directory}/specification.md"),
            WORKFLOW_FILE.into(),
        ]);
        if batch.contract.is_some() {
            paths.push(format!("{directory}/contract.json"));
        }
        return Ok(paths);
    }

    let feature = batch_slug(batch);
    let mut directory = format!("{task_dir}/{feature}");
    let mut revision = 2;
    while std::fs::symlink_metadata(repo.join(&directory)).is_ok() {
        directory = format!("{task_dir}/{feature}-{revision:02}");
        revision += 1;
    }
    let stage = repo.join(format!(
        "{}/.packet-{}-{}",
        task_dir,
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
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
    let result = (|| -> anyhow::Result<Vec<String>> {
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
                    .ok_or_else(|| anyhow::anyhow!("Generated task has no Packet identity"))?;
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
        anyhow::ensure!(
            !repo.join(&directory).exists(),
            "Task destination changed during generation"
        );
        std::fs::rename(&stage, repo.join(&directory))?;
        let mut next = workflow.clone();
        next.task_batches.push(TaskBatchRef {
            identity: Some(batch_identity),
            feature: batch.brief.feature_name.clone(),
            directory: directory.clone(),
            count: names.len(),
        });
        if let Err(e) = save_workflow(repo, &next) {
            std::fs::remove_dir_all(repo.join(&directory))?;
            return Err(e);
        }
        *workflow = next;
        let mut paths: Vec<String> = names
            .iter()
            .chain(["README.md".to_owned(), "specification.md".to_owned()].iter())
            .map(|n| format!("{directory}/{n}"))
            .collect();
        paths.push(WORKFLOW_FILE.into());
        if batch.contract.is_some() {
            paths.push(format!("{directory}/contract.json"));
        }
        Ok(paths)
    })();
    if stage.exists() {
        let _ = std::fs::remove_dir_all(stage);
    }
    result
}
