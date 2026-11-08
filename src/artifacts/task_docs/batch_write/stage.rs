use super::*;

pub(super) fn write_new_batch<R: PlanningRoot + ?Sized>(
    repo: &R,
    batch: &TaskBatch,
    workflow: &mut Workflow,
    expected_revision: &str,
    additional_checks: &[crate::artifacts::planning_store::RecordRevisionCheck],
) -> anyhow::Result<(Vec<String>, String)> {
    let store = repo.planning_store();
    let layout = repo.planning_layout();
    let task_dir = crate::artifacts::koolade::task_dir(repo);
    safe_directory(repo, &task_dir)?;
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
    let stage = crate::persistence::temporary_directory("koolade-task-batch-")?;
    let mut batch_identity = identity::new_batch_identity(&batch.brief.feature_name);
    identity::link_batch_to_feature(repo, batch.feature_id.as_deref(), &mut batch_identity)?;
    let names: Vec<_> = batch
        .stories
        .iter()
        .enumerate()
        .map(|(i, s)| task_name(batch.feature_id.as_deref(), i, s))
        .collect();
    ensure_unique_task_names(&names)?;
    let task_intent_uid = super::super::task_intent_uid_for_batch(repo, batch)?;
    let task_intent_cycle_uid = super::super::task_intent_cycle_uid_for_batch(repo, batch)?;
    (|| -> anyhow::Result<(Vec<String>, String)> {
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
            let path = stage.path().join(&names[i]);
            let base = render(batch, i, story, &names);
            let mut identified_text = identity::with_path_identity(
                &path,
                &base,
                names[i].trim_end_matches(".md"),
                &story.title,
                Some(&batch_identity.uid),
            )?;
            if let Some(graph_uid) = story.graph_uid.as_deref() {
                identified_text = identity::replace_uid(&identified_text, graph_uid)?;
            }
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
            anyhow::ensure!(
                !story.target_repository.is_empty(),
                "Task generation has no explicit target repository"
            );
            let repository_id = &story.target_repository;
            let source_sha = story
                .source_sha
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Task generation has no pinned source commit"))?;
            let structural_parent_uid = story
                .parent_task_index
                .map(|parent| {
                    parent
                        .checked_sub(1)
                        .and_then(|index| task_uids.get(index))
                        .map(String::as_str)
                        .ok_or_else(|| anyhow::anyhow!("Task parent index {parent} is invalid"))
                })
                .transpose()?;
            let inherited_git_parent_uid = story
                .parent_task_index
                .and_then(|parent| parent.checked_sub(1))
                .and_then(|index| batch.stories.get(index).zip(task_uids.get(index)))
                .filter(|(parent, _)| {
                    parent.target_repository == *repository_id
                        && parent.source_sha.as_deref() == Some(source_sha)
                })
                .map(|(_, uid)| uid.as_str());
            let git_intent =
                batch.git_intent_for_story(repository_id, source_sha, inherited_git_parent_uid)?;
            let metadata = super::super::metadata::TaskMetadata::new(
                &identified[i].1,
                repository_id,
                dependency_uids,
            )?
            .with_task_intent_uid(task_intent_uid.as_deref())?
            .with_task_intent_cycle_uid(task_intent_cycle_uid.as_deref())?
            .with_parent_task_uid(structural_parent_uid)?
            .with_git_intent(Some(&git_intent))?
            .with_parallelizable(story.parallelizable)?
            .with_task_routing(
                &batch.task_routing.overrides,
                batch.task_routing.source_work_uid.as_deref(),
                batch.task_routing.inherited_from.as_deref(),
            )?;
            let contents = super::super::metadata::embed(&identified[i].0, &metadata)?;
            std::fs::write(stage.path().join(&names[i]), contents)?;
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
        std::fs::write(stage.path().join("README.md"), index)?;
        std::fs::write(stage.path().join("specification.md"), &batch.specification)?;
        if let Some(contract) = &batch.contract {
            std::fs::write(
                stage.path().join("contract.json"),
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
            created_at_ms: super::super::record_files::next_batch_timestamp(&next),
        });
        let directory_relative = store_relative_directory(&directory);
        let (mut changes, workflow_checks) = super::super::record_files::changes(repo, &next)?;
        let mut record_checks = workflow_checks;
        record_checks.extend(additional_checks.iter().cloned());
        let mut staged_task_state = Vec::new();
        for name in names.iter().take(identified.len()) {
            let task_text = std::fs::read_to_string(stage.path().join(name))?;
            let metadata = super::super::metadata::parse(&task_text)?
                .ok_or_else(|| anyhow::anyhow!("Generated task {name} has no metadata"))?;
            let (path, bytes, check) = super::super::task_state::create(&metadata)?;
            staged_task_state.push((path, bytes));
            record_checks.push(check);
        }
        changes.extend(
            std::fs::read_dir(stage.path())?
                .map(|entry| {
                    let entry = entry?;
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let relative = format!("{directory_relative}/{name}");
                    Ok((relative, std::fs::read(entry.path())?))
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
        );
        changes.extend(staged_task_state);
        if let Some(root_uid) = task_intent_uid.as_deref() {
            let (history_changes, history_checks) =
                crate::core::planning_work::linked_executable_task_changes(
                    &store, root_uid, &task_uids,
                )?;
            changes.extend(history_changes);
            record_checks.extend(history_checks);
        }
        let (committed, revision) = store.transaction_with_revision_and_record_revisions(
            &changes,
            Some(expected_revision),
            &record_checks,
        )?;
        if committed.is_empty() {
            anyhow::bail!("Validated task batch produced no planning changes");
        }
        *workflow = super::super::record_files::load(repo)?;
        Ok((
            committed
                .iter()
                .map(|path| store.relative_path(path))
                .collect(),
            revision,
        ))
    })()
}
