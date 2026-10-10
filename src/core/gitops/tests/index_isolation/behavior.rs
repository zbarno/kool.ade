use super::*;

#[test]
fn commit_isolates_all_unrelated_index_and_worktree_changes() {
    let repo = seed_repo("isolate");
    fs::write(repo.join("src/foo.rs"), "fn value() { 2 }\n").unwrap();
    fs::write(repo.join("one.txt"), "one staged\n").unwrap();
    fs::write(repo.join("two.txt"), "two staged\n").unwrap();
    git_ok(&repo, &["add", "src/foo.rs", "one.txt", "two.txt"]);

    let partial_staged = "first staged\nsecond staged\nthird staged\nfourth staged\n";
    fs::write(repo.join("partial.txt"), partial_staged).unwrap();
    git_ok(&repo, &["add", "partial.txt"]);
    fs::write(
        repo.join("partial.txt"),
        format!("{partial_staged}fifth unstaged\n"),
    )
    .unwrap();
    fs::write(repo.join("unstaged.txt"), "unstaged user edit\n").unwrap();
    fs::write(repo.join("user-untracked.txt"), "keep untracked\n").unwrap();

    // The Koolade-owned path also has a user-staged version. Koolade's commit
    // records its authorized working-tree version, then restores the staged
    // user blob so that both pieces of work remain visible.
    fs::write(repo.join("planning/open-items.md"), "user staged version\n").unwrap();
    git_ok(&repo, &["add", "planning/open-items.md"]);
    let staged_entries = [
        "src/foo.rs",
        "one.txt",
        "two.txt",
        "partial.txt",
        "planning/open-items.md",
    ]
    .map(|path| (path, index_entry(&repo, path)));
    fs::write(
        repo.join("planning/open-items.md"),
        "Kool.ad/e's authorized version\n",
    )
    .unwrap();

    let sha = commit(
        &repo,
        "planner: save open items",
        &["planning/open-items.md".into()],
    )
    .unwrap();

    assert!(!sha.is_empty());
    assert_eq!(
        git_ok(
            &repo,
            &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]
        ),
        "planning/open-items.md\n",
        "the commit must contain only its authorized path"
    );
    for (path, entry) in staged_entries {
        assert_eq!(
            index_entry(&repo, path),
            entry,
            "staged entry changed: {path}"
        );
    }
    assert_eq!(
        git_ok(&repo, &["diff", "--cached", "--name-only"]),
        "one.txt\npartial.txt\nplanning/open-items.md\nsrc/foo.rs\ntwo.txt\n"
    );
    assert_eq!(
        git_ok(&repo, &["show", ":planning/open-items.md"]),
        "user staged version\n"
    );
    assert_eq!(
        git_ok(&repo, &["show", "HEAD:planning/open-items.md"]),
        "Kool.ad/e's authorized version\n"
    );
    assert_eq!(
        git_ok(&repo, &["show", ":src/foo.rs"]),
        "fn value() { 2 }\n"
    );
    assert_eq!(git_ok(&repo, &["show", ":partial.txt"]), partial_staged);
    assert_eq!(
        fs::read_to_string(repo.join("partial.txt")).unwrap(),
        format!("{partial_staged}fifth unstaged\n")
    );
    assert_eq!(
        fs::read_to_string(repo.join("unstaged.txt")).unwrap(),
        "unstaged user edit\n"
    );
    assert_eq!(
        fs::read_to_string(repo.join("user-untracked.txt")).unwrap(),
        "keep untracked\n"
    );
    let _ = fs::remove_dir_all(repo);
}

#[test]
fn commit_skips_a_removed_ignored_untracked_authorized_path() {
    let repo = seed_repo("removed-ignored-progress");
    git_ok(&repo, &["config", "user.name", "Koolade Test"]);
    git_ok(&repo, &["config", "user.email", "koolade@example.test"]);
    fs::write(
        repo.join(".gitignore"),
        "planning/tasks/**/.koolade-progress.json\n",
    )
    .unwrap();
    git_ok(&repo, &["add", ".gitignore"]);
    git_ok(&repo, &["commit", "-qm", "ignore runtime task progress"]);

    let progress = repo.join("planning/tasks/feature/.koolade-progress.json");
    fs::create_dir_all(progress.parent().unwrap()).unwrap();
    fs::write(&progress, "runtime-only progress\n").unwrap();
    fs::remove_file(progress).unwrap();
    fs::write(
        repo.join("planning/open-items.md"),
        "updated planning state\n",
    )
    .unwrap();

    commit(
        &repo,
        "planner: save planning state",
        &[
            "planning/open-items.md".into(),
            "planning/tasks/feature/.koolade-progress.json".into(),
        ],
    )
    .unwrap();

    assert_eq!(
        git_ok(
            &repo,
            &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]
        ),
        "planning/open-items.md\n"
    );
    assert!(
        git_ok(
            &repo,
            &[
                "ls-files",
                "--",
                "planning/tasks/feature/.koolade-progress.json"
            ]
        )
        .is_empty()
    );
    let _ = fs::remove_dir_all(repo);
}

