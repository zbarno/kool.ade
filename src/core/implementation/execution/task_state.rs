use super::super::*;
use super::{initial_base, source};
use std::path::Path;

pub(super) struct Request<'a> {
    pub(super) planning_root: &'a Path,
    pub(super) repo: &'a Path,
    pub(super) ticket: &'a str,
    pub(super) text: &'a str,
    pub(super) task_uid: Option<&'a str>,
    pub(super) metadata: &'a Option<crate::artifacts::task_docs::TaskMetadata>,
    pub(super) publication_mode: PublicationMode,
    pub(super) dir: &'a Path,
    pub(super) runner: &'a Runner,
}

pub(super) fn load_or_create(request: Request<'_>) -> anyhow::Result<Implementation> {
    let Request {
        planning_root,
        repo,
        ticket,
        text,
        task_uid,
        metadata,
        publication_mode,
        dir,
        runner,
    } = request;
    let state = if dir.join("state.json").exists() {
        let mut state = read_state_file(&dir.join("state.json"))?;
        anyhow::ensure!(
            (state.ticket == ticket
                || task_uid.is_some_and(|uid| state.task_uid.as_deref() == Some(uid)))
                && state.ticket_text == text
                && state
                    .task_uid
                    .as_deref()
                    .is_none_or(|uid| task_uid == Some(uid)),
            "Ticket changed since implementation started. Review the existing task repository before starting a revised ticket."
        );
        if state.task_repository_kind == TaskRepositoryKind::LegacyWorktree
            && let Some(source) = state.source_branch.as_deref()
        {
            let local_exists = runner
                .git(
                    repo,
                    &["show-ref", "--verify", &format!("refs/heads/{source}")],
                )
                .is_ok();
            let remote_exists = runner
                .git(
                    repo,
                    &[
                        "ls-remote",
                        "--exit-code",
                        "--heads",
                        "origin",
                        &format!("refs/heads/{source}"),
                    ],
                )
                .is_ok();
            if !local_exists && !remote_exists {
                return Err(
                    crate::core::implementation::initial_reconciliation::support::user_action(
                        format!(
                            "Selected source branch '{source}' no longer exists or cannot be reached. Restore it or select an existing source branch before resuming."
                        ),
                    ),
                );
            }
        }

        if state.task_repository_kind == TaskRepositoryKind::Clone {
            task_repository::validate_task_path(planning_root, &state)?;
            if state.task_repository_allocation_key.is_none() {
                state.task_repository_allocation_key =
                    Some(task_repository::allocation_key(&state));
            }
        }
        state.ticket = ticket.to_owned();
        if state.task_repository_kind == TaskRepositoryKind::Clone {
            let _cache = task_repository::cache_for_state(repo, planning_root, &state, runner)?;
            anyhow::ensure!(
                state.source_ref.is_some() && state.source_commit.is_some(),
                "Saved task source commit is missing; the task repository is preserved"
            );
        }
        if let Some(metadata) = metadata {
            anyhow::ensure!(
                state.source_branch.as_deref() == metadata.source_branch.as_deref()
                    && state.destination_branch.as_deref()
                        == metadata.destination_branch.as_deref(),
                "Task branch intent changed after implementation started. Review the existing task repository before resuming."
            );
        }
        if task_uid.is_some() {
            state.task_uid = task_uid.map(str::to_owned);
        }
        save(dir, &state)?;
        state
    } else {
        let saved_plan = initial_reconciliation::load_plan(dir)?;
        let has_legacy_reconciliation = saved_plan
            .as_ref()
            .is_some_and(|plan| plan.clone_repository.is_none());
        let (
            destination,
            head,
            explicit_source,
            source_ref,
            source_commit,
            repository_id,
            repository_identity,
            repository_cache,
            task_repository_path,
            repository_kind,
        ) = if has_legacy_reconciliation {
            let (destination, head, explicit_source) =
                initial_base::prepare(repo, dir, ticket, metadata, publication_mode, runner)?;
            let task_repository_path = task_repository::legacy_path(repo, ticket)?;
            if let Some(parent) = task_repository_path.parent() {
                fs::create_dir_all(parent)?;
            }
            (
                destination,
                head,
                explicit_source,
                None,
                None,
                None,
                None,
                None,
                task_repository_path,
                TaskRepositoryKind::LegacyWorktree,
            )
        } else {
            let source = match saved_plan.as_ref() {
                Some(plan) => source::resume_from_plan(
                    planning_root,
                    repo,
                    ticket,
                    metadata.as_ref(),
                    plan,
                    runner,
                )?,
                None => source::resolve(source::Request {
                    planning_root,
                    repo,
                    dir,
                    ticket,
                    text,
                    metadata: metadata.as_ref(),
                    publication_mode,
                    runner,
                })?,
            };
            let task_repository_path =
                task_repository::task_path(planning_root, &source.repository_id, ticket)?;
            (
                source.destination_branch,
                source.base_commit.clone(),
                metadata
                    .as_ref()
                    .and_then(|metadata| metadata.source_branch.clone()),
                Some(source.source_ref),
                Some(source.source_commit),
                Some(source.repository_id),
                Some(source.cache.identity),
                Some(source.cache.path),
                task_repository_path,
                TaskRepositoryKind::Clone,
            )
        };
        let project_id = if repository_kind == TaskRepositoryKind::Clone {
            Some(task_repository::project_id(planning_root)?)
        } else {
            None
        };
        let dependency_context = completed_dependency_context(planning_root, ticket, text)?;
        Implementation {
            ticket: ticket.into(),
            task_uid: task_uid.map(str::to_owned),
            ticket_text: text.to_owned(),
            approved_specification: Path::new(ticket).parent().and_then(|parent| {
                fs::read_to_string(planning_root.join(parent).join("specification.md")).ok()
            }),
            approved_product_context: scoped_product_context(planning_root, ticket)?,
            completed_dependency_context: dependency_context,
            branch: format!("koolade/{}", key(ticket)),
            source_branch: explicit_source,
            source_ref,
            source_commit,
            repository_id,
            project_id,
            repository_identity,
            repository_cache,
            task_repository_allocation_key: Some(key(ticket)),
            destination_branch: metadata
                .as_ref()
                .and_then(|metadata| metadata.destination_branch.clone()),
            base: destination,
            base_commit: head,
            task_repository_ready: false,
            task_repositories: vec![task_repository_path.clone()],
            task_repository_commits: std::collections::BTreeMap::new(),
            task_repository: task_repository_path,
            task_repository_kind: repository_kind,
            status: ImplementationStatus::Preparing,
            detail: String::new(),
            pr_url: None,
            verified_head: None,
            auto_merge: publication_mode == PublicationMode::AutoPublish,
            merged_commit: None,
            pr_state: None,
            pr_checked_at: None,
            pr_check_attempted_at: None,
            pr_check_error: None,
            independent_check: None,
            cleanup: Default::default(),
        }
    };

    if state.task_repository_kind == TaskRepositoryKind::Clone {
        crate::core::implementation::repository_cache::load_task_git_identity(dir, repo, runner)?;
    }

    Ok(state)
}
