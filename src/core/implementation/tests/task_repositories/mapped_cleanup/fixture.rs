use super::*;

#[path = "fixture/merge.rs"]
mod merge;
#[path = "fixture/repositories.rs"]
mod repositories;
#[path = "fixture/tasks.rs"]
mod tasks;

pub(super) use merge::confirm_merges_and_fast_forward;
pub(super) use repositories::{install_mapped_gh, refs, refs_after_merge, setup_api_repository};
pub(super) use tasks::{publish_tasks, start_tasks};

pub(super) fn set_intent(
    sandbox: &Sandbox,
    ticket: &str,
    repository: &str,
    source: &str,
    destination: &str,
    sha: &str,
) {
    let raw = String::from_utf8(sandbox.planning_store.read(ticket).unwrap()).unwrap();
    let mut metadata = crate::artifacts::task_docs::parse_metadata(&raw)
        .unwrap()
        .unwrap();
    metadata.repository_id = repository.into();
    metadata.source_branch = Some(source.into());
    metadata.destination_branch = Some(destination.into());
    metadata.source_sha = Some(sha.into());
    metadata.git_intent = Some(
        crate::core::git_intent::GitIntentV1::approve(
            repository.into(),
            source.into(),
            sha.into(),
            destination.into(),
            crate::core::git_intent::PublicationModeV1::CreatePullRequest,
            None,
        )
        .unwrap(),
    );
    let updated = crate::artifacts::task_docs::embed(&raw, &metadata).unwrap();
    sandbox
        .planning_store
        .atomic_write(ticket, updated.as_bytes())
        .unwrap();
}

pub(super) fn run_mapped_task(
    sandbox: &Sandbox,
    ticket: &str,
    calls: Arc<AtomicUsize>,
    publication_mode: PublicationMode,
) -> anyhow::Result<Implementation> {
    refresh_task_spec(&sandbox.planning_store, ticket);
    let repository = crate::core::implementation::target_repository_with_store(
        &sandbox.planning_store,
        &sandbox.repo,
        ticket,
    )?;
    let (progress, _updates) = mpsc::channel();
    run_with_code_root_and_task_repo_options(
        &sandbox.planning_store,
        &sandbox.planning_store.root,
        &sandbox.repo,
        &repository,
        ticket,
        RunOptions {
            harness: &Fixture {
                mode: "complete",
                calls,
            },
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
            gh: sandbox.gh.to_str().unwrap(),
            publication_mode,
            require_independent_checks: false,
            user_context: None,
            auto_publish_gate: None,
            claim_lease: None,
            computer_use: None,
        },
    )
}
