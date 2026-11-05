use super::super::*;
use super::repositories::ApiRepository;
use super::{install_mapped_gh, run_mapped_task, set_intent};

pub struct ActiveTasks {
    pub root: Implementation,
    pub api: Implementation,
    pub unrelated: Implementation,
    pub root_calls: Arc<AtomicUsize>,
    pub api_calls: Arc<AtomicUsize>,
    pub unrelated_calls: Arc<AtomicUsize>,
    pub api_ticket: &'static str,
}

pub struct PublishedTasks {
    pub root: Implementation,
    pub api: Implementation,
}

pub fn start_tasks(sandbox: &Sandbox, api_source_sha: &str) -> ActiveTasks {
    let root_source_sha = sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);
    let api_ticket = "planning/tasks/feature/002-api-mapped.md";
    super::super::super::sandbox::write_lifecycle_task_with_main_intent(
        sandbox,
        api_ticket,
        "# API task\n\nFeature: F1\n\n## Acceptance criteria\n\n- File contains implemented.\n",
        "TASK-002",
        "API mapped task",
    );
    let unrelated_ticket = "planning/tasks/feature/003-unrelated.md";
    super::super::super::sandbox::write_lifecycle_task_with_main_intent(
        sandbox,
        unrelated_ticket,
        "# Unrelated task\n\nFeature: F1\n\n## Acceptance criteria\n\n- File contains implemented.\n",
        "TASK-003",
        "Unrelated task",
    );
    set_intent(
        sandbox,
        &sandbox.ticket,
        "root",
        "main",
        "main",
        &root_source_sha,
    );
    set_intent(sandbox, api_ticket, "api", "main", "stable", api_source_sha);

    let root_calls = Arc::new(AtomicUsize::new(0));
    let api_calls = Arc::new(AtomicUsize::new(0));
    let unrelated_calls = Arc::new(AtomicUsize::new(0));
    for ticket in [&sandbox.ticket, api_ticket, unrelated_ticket] {
        refresh_task_spec(&sandbox.planning_store, ticket);
    }
    let (root, api, unrelated) = std::thread::scope(|scope| {
        let root_worker = scope.spawn(|| {
            let _executor = super::super::super::sandbox::install_verification_executor();
            run_mapped_task(
                sandbox,
                &sandbox.ticket,
                root_calls.clone(),
                PublicationMode::HoldForReview,
            )
        });
        let api_worker = scope.spawn(|| {
            let _executor = super::super::super::sandbox::install_verification_executor();
            run_mapped_task(
                sandbox,
                api_ticket,
                api_calls.clone(),
                PublicationMode::HoldForReview,
            )
        });
        let unrelated_worker = scope.spawn(|| {
            let _executor = super::super::super::sandbox::install_verification_executor();
            run_mapped_task(
                sandbox,
                unrelated_ticket,
                unrelated_calls.clone(),
                PublicationMode::HoldForReview,
            )
        });
        (
            root_worker.join().unwrap().unwrap(),
            api_worker.join().unwrap().unwrap(),
            unrelated_worker.join().unwrap().unwrap(),
        )
    });
    ActiveTasks {
        root,
        api,
        unrelated,
        root_calls,
        api_calls,
        unrelated_calls,
        api_ticket,
    }
}

pub fn publish_tasks(
    sandbox: &Sandbox,
    root_remote: &Path,
    api_repository: &ApiRepository,
    active: &ActiveTasks,
) -> PublishedTasks {
    install_mapped_gh(sandbox, root_remote, &api_repository.remote);
    super::super::super::super::publication_checks::authorize_fixture_publication(
        sandbox,
        &active.root,
    );
    let root = run_mapped_task(
        sandbox,
        &sandbox.ticket,
        active.root_calls.clone(),
        PublicationMode::CreatePullRequest,
    )
    .unwrap();
    super::super::super::super::publication_checks::authorize_fixture_publication(
        sandbox,
        &active.api,
    );
    let api = run_mapped_task(
        sandbox,
        active.api_ticket,
        active.api_calls.clone(),
        PublicationMode::CreatePullRequest,
    )
    .unwrap();
    assert_eq!(active.root_calls.load(Ordering::SeqCst), 1);
    assert_eq!(active.api_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        root.status,
        ImplementationStatus::AwaitingReview,
        "{}",
        root.detail
    );
    assert_eq!(
        api.status,
        ImplementationStatus::AwaitingReview,
        "{}",
        api.detail
    );
    assert_eq!(root.task_repository, active.root.task_repository);
    assert_eq!(api.task_repository, active.api.task_repository);
    let log = fs::read_to_string(sandbox.root.join("mapped-pr-log")).unwrap();
    assert!(log.contains(&format!(
        "github.com/fixture/repo\t{}\tmain",
        active.root.branch
    )));
    assert!(log.contains(&format!(
        "github.com/fixture/api\t{}\tstable",
        active.api.branch
    )));
    PublishedTasks { root, api }
}
