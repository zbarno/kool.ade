use super::*;

#[test]
fn missing_legacy_worktree_metadata_blocks_migration_and_preserves_workspace_files() {
    let sandbox = Sandbox::new();
    let (state, worktree, _) = setup_legacy_workspace(&sandbox);
    let destination = clone_destination(&sandbox, &state);
    let metadata = PathBuf::from(sandbox.git(&worktree, &["rev-parse", "--absolute-git-dir"]));
    fs::remove_dir_all(&metadata).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));

    let error = resume_with_harness(
        &sandbox,
        &MigrationAwareFixture {
            calls: calls.clone(),
        },
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("Legacy task repository migration needs attention"),
        "{error}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        fs::read_to_string(worktree.join("tracked.txt")).unwrap(),
        "unstaged change\n"
    );
    assert_eq!(
        fs::read_to_string(worktree.join("untracked\nname.txt")).unwrap(),
        "untracked data\n"
    );
    assert!(worktree.join("untracked-link").symlink_metadata().is_ok());
    assert!(!destination.exists());

    let state_path = state_dir(&sandbox.repo, &sandbox.ticket)
        .unwrap()
        .join("state.json");
    let blocked: Implementation = serde_json::from_slice(&fs::read(state_path).unwrap()).unwrap();
    assert_eq!(blocked.status, ImplementationStatus::Blocked);
    assert_eq!(blocked.task_repository, state.task_repository);
}
