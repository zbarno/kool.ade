use super::*;

#[path = "legacy_migration/concurrency.rs"]
mod concurrency;
#[path = "legacy_migration/metadata.rs"]
mod metadata;

struct MigrationAwareFixture {
    calls: Arc<AtomicUsize>,
}

impl AiHarness for MigrationAwareFixture {
    fn label(&self) -> String {
        "legacy migration fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }

    fn execute(
        &self,
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, AppError> {
        let index = std::process::Command::new("git")
            .args(["show", ":tracked.txt"])
            .current_dir(&request.repo_root)
            .output()
            .unwrap();
        assert!(index.status.success());
        assert_eq!(String::from_utf8(index.stdout).unwrap(), "staged change\n");
        assert_eq!(
            fs::read_to_string(request.repo_root.join("legacy-commit.txt")).unwrap(),
            "branch-local commit\n"
        );
        assert_eq!(
            fs::read_to_string(request.repo_root.join("tracked.txt")).unwrap(),
            "unstaged change\n"
        );
        assert!(request.repo_root.join("untracked\nname.txt").is_file());
        assert_eq!(
            fs::read_link(request.repo_root.join("untracked-link")).unwrap(),
            Path::new("untracked\nname.txt")
        );
        Fixture {
            mode: "complete",
            calls: self.calls.clone(),
        }
        .execute(request)
    }
}

#[test]
fn saved_legacy_workspace_migrates_all_local_changes_and_keeps_original() {
    let sandbox = Sandbox::new();
    let (_state, worktree, original_status) = setup_legacy_workspace(&sandbox);
    let result = resume_with_harness(
        &sandbox,
        &MigrationAwareFixture {
            calls: Arc::new(AtomicUsize::new(0)),
        },
    )
    .unwrap();

    assert_eq!(result.task_repository_kind, TaskRepositoryKind::Clone);
    assert_ne!(result.task_repository, worktree);
    assert!(
        fs::symlink_metadata(result.task_repository.join(".git"))
            .unwrap()
            .is_dir()
    );
    assert_eq!(
        sandbox.git(&result.task_repository, &["rev-parse", "--git-common-dir"]),
        sandbox.git(&result.task_repository, &["rev-parse", "--git-dir"])
    );
    assert_eq!(
        fs::read_to_string(worktree.join("tracked.txt")).unwrap(),
        "unstaged change\n"
    );
    assert_eq!(
        sandbox.git(&worktree, &["status", "--porcelain"]),
        original_status
    );
    assert!(worktree.is_dir());
    assert!(
        sandbox
            .git(&sandbox.repo, &["worktree", "list", "--porcelain"])
            .contains(worktree.to_str().unwrap())
    );
}

#[test]
fn interrupted_migration_resumes_a_partial_clone_without_overwriting_it() {
    let sandbox = Sandbox::new();
    let (state, worktree, original_status) = setup_legacy_workspace(&sandbox);
    let destination = clone_destination(&sandbox, &state);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    sandbox.git(
        &sandbox.root,
        &[
            "clone",
            "-q",
            "--no-hardlinks",
            "--no-checkout",
            "--single-branch",
            "--branch",
            &state.branch,
            sandbox.repo.to_str().unwrap(),
            destination.to_str().unwrap(),
        ],
    );
    sandbox.git(&destination, &["checkout", "-q", &state.branch]);
    fs::write(
        destination.join("untracked\nname.txt"),
        "partial clone conflict\n",
    )
    .unwrap();

    let migration_dir = state_dir(&sandbox.repo, &sandbox.ticket)
        .unwrap()
        .join("legacy-migration");
    fs::create_dir_all(&migration_dir).unwrap();
    fs::write(
        migration_dir.join("legacy-migration.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "source": worktree,
            "destination": destination,
            "branch": state.branch,
            "head": sandbox.git(&worktree, &["rev-parse", "HEAD"]),
            "snapshot_complete": false,
            "workspace_complete": false
        }))
        .unwrap(),
    )
    .unwrap();

    let error = resume_with_harness(
        &sandbox,
        &MigrationAwareFixture {
            calls: Arc::new(AtomicUsize::new(0)),
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("migrated untracked path changed"), "{error}");
    assert_eq!(
        fs::read_to_string(destination.join("untracked\nname.txt")).unwrap(),
        "partial clone conflict\n"
    );
    assert_eq!(
        sandbox.git(&worktree, &["status", "--porcelain"]),
        original_status
    );
    let record_path = migration_dir.join("legacy-migration.json");
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(&record_path).unwrap()).unwrap();
    assert_eq!(record["snapshot_complete"], true);
    assert_eq!(record["workspace_complete"], false);

