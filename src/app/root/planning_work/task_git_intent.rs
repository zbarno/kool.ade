use crate::{
    app::session::Project,
    artifacts::planning_store::PlanningRoot,
    core::git_intent::{GitIntentSelectionV1, GitIntentV1},
};

pub(super) fn approve(
    project: &mut Project,
    ticket: &str,
    repository_id: &str,
    selection: GitIntentSelectionV1,
) -> Result<(), String> {
    if project.implementation_states.contains_key(ticket)
        || project.active_implementations.contains_key(ticket)
    {
        return Err("Task Git intent is immutable after implementation starts.".into());
    }
    let index = project
        .task_documents
        .iter()
        .position(|document| document.path == ticket)
        .ok_or_else(|| "Task is no longer available.".to_owned())?;
    let current = project.task_documents[index]
        .metadata
        .as_ref()
        .ok_or_else(|| "Task metadata is unavailable for Git intent approval.".to_owned())?;
    current
        .validate(None)
        .map_err(|error| format!("Task Git intent cannot be reviewed: {error}"))?;
    super::git_intent::validate_selection(
        project,
        repository_id,
        &selection.source_ref,
        &selection.destination_ref,
    )
    .map_err(|error| format!("Cannot approve Git intent for {repository_id}: {error}"))?;

    let manifest = crate::core::project_repos::ProjectManifest::load(&project.state.planning_store)
        .map_err(|error| error.to_string())?;
    let checkout = manifest
        .target(
            &project.state.planning_store,
            &project.state.repo_root,
            repository_id,
        )
        .map_err(|error| error.to_string())?;
    let source_sha = super::git_intent::pin_source_sha(&checkout, &selection.source_ref)
        .map_err(|error| error.to_string())?;
    let inherited_from = current.parent_task_uid.as_deref().filter(|parent_uid| {
        project
            .task_documents
            .iter()
            .filter_map(|document| document.metadata.as_ref())
            .find(|parent| parent.uid == *parent_uid)
            .and_then(|parent| parent.git_intent.as_ref())
            .is_some_and(|parent| {
                parent.repository_id == repository_id
                    && parent.source_ref == selection.source_ref
                    && parent.source_sha == source_sha
                    && parent.destination_ref == selection.destination_ref
                    && parent.publication_mode == selection.publication_mode
            })
    });
    let intent = GitIntentV1::approve(
        repository_id.to_owned(),
        selection.source_ref,
        source_sha,
        selection.destination_ref,
        selection.publication_mode,
        inherited_from.map(str::to_owned),
    )
    .map_err(|error| error.to_string())?;
    let updated = current
        .clone()
        .with_git_intent(Some(&intent))
        .map_err(|error| error.to_string())?;
    updated
        .validate_for_execution()
        .map_err(|error| format!("Approved task Git intent is incomplete: {error}"))?;

    let store = project.state.planning_store.clone();
    let layout = store.planning_layout();
    let path = layout
        .canonical_path(ticket)
        .ok_or_else(|| "Task path is outside the private planning store.".to_owned())?;
    let file_type = std::fs::symlink_metadata(&path)
        .map_err(|error| format!("Cannot inspect task file: {error}"))?;
    if file_type.file_type().is_symlink() {
        return Err("Refusing to replace a linked task file.".into());
    }
    let markdown = String::from_utf8(
        store
            .read_planning_path(&path)
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let updated_markdown = crate::artifacts::task_docs::embed(&markdown, &updated)
        .map_err(|error| error.to_string())?;
    let revision = store.revision().map_err(|error| error.to_string())?;
    let (_, next_revision) = store
        .transaction_with_revision(
            &[(ticket.to_owned(), updated_markdown.as_bytes().to_vec())],
            Some(&revision),
        )
        .map_err(|error| error.to_string())?;

    project.state.baseline_planning_revision = next_revision;
    project.task_documents[index].metadata = Some(updated);
    project.task_documents[index].text =
        crate::artifacts::task_docs::visible_content(&updated_markdown);
    Ok(())
}
