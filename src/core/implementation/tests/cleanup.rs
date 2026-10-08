use super::*;

#[test]
fn cleanup_reclaims_ignored_builds_keeps_evidence_and_is_idempotent() {
    let s = Sandbox::new();
    let state = completed_cleanup_fixture(&s);
    assert_eq!(state.task_repository_kind, TaskRepositoryKind::Clone);
    fs::write(
        common(&state.task_repository).unwrap().join("info/exclude"),
        "target/\n",
    )
    .unwrap();
    fs::create_dir_all(state.task_repository.join("target/debug")).unwrap();
    fs::write(
        state.task_repository.join("target/debug/build-cache"),
        vec![0u8; 1024 * 1024],
    )
    .unwrap();
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let evidence = fs::read(dir.join("verified-report.json")).unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    let done = load(&s.repo, &s.ticket).unwrap();
    assert!(done.cleanup.completed_at.is_some(), "{:?}", done.cleanup);
    assert!(!state.task_repository.exists());
    assert_eq!(
        fs::read(dir.join("verified-report.json")).unwrap(),
        evidence
    );
    assert_eq!(done.status, ImplementationStatus::Completed);
    assert_eq!(board_column(Some(&done), false), 4);
    let user_git = common(&s.repo).unwrap();
    assert!(!user_git.join("koolade-auto-publish.lock").exists());
    assert!(
        s.git(
            &s.repo,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/koolade-cleanup-bases",
                "refs/koolade-evidence"
            ]
        )
        .is_empty()
    );
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    assert_eq!(load(&s.repo, &s.ticket).unwrap().cleanup, done.cleanup);
}

#[test]
fn cleanup_never_reclaims_an_unpublished_completion_commit() {
    let s = Sandbox::new();
    let mut state = s.run("complete", Arc::new(AtomicUsize::new(0))).unwrap();
    state.status = ImplementationStatus::Completed;
    state.merged_commit = state.verified_head.clone();
    save(&state_dir(&s.repo, &s.ticket).unwrap(), &state).unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    let preserved = load(&s.repo, &s.ticket).unwrap();
    assert!(preserved.cleanup.error.unwrap().contains("not in origin"));
    assert!(state.task_repository.join("implemented.txt").exists());
}

#[test]
fn cleanup_accepts_integration_evidence_after_task_ticket_rename() {
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

    let state = completed_cleanup_fixture(&s);
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let integration_dir = dir.join(format!("integration-{}", state.base_commit));
    fs::create_dir_all(&integration_dir).unwrap();
    let mut integration = state.clone();
    integration.task_repositories.clear();
    save(&integration_dir, &integration).unwrap();

    s.git(&s.repo, &["fetch", "-q", "origin"]);
    s.git(&s.repo, &["merge", "--ff-only", "origin/main"]);
    let renamed = ".koolade-packet/planning/tasks/implementation/002-implement-ticket-behavior.md";
    fs::create_dir_all(s.repo.join(renamed).parent().unwrap()).unwrap();
    fs::rename(s.repo.join(&s.ticket), s.repo.join(renamed)).unwrap();
    s.git(&s.repo, &["add", "-A"]);
    s.git(&s.repo, &["commit", "-qm", "rename task ticket"]);
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    s.ticket = renamed.into();
    let mut renamed_state = state.clone();
    renamed_state.ticket = renamed.into();
    save(&dir, &renamed_state).unwrap();

    refresh_pr(&s.repo, renamed, &cleanup_runner()).unwrap();
    let cleaned = load(&s.repo, renamed).unwrap();
    assert!(
        cleaned.cleanup.completed_at.is_some(),
        "{:?}",
        cleaned.cleanup
    );
    assert!(!state.task_repository.exists());
}

#[test]
fn cleanup_preserves_changes_and_retries_after_they_are_resolved() {
    let s = Sandbox::new();
    let state = completed_cleanup_fixture(&s);
    let draft = state.task_repository.join("unsaved-draft.txt");
    fs::write(&draft, "keep this").unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    let failed = load(&s.repo, &s.ticket).unwrap();
    assert!(
        failed
            .cleanup
            .error
            .as_deref()
            .unwrap()
            .contains("contains changes or untracked files")
    );
    assert_eq!(fs::read_to_string(&draft).unwrap(), "keep this");
    assert_eq!(failed.status, ImplementationStatus::Completed);
    fs::remove_file(&draft).unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    assert!(
        load(&s.repo, &s.ticket)
            .unwrap()
            .cleanup
            .completed_at
            .is_some()
    );
    assert!(!state.task_repository.exists());
}

#[test]
fn cleanup_preserves_a_changed_task_repository_head() {
    let s = Sandbox::new();
    let state = completed_cleanup_fixture(&s);
    s.git(&state.task_repository, &["config", "user.name", "Fixture"]);
    s.git(
        &state.task_repository,
        &["config", "user.email", "fixture@example.test"],
    );
    s.git(
        &state.task_repository,
        &["commit", "--allow-empty", "-qm", "new local work"],
    );
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    let failed = load(&s.repo, &s.ticket).unwrap();
    assert!(failed.cleanup.error.unwrap().contains("unverified HEAD"));
    assert!(state.task_repository.exists());
}

#[test]
fn cleanup_preserves_wrong_identity_missing_publication_and_active_work() {
    let s = Sandbox::new();
    let mut state = completed_cleanup_fixture(&s);
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.join("run.lock"))
        .unwrap();
    lock.lock().unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    assert!(state.task_repository.exists());
    assert!(
        load(&s.repo, &s.ticket)
            .unwrap()
            .cleanup
            .attempted_at
            .is_none()
    );
    drop(lock);
    state.task_repository = s.repo.clone();
    save(&dir, &state).unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    assert!(
        load(&s.repo, &s.ticket)
            .unwrap()
            .cleanup
            .error
            .unwrap()
            .contains("allocation")
    );
    assert!(s.repo.join(&s.ticket).exists());
    state.merged_commit = None;
    save(&dir, &state).unwrap();
    refresh_pr(&s.repo, &s.ticket, &cleanup_runner()).unwrap();
    assert!(
        load(&s.repo, &s.ticket)
            .unwrap()
            .cleanup
            .error
            .unwrap()
            .contains("No confirmed")
    );
}