#[cfg(unix)]
#[test]
fn failed_commit_preserves_head_and_the_users_index() {
    use std::os::unix::fs::PermissionsExt;

    let repo = seed_repo("commit-failure");
    fs::write(repo.join("src/foo.rs"), "fn value() { user staged }\n").unwrap();
    git_ok(&repo, &["add", "src/foo.rs"]);
    let staged_entry = index_entry(&repo, "src/foo.rs");
    let koolade_entry = index_entry(&repo, "planning/open-items.md");
    let head_before = git_ok(&repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join("planning/open-items.md"), "Kool.ad/e update\n").unwrap();

    let hook_dir = repo.join("rejecting-hooks");
    fs::create_dir_all(&hook_dir).unwrap();
    let hook = hook_dir.join("pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    git_ok(&repo, &["config", "core.hooksPath", "rejecting-hooks"]);

    assert!(
        commit(
            &repo,
            "planner: rejected checkpoint",
            &["planning/open-items.md".into()]
        )
        .is_err()
    );
    assert_eq!(git_ok(&repo, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(index_entry(&repo, "src/foo.rs"), staged_entry);
    assert_eq!(index_entry(&repo, "planning/open-items.md"), koolade_entry);
    assert_eq!(
        git_ok(&repo, &["diff", "--cached", "--name-only"]),
        "src/foo.rs\n"
    );
    assert_eq!(
        fs::read_to_string(repo.join("planning/open-items.md")).unwrap(),
        "Kool.ad/e update\n",
        "a failed checkpoint keeps Kool.ad/e's prepared working-tree data"
    );
    let _ = fs::remove_dir_all(repo);
}

#[test]
fn no_op_commit_does_not_commit_staged_user_work() {
    let repo = seed_repo("no-op");
    let head_before = git_ok(&repo, &["rev-parse", "HEAD"]);
    let short_head_before = git_ok(&repo, &["rev-parse", "--short", "HEAD"]);
    fs::write(repo.join("src/foo.rs"), "user staged only\n").unwrap();
    git_ok(&repo, &["add", "src/foo.rs"]);
    let staged_entry = index_entry(&repo, "src/foo.rs");

    let sha = commit(&repo, "planner: no-op", &["planning/open-items.md".into()]).unwrap();

    assert_eq!(sha, short_head_before.trim());
    assert_eq!(git_ok(&repo, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(index_entry(&repo, "src/foo.rs"), staged_entry);
    assert_eq!(
        git_ok(&repo, &["show", ":src/foo.rs"]),
        "user staged only\n"
    );
    let _ = fs::remove_dir_all(repo);
}

#[test]
fn cancelled_commit_preserves_the_head_index_and_koolade_worktree_data() {
    use std::sync::atomic::AtomicBool;

    let repo = seed_repo("cancelled");
    fs::write(repo.join("src/foo.rs"), "user staged version\n").unwrap();
    git_ok(&repo, &["add", "src/foo.rs"]);
    let staged_entry = index_entry(&repo, "src/foo.rs");
    let koolade_entry = index_entry(&repo, "planning/open-items.md");
    let head_before = git_ok(&repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join("planning/open-items.md"), "Kool.ad/e update\n").unwrap();
    let cancel = AtomicBool::new(true);

    let error = commit_cancellable(
        &repo,
        "planner: cancelled checkpoint",
        &["planning/open-items.md".into()],
        &cancel,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("cancelled before Kool.ad/e committed")
    );
    assert_eq!(git_ok(&repo, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(index_entry(&repo, "src/foo.rs"), staged_entry);
    assert_eq!(index_entry(&repo, "planning/open-items.md"), koolade_entry);
    assert_eq!(
        fs::read_to_string(repo.join("planning/open-items.md")).unwrap(),
        "Kool.ad/e update\n",
        "cancelled checkpoints retain the applied artifacts for retry"
    );
    let _ = fs::remove_dir_all(repo);
}
