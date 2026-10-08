use super::*;

#[test]
fn cancelled_task_clone_is_reviewed_and_resumed() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(s.run("cancel", calls.clone()).is_err());
    let state = load(&s.repo, &s.ticket).unwrap();
    assert_eq!(state.task_repository_kind, TaskRepositoryKind::Clone);
    assert!(state.task_repository.join(".git").is_dir());
    assert_eq!(
        fs::read_to_string(state.task_repository.join("implemented.txt")).unwrap(),
        "partial\n"
    );
    assert_eq!(state.status, ImplementationStatus::Interrupted);
    assert!(!s.root.join("pr-created").exists());
    s.advance_remote();
    let resumed = s
        .run("resume", calls.clone())
        .unwrap_or_else(|error| panic!("{error:#}"));
    assert_eq!(resumed.task_repository, state.task_repository);
    assert_eq!(resumed.source_commit, state.source_commit);
    assert_eq!(resumed.base_commit, state.base_commit);
    assert!(!resumed.task_repository.join("upstream.txt").exists());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn active_task_clone_resumes_after_ticket_rename() {
    let mut s = Sandbox::new();
    let original = fs::read_to_string(s.repo.join(&s.ticket)).unwrap();
    let identified = crate::domain::ArtifactIdentity::preserve_markdown(
        &original,
        None,
        "TASK-001",
        "Implement ticket behavior",
    )
    .unwrap();
    fs::write(s.repo.join(&s.ticket), identified).unwrap();
    s.git(&s.repo, &["add", &s.ticket]);
    s.git(&s.repo, &["commit", "-qm", "identify task ticket"]);
    s.git(&s.repo, &["push", "-q", "origin", "main"]);

    let calls = Arc::new(AtomicUsize::new(0));
    assert!(s.run("cancel", calls.clone()).is_err());
    let state = load(&s.repo, &s.ticket).unwrap();
    let allocation_key = state.task_repository_allocation_key.clone().unwrap();
    let original_repository = state.task_repository.clone();
    let original_branch = state.branch.clone();
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    crate::core::implementation::initial_reconciliation::save_plan(
        &dir,
        &state.base,
        &state.base_commit,
        &state.base_commit,
        &state.base_commit,
        &[],
    )
    .unwrap();
    let plan = crate::core::implementation::initial_reconciliation::load_plan(&dir)
        .unwrap()
        .unwrap();
    let canonical_repository = state.task_repository.canonicalize().unwrap();
    fs::write(
        dir.join("base-reconciliation-generated.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "task_repository": canonical_repository,
            "branch": &state.branch,
            "ticket": &state.ticket,
            "local": &plan.local_commit,
            "remote": &plan.remote_commit,
            "files": {}
        }))
        .unwrap(),
    )
    .unwrap();

    let renamed = ".koolade-packet/planning/tasks/implementation/002-implement-ticket-behavior.md";
    fs::create_dir_all(s.repo.join(renamed).parent().unwrap()).unwrap();
    fs::rename(s.repo.join(&s.ticket), s.repo.join(renamed)).unwrap();
    s.git(&s.repo, &["add", "-A"]);
    s.git(&s.repo, &["commit", "-qm", "rename task ticket"]);
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    s.ticket = renamed.into();

    let mut renamed_state = state.clone();
    renamed_state.ticket = renamed.into();
    crate::core::implementation::initial_reconciliation::support::generated::trusted(
        &cleanup_runner(),
        &renamed_state,
        &dir,
    )
    .unwrap();
    fs::remove_file(dir.join("base-reconciliation-generated.json")).unwrap();
    fs::remove_file(dir.join("base-reconciliation.json")).unwrap();

    let resumed = s
        .run("resume", calls.clone())
        .unwrap_or_else(|error| panic!("{error:#}"));

    assert_eq!(resumed.task_repository, original_repository);
    assert_eq!(
        resumed.task_repository_allocation_key.as_deref(),
        Some(allocation_key.as_str())
    );
    assert_eq!(resumed.branch, original_branch);
    assert_eq!(resumed.source_commit, state.source_commit);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
