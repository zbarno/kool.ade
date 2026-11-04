use std::path::Path;

type GitIntent = crate::core::git_intent::GitIntentV1;

impl super::KooladeApp {
    pub(crate) fn approve_feature_git_intents(
        &mut self,
        work_key: &str,
        feature_id: &str,
        selections: std::collections::BTreeMap<
            String,
            crate::core::git_intent::GitIntentSelectionV1,
        >,
    ) -> Result<(), String> {
        (|| -> anyhow::Result<()> {
            let super::Screen::Connected(project) = &mut self.screen else {
                anyhow::bail!("Connect a project before approving repository Git intent.");
            };
            let index = project
                .planning_work
                .iter()
                .position(|work| {
                    work.key == work_key
                        && work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                        && work.feature_id.as_deref() == Some(feature_id)
                })
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "The task-generation work item is no longer available for this feature"
                    )
                })?;
            anyhow::ensure!(
                !selections.is_empty(),
                "Select at least one implementation repository"
            );
            let mut intents = std::collections::BTreeMap::new();
            for (repository_id, selection) in selections {
                validate_selection(
                    project,
                    &repository_id,
                    &selection.source_ref,
                    &selection.destination_ref,
                )?;
                let intent = approve_for_work(
                    project,
                    &repository_id,
                    &selection.source_ref,
                    &selection.destination_ref,
                    selection.publication_mode,
                    None,
                    None,
                )?;
                intents.insert(repository_id, intent);
            }
            let previous = project.planning_work[index].clone();
            let primary = intents.get("root").cloned();
            project.planning_work[index].source_branch =
                primary.as_ref().map(|intent| intent.source_ref.clone());
            project.planning_work[index].destination_branch = primary
                .as_ref()
                .map(|intent| intent.destination_ref.clone());
            project.planning_work[index].git_intent = primary;
            project.planning_work[index].git_intents = intents;
            if let Err(error) = project.save_planning_work() {
                project.planning_work[index] = previous;
                return Err(anyhow::anyhow!(error));
            }
            Ok(())
        })()
        .map_err(|error| error.to_string())
    }
}

pub(super) fn inherit_selections(
    parent: Option<&std::collections::BTreeMap<String, GitIntent>>,
    mut selected: std::collections::BTreeMap<String, crate::core::git_intent::GitIntentSelectionV1>,
) -> std::collections::BTreeMap<String, crate::core::git_intent::GitIntentSelectionV1> {
    if let Some(parent) = parent {
        for (repository_id, intent) in parent {
            selected.entry(repository_id.clone()).or_insert_with(|| {
                crate::core::git_intent::GitIntentSelectionV1 {
                    source_ref: intent.source_ref.clone(),
                    destination_ref: intent.destination_ref.clone(),
                    publication_mode: intent.publication_mode,
                }
            });
        }
    }
    selected
}

pub(super) fn approve_for_work(
    project: &crate::app::session::Project,
    repository_id: &str,
    source_ref: &str,
    destination_ref: &str,
    publication_mode: crate::core::git_intent::PublicationModeV1,
    inherited: Option<&GitIntent>,
    parent_uid: Option<String>,
) -> anyhow::Result<GitIntent> {
    let checkout = project.state.repositories.target(
        &project.state.planning_store,
        &project.state.repo_root,
        repository_id,
    )?;
    validate_selection_checkout(&checkout, source_ref, destination_ref)?;
    let inherited_match = inherited.filter(|intent| {
        intent.repository_id == repository_id
            && intent.source_ref == source_ref
            && intent.destination_ref == destination_ref
            && intent.publication_mode == publication_mode
    });
    let source_sha = if let Some(intent) = inherited_match {
        intent.source_sha.clone()
    } else {
        pin_source_sha(&checkout, source_ref)?
    };
    GitIntent::approve(
        repository_id.to_owned(),
        source_ref.to_owned(),
        source_sha,
        destination_ref.to_owned(),
        publication_mode,
        inherited_match.and(parent_uid),
    )
}

pub(super) fn validate_selection(
    project: &crate::app::session::Project,
    repository_id: &str,
    source_ref: &str,
    destination_ref: &str,
) -> anyhow::Result<()> {
    let manifest =
        crate::core::project_repos::ProjectManifest::load(&project.state.planning_store)?;
    let checkout = manifest.target(
        &project.state.planning_store,
        &project.state.repo_root,
        repository_id,
    )?;
    validate_selection_checkout(&checkout, source_ref, destination_ref)
}

