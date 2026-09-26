use super::*;

#[cfg(test)]
pub(super) fn read_ticket(repo: &Path, ticket: &str) -> anyhow::Result<String> {
    read_ticket_and_identity(repo, ticket).map(|(text, _, _)| text)
}

pub(super) fn ticket_identity(repo: &Path, ticket: &str) -> anyhow::Result<Option<String>> {
    read_ticket_and_identity(repo, ticket).map(|(_, identity, _)| identity)
}

pub(super) fn read_ticket_and_identity(
    repo: &Path,
    ticket: &str,
) -> anyhow::Result<(
    String,
    Option<String>,
    Option<crate::artifacts::task_docs::TaskMetadata>,
)> {
    anyhow::ensure!(
        crate::artifacts::layout::ArtifactLayout::is_task_ticket_path(ticket)
            && Path::new(ticket)
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(crate::artifacts::task_docs::is_task_story_filename),
        "Select a generated task story (numbered or feature-ID filename)"
    );
    anyhow::ensure!(
        Path::new(ticket)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_))),
        "Invalid ticket path"
    );
    let path = repo.join(ticket).canonicalize()?;
    anyhow::ensure!(
        path.starts_with(repo.canonicalize()?),
        "Ticket is outside the repository"
    );
    let raw = fs::read_to_string(path)?;
    let artifact_identity = crate::domain::ArtifactIdentity::from_markdown(&raw)?;
    let identity = artifact_identity.as_ref().map(|id| id.uid.clone());
    let metadata = crate::artifacts::task_docs::parse_metadata(&raw)?;
    if let Some(metadata) = &metadata {
        anyhow::ensure!(
            artifact_identity.is_some(),
            "Task metadata has no Packet artifact identity"
        );
        metadata.validate(artifact_identity.as_ref())?;
    }
    let text = crate::artifacts::task_docs::visible_content(&raw);
    anyhow::ensure!(!text.trim().is_empty(), "Ticket is empty");
    Ok((text, identity, metadata))
}

pub fn target_repository(planning_root: &Path, ticket: &str) -> anyhow::Result<PathBuf> {
    let (text, _, metadata) = read_ticket_and_identity(planning_root, ticket)?;
    let manifest = crate::core::project_repos::ProjectManifest::load(planning_root)?;
    let id = task_repository_id(&text, metadata.as_ref(), &manifest)?;
    manifest.target(planning_root, &id)
}

pub(super) fn task_repository_id(
    text: &str,
    metadata: Option<&crate::artifacts::task_docs::TaskMetadata>,
    manifest: &crate::core::project_repos::ProjectManifest,
) -> anyhow::Result<String> {
    match metadata {
        Some(metadata) => Ok(metadata.repository_id.clone()),
        None => legacy_repository_target(text, manifest),
    }
}

fn legacy_repository_target(
    text: &str,
    manifest: &crate::core::project_repos::ProjectManifest,
) -> anyhow::Result<String> {
    let mut target = None;
    for line in text.lines().take(40) {
        if let Some(value) = line.strip_prefix("Repository: ") {
            anyhow::ensure!(
                target.is_none(),
                "Legacy task has more than one repository target"
            );
            target = Some(value.trim().to_owned());
        }
    }
    anyhow::ensure!(
        target.is_some() || manifest.repositories.len() == 1,
        "Multi-repository legacy task lacks a repository target"
    );
    Ok(target.unwrap_or_else(|| "root".into()))
}

