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
            let metadata = super::super::metadata::TaskMetadata::new(
                &identified[i].1,
                if story.target_repository.is_empty() {
                    "root"
                } else {
                    &story.target_repository
                },
                dependency_uids,
            )?
            .with_branch_targets(batch.branch_targets.as_ref())?
            .with_source_sha(story.source_sha.as_deref())?
            .with_parallelizable(story.parallelizable)?
            .with_task_routing(
                &batch.task_routing.overrides,
                batch.task_routing.source_work_uid.as_deref(),
                batch.task_routing.inherited_from.as_deref(),
            )?;
            let contents = super::super::metadata::embed(&identified[i].0, &metadata)?;
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
            created_at_ms: super::super::record_files::next_batch_timestamp(&next),
        });
        let directory_relative = store_relative_directory(&directory);
        let (mut changes, workflow_checks, _) = super::super::record_files::changes(repo, &next)?;
        let mut record_checks = workflow_checks;
        record_checks.extend(additional_checks.iter().cloned());
        let mut staged_task_state = Vec::new();
        for name in names.iter().take(identified.len()) {
            let task_text = std::fs::read_to_string(stage.join(name))?;
            let metadata = super::super::metadata::parse(&task_text)?
                .ok_or_else(|| anyhow::anyhow!("Generated task {name} has no metadata"))?;
            let (path, bytes, check) = super::super::task_state::create(&metadata)?;
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
        *workflow = super::super::record_files::load(repo)?;
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