pub(super) fn validate_approved_intent(
    project: &crate::app::session::Project,
    feature_uid: Option<&str>,
    repository_id: &str,
    intent: &GitIntent,
) -> anyhow::Result<()> {
    intent.validate()?;
    anyhow::ensure!(
        intent.repository_id == repository_id,
        "Git intent repository ID does not match its repository map entry"
    );
    validate_selection(
        project,
        repository_id,
        &intent.source_ref,
        &intent.destination_ref,
    )?;
    if let Some(run) = feature_uid
        .map(|uid| {
            crate::core::self_improvement::SelfImprovementStore::new(&project.state.planning_store)
                .run_for_feature(uid)
        })
        .transpose()?
        .flatten()
    {
        let workspace_uid = project
            .state
            .planning_store
            .project_id
            .hyphenated()
            .to_string();
        let manifest =
            crate::core::project_repos::ProjectManifest::load(&project.state.planning_store)?;
        let repository = manifest
            .repositories
            .iter()
            .find(|repository| repository.id == repository_id);
        let is_pursued_repository = repository.is_some_and(|repository| {
            crate::core::project_repos::stable_uid(&workspace_uid, "repository", &repository.id)
                == run.target_repository_uid
        });
        if is_pursued_repository {
            anyhow::ensure!(
                run.source_ref
                    .as_deref()
                    .is_none_or(|source_ref| source_ref == intent.source_ref)
                    && run.source_sha == intent.source_sha,
                "Self-improvement Git intent must retain its immutable pursued source branch and commit"
            );
            return Ok(());
        }
    }
    let checkout = project.state.repositories.target(
        &project.state.planning_store,
        &project.state.repo_root,
        repository_id,
    )?;
    let current = pin_source_sha(&checkout, &intent.source_ref)?;
    anyhow::ensure!(
        current == intent.source_sha,
        "Approved source branch moved after Git intent approval; review and approve the updated source commit"
    );
    Ok(())
}

fn validate_selection_checkout(
    checkout: &Path,
    source_ref: &str,
    destination_ref: &str,
) -> anyhow::Result<()> {
    validate_ref(checkout, source_ref)?;
    validate_ref(checkout, destination_ref)?;
    let source_local = resolve(checkout, &format!("refs/heads/{source_ref}^{{commit}}"));
    let source_advertised = remote_head(checkout, source_ref)?;
    anyhow::ensure!(
        source_local.is_some() || source_advertised.is_some(),
        "Selected source branch has no resolvable ref in this repository"
    );

    let has_origin = crate::core::gitops::origin_url(checkout).is_some();
    let destination_exists = if has_origin {
        remote_head(checkout, destination_ref)?.is_some()
    } else {
        resolve(
            checkout,
            &format!("refs/heads/{destination_ref}^{{commit}}"),
        )
        .is_some()
    };
    anyhow::ensure!(
        destination_exists,
        "Selected destination branch has no verified target in this repository"
    );
    Ok(())
}

pub(super) fn pin_source_sha(repository: &Path, branch: &str) -> anyhow::Result<String> {
    validate_ref(repository, branch)?;
    if let Some(local) = resolve(repository, &format!("refs/heads/{branch}^{{commit}}")) {
        return Ok(local);
    }
    remote_head(repository, branch)?
        .ok_or_else(|| anyhow::anyhow!("Selected source branch has no resolvable commit"))
}

#[cfg(test)]
#[path = "git_intent_tests.rs"]
mod tests;

fn validate_ref(repository: &Path, branch: &str) -> anyhow::Result<()> {
    let output = git(repository, &["check-ref-format", "--branch", branch])?;
    anyhow::ensure!(output.0, "Selected branch ref is invalid");
    Ok(())
}

fn remote_head(repository: &Path, branch: &str) -> anyhow::Result<Option<String>> {
    if crate::core::gitops::origin_url(repository).is_none() {
        return Ok(None);
    }
    let reference = format!("refs/heads/{branch}");
    let (_, output) = git(repository, &["ls-remote", "--heads", "origin", &reference])?;
    Ok(output.lines().next().and_then(|line| {
        let sha = line.split_whitespace().next()?;
        valid_sha(sha).then(|| sha.to_owned())
    }))
}

fn resolve(repository: &Path, reference: &str) -> Option<String> {
    let (_, output) = git(repository, &["rev-parse", "--verify", reference]).ok()?;
    output
        .split_whitespace()
        .next()
        .filter(|sha| valid_sha(sha))
        .map(str::to_owned)
}

fn git(repository: &Path, args: &[&str]) -> anyhow::Result<(bool, String)> {
    let output = crate::core::git_commands::validated_git_command(repository)?
        .args(args)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.first().copied().unwrap_or_default(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok((true, stdout))
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