    fs::remove_file(destination.join("untracked\nname.txt")).unwrap();
    crate::core::implementation::mark_resume_started(&sandbox.repo, &sandbox.ticket).unwrap();
    let result = resume_with_harness(
        &sandbox,
        &MigrationAwareFixture {
            calls: Arc::new(AtomicUsize::new(0)),
        },
    )
    .unwrap();

    assert_eq!(result.task_repository, destination);
    assert_eq!(result.task_repository_kind, TaskRepositoryKind::Clone);
    assert_eq!(
        fs::read_to_string(destination.join("untracked\nname.txt")).unwrap(),
        "untracked data\n"
    );
    assert_eq!(
        fs::read_to_string(destination.join("tracked.txt")).unwrap(),
        "unstaged change\n"
    );
    let record: serde_json::Value =
        serde_json::from_slice(&fs::read(record_path).unwrap()).unwrap();
    assert_eq!(record["snapshot_complete"], true);
    assert_eq!(record["workspace_complete"], true);
    assert_eq!(
        sandbox.git(&worktree, &["status", "--porcelain"]),
        original_status
    );
}

fn setup_legacy_workspace(sandbox: &Sandbox) -> (Implementation, PathBuf, String) {
    fs::write(sandbox.repo.join("tracked.txt"), "original\n").unwrap();
    sandbox.git(&sandbox.repo, &["add", "tracked.txt"]);
    sandbox.git(&sandbox.repo, &["commit", "-qm", "add tracked source"]);
    sandbox.git(&sandbox.repo, &["push", "-q", "origin", "main"]);
    let base = sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);
    let allocation = crate::core::implementation::key_for_ticket(&sandbox.ticket);
    let branch = format!("koolade/{allocation}");
    let worktree = sandbox
        .root
        .join(".koolade-worktrees")
        .join(crate::persistence::project_slug(
            &sandbox.repo.canonicalize().unwrap(),
        ))
        .join(&allocation);
    fs::create_dir_all(worktree.parent().unwrap()).unwrap();
    sandbox.git(
        &sandbox.repo,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            worktree.to_str().unwrap(),
            "main",
        ],
    );
    fs::write(worktree.join("legacy-commit.txt"), "branch-local commit\n").unwrap();
    sandbox.git(&worktree, &["add", "legacy-commit.txt"]);
    sandbox.git(
        &worktree,
        &["commit", "-qm", "save legacy task branch work"],
    );
    fs::write(worktree.join("tracked.txt"), "staged change\n").unwrap();
    sandbox.git(&worktree, &["add", "tracked.txt"]);
    fs::write(worktree.join("tracked.txt"), "unstaged change\n").unwrap();
    fs::write(worktree.join("untracked\nname.txt"), "untracked data\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("untracked\nname.txt", worktree.join("untracked-link")).unwrap();
    let original_status = sandbox.git(&worktree, &["status", "--porcelain"]);
    let state: Implementation = serde_json::from_value(serde_json::json!({
        "ticket": sandbox.ticket,
        "ticket_text": fs::read_to_string(sandbox.repo.join(&sandbox.ticket)).unwrap(),
        "branch": branch,
        "base": "main",
        "base_commit": base,
        "task_repository": worktree,
        "task_repository_kind": "legacy_worktree",
        "task_repository_ready": true,
        "task_repositories": [worktree],
        "status": "preparing",
        "detail": "Resuming saved work"
    }))
    .unwrap();
    let dir = state_dir(&sandbox.repo, &sandbox.ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    save(&dir, &state).unwrap();
    (state, worktree, original_status)
}

fn clone_destination(sandbox: &Sandbox, state: &Implementation) -> PathBuf {
    let manifest = crate::core::project_repos::ProjectManifest::load(&sandbox.repo).unwrap();
    let repository_id =
        crate::core::implementation::task_repository_id(&state.ticket_text, None, &manifest)
            .unwrap();
    let project_id =
        crate::core::implementation::task_repository::project_id(&sandbox.repo).unwrap();
    crate::core::implementation::task_repository::allocated_path(
        &project_id,
        &repository_id,
        &crate::core::implementation::task_repository::allocation_key(state),
    )
    .unwrap()
}

fn resume_with_harness(
    sandbox: &Sandbox,
    harness: &dyn AiHarness,
) -> anyhow::Result<Implementation> {
    let (progress, _updates) = mpsc::channel();
    run_with_project_options(
        &sandbox.repo,
        &sandbox.repo,
        &sandbox.ticket,
        RunOptions {
            harness,
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
            gh: sandbox.gh.to_str().unwrap(),
            publication_mode: PublicationMode::HoldForReview,
            require_independent_checks: false,
            user_context: None,
            auto_publish_gate: None,
            claim_lease: None,
        },
    )
}
