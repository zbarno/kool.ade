use super::identity;
use super::naming::{batch_slug, ensure_unique_task_names, render, task_name};
use super::progress::{ProgressBatch, progress_batches};
use super::workflow::safe_directory;
use crate::artifacts::planning_store::PlanningRoot;
use crate::core::workflow::TaskBatch;

/// Publish only validated stories, preserving their contents on subsequent updates.
pub fn save_progress<R: PlanningRoot + ?Sized>(
    repo: &R,
    run: &str,
    batch: &TaskBatch,
    total: usize,
) -> anyhow::Result<crate::domain::ArtifactIdentity> {
    let store = repo.planning_store();
    let revision = store.revision()?;
    save_progress_expected(repo, run, batch, total, &revision).map(|(identity, _)| identity)
}

pub fn save_progress_expected<R: PlanningRoot + ?Sized>(
    repo: &R,
    run: &str,
    batch: &TaskBatch,
    total: usize,
    expected_revision: &str,
) -> anyhow::Result<(crate::domain::ArtifactIdentity, String)> {
    let task_dir = crate::artifacts::koolade::task_dir(repo);
    let layout = repo.planning_layout();
    let store = repo.planning_store();
    safe_directory(repo, &task_dir)?;
    let existing = progress_batches(repo)
        .into_iter()
        .find(|(_, p)| p.run == run);
    let (directory, naming_id, recorded_identity) = if let Some((directory, progress)) = existing {
        anyhow::ensure!(
            progress.task_routing == batch.task_routing,
            "Task routing changed since generation was interrupted"
        );
        (directory, progress.feature_id, progress.identity)
    } else {
        let feature = batch_slug(batch);
        let mut directory = format!("{task_dir}/{feature}");
        let mut revision = 2;
        while layout
            .canonical_path(&directory)
            .is_some_and(|path| path.exists())
        {
            directory = format!("{task_dir}/{feature}-{revision:02}");
            revision += 1;
        }
        (directory, batch.feature_id.clone(), None)
    };
    let directory_path = layout
        .canonical_path(&directory)
        .ok_or_else(|| anyhow::anyhow!("Task directory is outside the planning root"))?;
    let document_identity = identity::read_identity_in(repo, &directory_path.join("README.md"))?;
    let mut batch_identity = identity::choose_batch_identity(
        recorded_identity,
        document_identity,
        &batch.brief.feature_name,
    )?;
    identity::link_batch_to_feature(repo, batch.feature_id.as_deref(), &mut batch_identity)?;
    let names: Vec<_> = batch
        .stories
        .iter()
        .enumerate()
        .map(|(i, s)| task_name(naming_id.as_deref(), i, s))
        .collect();
    ensure_unique_task_names(&names)?;
    let mut changes = Vec::new();
    let mut identified = Vec::with_capacity(batch.stories.len());
    let mut task_uids = Vec::with_capacity(batch.stories.len());
    for (i, story) in batch.stories.iter().enumerate() {
        let path = directory_path.join(&names[i]);
        anyhow::ensure!(
            !std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()),
            "Refusing linked task story"
        );
        let base = render(batch, i, story, &names);
        let expected = identity::with_planning_path_identity(
            repo,
            &path,
            &base,
            names[i].trim_end_matches(".md"),
            &story.title,
            Some(&batch_identity.uid),
        )?;
        let task_identity = crate::domain::ArtifactIdentity::from_markdown(&expected)?
            .ok_or_else(|| anyhow::anyhow!("Generated task has no Koolade identity"))?;
        task_uids.push(task_identity.uid.clone());
        identified.push((base, expected, task_identity));
    }
    for (i, story) in batch.stories.iter().enumerate() {
        let path = directory_path.join(&names[i]);
        let dependency_uids = story
            .dependencies
            .iter()
            .map(|dependency| {
                let index = dependency.checked_sub(1).ok_or_else(|| {
                    anyhow::anyhow!("Task dependency index {dependency} is invalid")
                })?;
                task_uids
                    .get(index)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Task dependency index {dependency} is invalid"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let metadata = super::metadata::TaskMetadata::new(
            &identified[i].2,
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
        let expected = super::metadata::embed(&identified[i].1, &metadata)?;
        if path.exists() {
            let saved = String::from_utf8(repo.read_planning_path(&path)?)?;
            anyhow::ensure!(
                saved == expected || identity::visible_content(&saved) == identified[i].0,
                "Saved task was edited: {}. Preserve the edit and review before retrying.",
                path.display()
            );
            if saved != expected {
                stage_progress_file(repo, &mut changes, &path, &expected)?;
            }
        } else {
            stage_progress_file(repo, &mut changes, &path, &expected)?;
        }
    }
    stage_progress_file(
        repo,
        &mut changes,
        &directory_path.join("specification.md"),
        &batch.specification,
    )?;
    if let Some(contract) = &batch.contract {
        let path = directory_path.join("contract.json");
        let encoded = serde_json::to_string_pretty(contract)?;
        if path.exists() {
            anyhow::ensure!(
                String::from_utf8(repo.read_planning_path(&path)?)? == encoded,
                "Frozen batch contract changed during generation"
            );
        } else {
            stage_progress_file(repo, &mut changes, &path, &encoded)?;
        }
    }
    let mut index = format!(
        "# {} — task stories\n\n**Status: In progress — {} of {total} stories saved.**\n\nEach saved story is individually validated. Batch coverage and dependencies are not yet finalized. Generation can be resumed after interruption.\n\n[Approved specification](specification.md)\n\n",
        batch.brief.feature_name,
        names.len()
    );
    for (i, story) in batch.stories.iter().enumerate() {
        index.push_str(&format!("{}. [{}]({})\n", i + 1, story.title, names[i]));
    }
    let index = identity::embed_identity(&index, &batch_identity)?;
    stage_progress_file(
        repo,
        &mut changes,
        &directory_path.join("README.md"),
        &index,
    )?;
    stage_progress_file(
        repo,
        &mut changes,
        &directory_path.join(".koolade-progress.json"),
        &serde_json::to_string_pretty(&ProgressBatch {
            run: run.into(),
            total,
            brief: batch.brief.clone(),
            specification: batch.specification.clone(),
            stories: batch.stories.clone(),
            task_routing: batch.task_routing.clone(),
            feature_id: naming_id,
            identity: Some(batch_identity.clone()),
        })?,
    )?;
    let (_, revision) = store.transaction_with_revision(&changes, Some(expected_revision))?;
    Ok((batch_identity, revision))
}

fn stage_progress_file<R: PlanningRoot + ?Sized>(
    repo: &R,
    changes: &mut Vec<(String, Vec<u8>)>,
    path: &std::path::Path,
    text: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()),
        "Refusing linked task file"
    );
    let store = repo.planning_store();
    let relative = path
        .strip_prefix(&store.root)
        .map_err(|_| anyhow::anyhow!("Task file is outside the planning store"))?
        .to_string_lossy()
        .replace('\\', "/");
    changes.push((relative, text.as_bytes().to_vec()));
    Ok(())
}