pub(super) fn scoped_product_context(
    planning_root: &Path,
    ticket: &str,
) -> anyhow::Result<Option<String>> {
    let Some(parent) = Path::new(ticket).parent() else {
        return Ok(None);
    };
    let contract_path = planning_root.join(parent).join("contract.json");
    let bytes = match fs::read(&contract_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let contract: crate::core::contract_snapshot::BatchContract = serde_json::from_slice(&bytes)?;
    let mut context = String::new();
    for (id, body) in contract.product_modules {
        context.push_str(&format!("\n=== product:{id} ===\n{body}\n"));
    }
    Ok(Some(context))
}

/// Collect objective history evidence for explicit commit references in a
/// ticket before a worker starts. Exact-footprint requirements often name an
/// older checkpoint; this makes the changes already present at the task base
/// visible and reviewable before implementation or recovery begins.
pub fn completed_dependency_context(
    planning_root: &Path,
    ticket: &str,
    ticket_text: &str,
) -> anyhow::Result<Option<String>> {
    let (current_text, _, metadata) = read_ticket_and_identity(planning_root, ticket)?;
    anyhow::ensure!(
        current_text == ticket_text,
        "Task changed while resolving dependencies"
    );
    let dependencies = if let Some(metadata) = metadata {
        let workflow = crate::artifacts::task_docs::load_workflow(planning_root)?;
        let documents = crate::artifacts::task_docs::load_board(planning_root, &workflow);
        metadata
            .dependency_uids
            .iter()
            .map(|uid| {
                let dependency = documents
                    .iter()
                    .find(|doc| doc.identity.as_ref().is_some_and(|id| &id.uid == uid))
                    .ok_or_else(|| {
                        anyhow::anyhow!("Dependency identity {uid} is not on the task board")
                    })?;
                let dependency_metadata = dependency.metadata.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("Dependency {} lacks Packet task metadata", dependency.path)
                })?;
                anyhow::ensure!(
                    dependency_metadata.batch_uid == metadata.batch_uid,
                    "Task dependency must be in the same batch"
                );
                anyhow::ensure!(
                    dependency.metadata_error.is_none(),
                    "Dependency {} has invalid task metadata",
                    dependency.path
                );
                Ok((dependency.path.clone(), Some(uid.clone())))
            })
            .collect::<anyhow::Result<Vec<_>>>()?
    } else {
        crate::artifacts::task_docs::legacy_dependencies(ticket_text)?
            .into_iter()
            .map(|filename| {
                let path = Path::new(ticket)
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("Task path has no batch directory"))?
                    .join(filename)
                    .to_string_lossy()
                    .into_owned();
                Ok((path, None))
            })
            .collect::<anyhow::Result<Vec<_>>>()?
    };
    let mut context = String::new();
    for (relative, expected_uid) in dependencies {
        anyhow::ensure!(relative.as_str() != ticket, "Task cannot depend on itself");
        let record = load(planning_root, &relative)
            .ok_or_else(|| anyhow::anyhow!("Dependency {relative} has no implementation record"))?;
        if let Some(expected_uid) = expected_uid {
            anyhow::ensure!(
                record.task_uid.as_deref() == Some(expected_uid.as_str()),
                "Dependency {relative} implementation state belongs to another task identity"
            );
        }
        anyhow::ensure!(
            (record.status == ImplementationStatus::Completed
                || record.pr_state == Some(PullRequestState::Merged))
                && record.merged_commit.is_some(),
            "Dependency {relative} has not merged"
        );
        let story = crate::artifacts::task_docs::visible_content(&fs::read_to_string(
            planning_root.join(&relative),
        )?);
        anyhow::ensure!(
            story == record.ticket_text,
            "Dependency {relative} story changed"
        );
        context.push_str(&format!(
            "\n=== Completed dependency {relative} ===\nMerged commit: {}\n{}\n",
            record.merged_commit.as_deref().unwrap(),
            story.chars().take(6_000).collect::<String>()
        ));
    }
    Ok((!context.is_empty()).then_some(context))
}

pub(super) fn title(ticket: &str) -> String {
    ticket
        .lines()
        .next()
        .unwrap_or("Implement ticket")
        .trim_start_matches('#')
        .trim()
        .chars()
        .take(200)
        .collect()
}
pub(super) fn specification_matches_task(ticket: &str, specification: &str) -> bool {
    let feature = ticket
        .lines()
        .find_map(|line| line.strip_prefix("Feature: "));
    let title = specification
        .lines()
        .find_map(|line| line.strip_prefix("# "));
    match (feature, title) {
        (Some(feature), Some(title)) => title
            .to_ascii_lowercase()
            .contains(&feature.to_ascii_lowercase()),
        _ => true,
    }
}

/// Recognize the deliberately narrow contract used by audit/demonstration
/// tickets. Requiring all three independent statements prevents an ordinary
/// implementation task from turning a no-op report into a successful result.
pub(crate) fn permits_evidence_only_completion(ticket: &str) -> bool {
    let contract = ticket.to_ascii_lowercase();
    [
        "this ticket lands zero planner code",
        "explicitly unchanged",
        "no product-repo file may be created or modified by this ticket",
    ]
    .iter()
    .all(|statement| contract.contains(statement))
        || ([
            "pure verification",
            "commits no bytes",
            "this ticket itself changed no repository file",
        ]
        .iter()
        .all(|statement| contract.contains(statement)))
}
