use super::TaskDocument;
use super::identity;
use super::naming::is_task_story_filename;
use super::progress::progress_batches;
use crate::artifacts::planning_store::PlanningRoot;
use crate::core::workflow::{TaskBatchRef, Workflow};

pub fn load_latest<R: PlanningRoot + ?Sized>(repo: &R, workflow: &Workflow) -> Vec<TaskDocument> {
    let layout = repo.planning_layout();
    let completed_time = workflow
        .task_batches
        .last()
        .and_then(|batch| identity::resolve_batch_directory(repo, batch))
        .and_then(|directory| layout.canonical_path(&directory))
        .and_then(|directory| std::fs::metadata(directory.join("README.md")).ok())
        .and_then(|m| m.modified().ok());
    let pending = progress_batches(repo).into_iter().rfind(|(directory, _)| {
        !workflow
            .task_batches
            .iter()
            .any(|b| &b.directory == directory)
            && layout
                .canonical_path(directory)
                .and_then(|path| std::fs::metadata(path.join("README.md")).ok())
                .and_then(|m| m.modified().ok())
                > completed_time
    });
    let pending_ref = pending.as_ref().map(|(directory, p)| TaskBatchRef {
        identity: p.identity.clone().or_else(|| {
            layout.canonical_path(directory).and_then(|path| {
                identity::read_identity_in(repo, &path.join("README.md"))
                    .ok()
                    .flatten()
            })
        }),
        feature: p.brief.feature_name.clone(),
        directory: directory.clone(),
        count: p.stories.len(),
    });
    let Some(batch) = pending_ref
        .as_ref()
        .or_else(|| workflow.task_batches.last())
    else {
        return Vec::new();
    };
    load_batch(repo, batch, pending.is_some())
}

/// The board accounts for every batch, including interrupted generation.
pub fn load_board<R: PlanningRoot + ?Sized>(repo: &R, workflow: &Workflow) -> Vec<TaskDocument> {
    let mut docs = Vec::new();
    for batch in &workflow.task_batches {
        docs.extend(load_batch(repo, batch, false));
    }
    for (directory, p) in progress_batches(repo) {
        if !workflow
            .task_batches
            .iter()
            .any(|b| b.directory == directory)
        {
            docs.extend(load_batch(
                repo,
                &TaskBatchRef {
                    identity: p.identity.clone().or_else(|| {
                        repo.planning_layout()
                            .canonical_path(&directory)
                            .and_then(|path| {
                                identity::read_identity_in(repo, &path.join("README.md"))
                                    .ok()
                                    .flatten()
                            })
                    }),
                    feature: p.brief.feature_name,
                    directory,
                    count: p.stories.len(),
                },
                true,
            ));
        }
    }
    docs.sort_by(|a, b| a.path.cmp(&b.path));
    docs.dedup_by(|a, b| a.path == b.path);
    docs
}

fn load_batch<R: PlanningRoot + ?Sized>(
    repo: &R,
    batch: &TaskBatchRef,
    pending: bool,
) -> Vec<TaskDocument> {
    // Only app-generated directory names may be read from the metadata.
    let Some(directory) = identity::resolve_batch_directory(repo, batch) else {
        return Vec::new();
    };
    let layout = repo.planning_layout();
    let Ok(canonical_repo) = layout.root().canonicalize() else {
        return Vec::new();
    };
    let Some(path) = layout.canonical_path(&directory) else {
        return Vec::new();
    };
    let Ok(canonical_dir) = path.canonicalize() else {
        return Vec::new();
    };
    if !canonical_dir.starts_with(&canonical_repo) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(canonical_dir) else {
        return Vec::new();
    };
    let mut docs = Vec::new();
    for e in entries.flatten() {
        let filename = e.file_name().to_string_lossy().into_owned();
        if filename == "README.md"
            || filename == "specification.md"
            || !e.file_type().is_ok_and(|t| t.is_file())
        {
            continue;
        }
        if let Ok(bytes) = repo.read_planning_path(&e.path())
            && let Ok(text) = String::from_utf8(bytes)
        {
            if !is_task_story_filename(&filename)
                && !crate::domain::ArtifactIdentity::from_markdown(&text)
                    .ok()
                    .flatten()
                    .is_some()
            {
                continue;
            }
            let identity = crate::domain::ArtifactIdentity::from_markdown(&text)
                .ok()
                .flatten();
            let (metadata, metadata_error) =
                match super::metadata::parse(&text).and_then(|metadata| {
                    if let Some(metadata) = &metadata {
                        metadata.validate(identity.as_ref())?;
                    }
                    Ok(metadata)
                }) {
                    Ok(metadata) => (metadata, None),
                    Err(error) => (None, Some(error.to_string())),
                };
            let visible = super::visible_content(&text);
            let title = visible
                .lines()
                .next()
                .unwrap_or(&filename)
                .trim_start_matches("# ")
                .to_owned();
            docs.push(TaskDocument {
                path: format!("{directory}/{filename}"),
                title,
                text: visible,
                identity,
                metadata,
                metadata_error,
            });
        }
    }
    docs.sort_by(|a, b| a.path.cmp(&b.path));
    if pending
        && let Some(path) = layout.canonical_path(&batch.directory)
        && let Ok(bytes) = repo.read_planning_path(&path.join("README.md"))
        && let Ok(text) = String::from_utf8(bytes)
    {
        let identity = crate::domain::ArtifactIdentity::from_markdown(&text)
            .ok()
            .flatten();
        docs.insert(
            0,
            TaskDocument {
                path: format!("{directory}/README.md"),
                title: format!("In progress — {} stories saved", batch.count),
                text: super::visible_content(&text),
                identity,
                metadata: None,
                metadata_error: None,
            },
        );
    }
    docs
}
