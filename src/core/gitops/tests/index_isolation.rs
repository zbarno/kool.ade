use super::*;

fn git_ok(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn seed_repo(tag: &str) -> PathBuf {
    let repo = mkrepo(tag);
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::create_dir_all(repo.join("planning")).unwrap();
    for (path, text) in [
        ("src/foo.rs", "fn value() { 1 }\n"),
        ("one.txt", "one baseline\n"),
        ("two.txt", "two baseline\n"),
        ("partial.txt", "first\nsecond\nthird\nfourth\n"),
        ("unstaged.txt", "unstaged baseline\n"),
        ("planning/open-items.md", "old open items\n"),
    ] {
        fs::write(repo.join(path), text).unwrap();
    }
    git_ok(&repo, &["add", "-A"]);
    git_ok(&repo, &["commit", "-qm", "seed Packet test files"]);
    repo
}

fn index_entry(repo: &Path, path: &str) -> String {
    git_ok(repo, &["ls-files", "--stage", "--", path])
}

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

    // The Packet-owned path also has a user-staged version. Packet's commit
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
        "Packet's authorized version\n",
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
        "Packet's authorized version\n"
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

#[cfg(unix)]
#[test]
fn failed_commit_preserves_head_and_the_users_index() {
    use std::os::unix::fs::PermissionsExt;

    let repo = seed_repo("commit-failure");
    fs::write(repo.join("src/foo.rs"), "fn value() { user staged }\n").unwrap();
    git_ok(&repo, &["add", "src/foo.rs"]);
    let staged_entry = index_entry(&repo, "src/foo.rs");
    let packet_entry = index_entry(&repo, "planning/open-items.md");
    let head_before = git_ok(&repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join("planning/open-items.md"), "Packet update\n").unwrap();

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
    assert_eq!(index_entry(&repo, "planning/open-items.md"), packet_entry);
    assert_eq!(
        git_ok(&repo, &["diff", "--cached", "--name-only"]),
        "src/foo.rs\n"
    );
    assert_eq!(
        fs::read_to_string(repo.join("planning/open-items.md")).unwrap(),
        "Packet update\n",
        "a failed checkpoint keeps Packet's prepared working-tree data"
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
fn cancelled_commit_preserves_the_head_index_and_packet_worktree_data() {
    use std::sync::atomic::AtomicBool;

    let repo = seed_repo("cancelled");
    fs::write(repo.join("src/foo.rs"), "user staged version\n").unwrap();
    git_ok(&repo, &["add", "src/foo.rs"]);
    let staged_entry = index_entry(&repo, "src/foo.rs");
    let packet_entry = index_entry(&repo, "planning/open-items.md");
    let head_before = git_ok(&repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join("planning/open-items.md"), "Packet update\n").unwrap();
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
            .contains("cancelled before Packet committed")
    );
    assert_eq!(git_ok(&repo, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(index_entry(&repo, "src/foo.rs"), staged_entry);
    assert_eq!(index_entry(&repo, "planning/open-items.md"), packet_entry);
    assert_eq!(
        fs::read_to_string(repo.join("planning/open-items.md")).unwrap(),
        "Packet update\n",
        "cancelled checkpoints retain the applied artifacts for retry"
    );
    let _ = fs::remove_dir_all(repo);
}

#[test]
fn parallel_packet_commits_serialize_on_the_latest_head() {
    let repo = seed_repo("parallel");
    let first = repo.clone();
    let first_thread = std::thread::spawn(move || {
        fs::write(first.join("planning/first.md"), "first\n").unwrap();
        commit(&first, "planner: first", &["planning/first.md".into()]).unwrap()
    });
    let second = repo.clone();
    let second_thread = std::thread::spawn(move || {
        fs::write(second.join("planning/second.md"), "second\n").unwrap();
        commit(&second, "planner: second", &["planning/second.md".into()]).unwrap()
    });
    assert!(!first_thread.join().unwrap().is_empty());
    assert!(!second_thread.join().unwrap().is_empty());
    assert_eq!(
        git_ok(&repo, &["show", "HEAD:planning/first.md"]),
        "first\n"
    );
    assert_eq!(
        git_ok(&repo, &["show", "HEAD:planning/second.md"]),
        "second\n"
    );
    let _ = fs::remove_dir_all(repo);
}
