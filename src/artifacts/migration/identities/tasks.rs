mod helpers;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use crate::{
    artifacts::layout::canonical,
    core::workflow::{TaskBatchRef, Workflow},
    domain::ArtifactIdentity,
};

use super::super::plan::Plan as ArtifactPlan;
use super::{
    Change, change, features::unique_feature_uid, files_under, preserve_markdown, read, register,
};
use helpers::{
    attach_parent, batch_title, choose_batch_identity, feature_id_from_directory, group_files,
    markdown_with_identity, task_title, validate_batch_path,
};

pub(super) fn build(
    repo: &Path,
    migration: &ArtifactPlan,
    features: &BTreeMap<String, Vec<ArtifactIdentity>>,
    seen: &mut BTreeMap<String, String>,
) -> anyhow::Result<(BTreeMap<String, String>, Vec<Change>)> {
    let workflow_path = canonical::WORKFLOW;
    let workflow_before = read(repo, migration, workflow_path)?;
    let mut workflow = workflow_before
        .as_deref()
        .map(serde_json::from_str::<Workflow>)
        .transpose()?
        .unwrap_or_default();
    let files = files_under(repo, migration, canonical::TASKS)?;
    let mut groups = group_files(files)?;

    let mut registered = BTreeSet::new();
    for batch in &workflow.task_batches {
        validate_batch_path(&batch.directory)?;
        anyhow::ensure!(
            registered.insert(batch.directory.clone()),
            "Workflow refers to task batch {} more than once",
            batch.directory
        );
    }
    for batch in &mut workflow.task_batches {
        if groups.contains_key(&batch.directory) {
            continue;
        }
        let Some(uid) = batch
            .identity
            .as_ref()
            .map(|identity| identity.uid.as_str())
        else {
            anyhow::bail!(
                "Workflow task batch {} has no stable identity and its directory is missing; restore the batch before connecting",
                batch.directory
            );
        };
        let matches = groups
            .iter()
            .filter(|(_, files)| {
                files
                    .get("README.md")
                    .and_then(|markdown| ArtifactIdentity::from_markdown(markdown).ok().flatten())
                    .is_some_and(|identity| identity.uid == uid)
            })
            .map(|(directory, _)| directory.clone())
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            batch.directory = matches[0].clone();
        } else if matches.len() > 1 {
            anyhow::bail!("Task batch identity {uid} occurs in multiple directories");
        } else {
            anyhow::bail!(
                "Workflow task batch {} has a stable identity but no matching directory; restore its files before connecting",
                batch.directory
            );
        }
    }
    let discovered = groups
        .iter()
        .filter(|(directory, files)| {
            files
                .keys()
                .any(|name| crate::artifacts::task_docs::is_task_story_filename(name))
                && !workflow
                    .task_batches
                    .iter()
                    .any(|batch| &batch.directory == *directory)
        })
        .map(|(directory, files)| {
            let progress = files
                .get(".koolade-progress.json")
                .map(|text| serde_json::from_str::<serde_json::Value>(text))
                .transpose()?
                .unwrap_or_else(|| serde_json::json!({}));
            let identity = progress
                .get("identity")
                .filter(|value| !value.is_null())
                .map(|value| serde_json::from_value::<ArtifactIdentity>(value.clone()))
                .transpose()?
                .or_else(|| {
                    files
                        .get("README.md")
                        .and_then(|text| ArtifactIdentity::from_markdown(text).ok().flatten())
                });
            let story_count = files
                .keys()
                .filter(|name| crate::artifacts::task_docs::is_task_story_filename(name))
                .count();
            Ok(TaskBatchRef {
                identity,
                feature: batch_title(None, &progress, files, directory),
                directory: directory.clone(),
                count: story_count,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    workflow.task_batches.extend(discovered);
    for batch in &workflow.task_batches {
        groups.entry(batch.directory.clone()).or_default();
    }

    let mut changes = Vec::new();
    let mut task_uids = BTreeMap::new();
    for (directory, files) in &mut groups {
        let has_progress = files.contains_key(".koolade-progress.json");
        let has_story = files
            .keys()
            .any(|name| crate::artifacts::task_docs::is_task_story_filename(name));
        let batch_ref = workflow
            .task_batches
            .iter_mut()
            .find(|batch| &batch.directory == directory);
        if batch_ref.is_none() && !has_progress && !has_story {
            continue;
        }
        let mut progress = files
            .get(".koolade-progress.json")
            .map(|text| serde_json::from_str::<serde_json::Value>(text))
            .transpose()?
            .unwrap_or_else(|| serde_json::json!({}));
        let progress_identity = progress
            .get("identity")
            .filter(|value| !value.is_null())
            .map(|value| serde_json::from_value::<ArtifactIdentity>(value.clone()))
            .transpose()?;
        let readme_identity = files
            .get("README.md")
            .map(|text| ArtifactIdentity::from_markdown(text))
            .transpose()?
            .flatten();
        let recorded_identity = batch_ref.as_ref().and_then(|batch| batch.identity.clone());
        let identity = choose_batch_identity(
            recorded_identity,
            progress_identity,
            readme_identity,
            batch_title(batch_ref.as_deref(), &progress, files, directory),
        )?;
        let feature_id = progress
            .get("featureId")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .or_else(|| feature_id_from_directory(directory));
        let parent_uid = feature_id
            .as_deref()
            .map(|id| unique_feature_uid(features, id))
            .transpose()?
            .flatten();
        let identity = attach_parent(identity, parent_uid.as_deref())?;
        let entity = format!("batch:{directory}");
        register(seen, &identity, &entity)?;

        if let Some(markdown) = files.get("README.md") {
            let contents = markdown_with_identity(markdown, &identity)?;
            change(
                &mut changes,
                format!("{directory}/README.md"),
                markdown,
                contents,
            );
        } else if has_story {
            let markdown = format!("# {} — task stories\n", identity.title);
            changes.push(Change {
                path: format!("{directory}/README.md"),
                contents: markdown_with_identity(&markdown, &identity)?,
            });
        }
        if files.contains_key(".koolade-progress.json") && progress.is_object() {
            progress["identity"] = serde_json::to_value(&identity)?;
            let contents = serde_json::to_string_pretty(&progress)?;
            if files.get(".koolade-progress.json") != Some(&contents) {
                changes.push(Change {
                    path: format!("{directory}/.koolade-progress.json"),
                    contents,
                });
            }
        }
        if let Some(batch) = batch_ref {
            batch.identity = Some(identity.clone());
        }

        for (name, markdown) in files.iter_mut() {
            if name == "README.md" || name == "specification.md" || !name.ends_with(".md") {
                continue;
            }
            if !crate::artifacts::task_docs::is_task_story_filename(name)
                && ArtifactIdentity::from_markdown(markdown)?.is_none()
            {
                continue;
            }
            let (suggested_id, title) = task_title(markdown, name);
            let (contents, task_identity) =
                preserve_markdown(markdown, &suggested_id, title, Some(&identity.uid))?;
            let path = format!("{directory}/{name}");
            register(seen, &task_identity, &format!("task:{path}"))?;
            task_uids.insert(path.clone(), task_identity.uid);
            change(&mut changes, path, markdown, contents);
        }
    }

    let workflow_changed = workflow_before.as_deref().map_or_else(
        || workflow != Workflow::default(),
        |before| serde_json::from_str::<Workflow>(before).ok().as_ref() != Some(&workflow),
    );
    if workflow_changed {
        changes.insert(
            0,
            Change {
                path: workflow_path.into(),
                contents: serde_json::to_string_pretty(&workflow)?,
            },
        );
    }
    Ok((task_uids, changes))
}
