use super::*;
use std::os::unix::fs::PermissionsExt;

fn approve(s: &Sandbox) {
    approve_path(s, "App/.env");
}

pub(super) fn approve_path(s: &Sandbox, relative: &str) {
    let dir = s.repo.join(".git/koolade");
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.join("runtime-config.json");
    fs::write(&path, serde_json::to_vec(&serde_json::json!({"schema_version":1,"source_root":s.repo.canonicalize().unwrap(),"files":[relative]})).unwrap()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn runtime_config_survives_recovery_and_never_enters_stash_or_task_commit() {
    let s = Sandbox::new();
    fs::write(s.repo.join(".gitignore"), ".cache/\n.env\n").unwrap();
    s.git(&s.repo, &["add", ".gitignore"]);
    s.git(
        &s.repo,
        &["commit", "-qm", "ignore development configuration"],
    );
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    s.advance_remote();
    s.git(&s.repo, &["fetch", "-q", "origin", "main"]);
    fs::write(s.repo.join("local.txt"), "local change\n").unwrap();
    s.git(&s.repo, &["add", "local.txt"]);
    s.git(&s.repo, &["commit", "-qm", "local change"]);
    let remote = s.git(&s.repo, &["rev-parse", "origin/main"]);
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let common = s.git(&s.repo, &["merge-base", &local, &remote]);
    fs::create_dir_all(s.repo.join("App")).unwrap();
    fs::write(
        s.repo.join("App/.env"),
        "SYNTHETIC_CONFIG=normal-checkout\n",
    )
    .unwrap();
    approve(&s);
    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    crate::core::implementation::initial_reconciliation::save_plan(
        &dir,
        "main",
        &local,
        &remote,
        &common,
        &[],
    )
    .unwrap();
    let worktree = super::merge_recovery::task_worktree(&s);
    fs::create_dir_all(worktree.parent().unwrap()).unwrap();
    let branch = format!(
        "koolade/{}",
        crate::core::implementation::key_for_ticket(&s.ticket)
    );
    s.git(
        &s.repo,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            worktree.to_str().unwrap(),
            &remote,
        ],
    );
    s.git(
        &worktree,
        &["merge", "--no-ff", "--no-commit", "--no-edit", &local],
    );
    fs::create_dir_all(worktree.join("App")).unwrap();
    fs::write(worktree.join("App/.env"), "SYNTHETIC_CONFIG=old-task\n").unwrap();
    fs::create_dir_all(worktree.join(".cache")).unwrap();
    fs::write(
        worktree.join(".cache/unexpected.txt"),
        "preserve unrelated work\n",
    )
    .unwrap();
    let (_, agent) = super::merge_recovery::agent();
    let result = run_with_agent(&s, &agent, None).unwrap();
    assert_eq!(result.status, ImplementationStatus::AwaitingReview);
    assert_eq!(
        fs::read_to_string(worktree.join("App/.env")).unwrap(),
        "SYNTHETIC_CONFIG=old-task\n"
    );
    assert!(s.git(&worktree, &["ls-files", "App/.env"]).is_empty());
    let (_, snapshot) = super::merge_recovery::recovery_snapshot(&s);
    assert_eq!(
        snapshot["retained_configuration_paths"],
        serde_json::json!(["App/.env"])
    );
    let stash = snapshot["stash_commit"].as_str().unwrap();
    let saved = s.git(
        &s.repo,
        &["ls-tree", "-r", "--name-only", &format!("{stash}^3")],
    );
    assert!(saved.contains(".cache/unexpected.txt"));
    assert!(!saved.contains(".env"));
    assert!(
        !s.git(&worktree, &["ls-tree", "-r", "--name-only", "HEAD"])
            .lines()
            .any(|path| path.ends_with(".env"))
    );
}

#[test]
fn runtime_config_is_excluded_from_staging_even_without_generated_ledger() {
    let s = Sandbox::new();
    fs::write(s.repo.join(".gitignore"), ".env\n").unwrap();
    s.git(&s.repo, &["add", ".gitignore"]);
    s.git(
        &s.repo,
        &["commit", "-qm", "ignore development configuration"],
    );
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    fs::create_dir_all(s.repo.join("App")).unwrap();
    fs::write(
        s.repo.join("App/.env"),
        "SYNTHETIC_CONFIG=normal-checkout\n",
    )
    .unwrap();
    approve(&s);
    let (_, agent) = super::merge_recovery::agent();
    let result = run_with_agent(&s, &agent, None).unwrap();
    assert!(
        !state_dir(&s.repo, &s.ticket)
            .unwrap()
            .join("base-reconciliation-generated.json")
            .exists()
    );
    assert!(
        s.git(&result.worktree, &["ls-files", "App/.env"])
            .is_empty()
    );
    assert!(
        !s.git(&result.worktree, &["ls-tree", "-r", "--name-only", "HEAD"])
            .contains(".env")
    );
}

#[test]
fn runtime_config_recovery_blocks_revoked_grants_without_rotating_snapshot() {
    let s = super::resilience::interrupted_with_configuration(true);
    let (path, _) = super::merge_recovery::recovery_snapshot(&s);
    let before = fs::read(&path).unwrap();
    fs::remove_file(s.repo.join(".git/koolade/runtime-config.json")).unwrap();
    let worktree = super::merge_recovery::task_worktree(&s);
    fs::write(worktree.join("build/cache.txt"), "operator edit\n").unwrap();
    crate::core::implementation::mark_resume_started(&s.repo, &s.ticket).unwrap();
    let (_, agent) = super::merge_recovery::agent();
    let error = run_with_agent(&s, &agent, Some("Resume verification"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("no longer granted"), "{error}");
    assert_eq!(fs::read(path).unwrap(), before);
    assert_eq!(
        fs::read_to_string(worktree.join(".cache/.env")).unwrap(),
        "SYNTHETIC_CONFIG=task-cache\n"
    );
}

#[test]
fn runtime_config_recovery_reviews_new_grants_already_inside_old_stash() {
    let s = super::resilience::interrupted_with_configuration(false);
    let (path, snapshot) = super::merge_recovery::recovery_snapshot(&s);
    let before = fs::read(&path).unwrap();
    let stash = snapshot["stash_commit"].as_str().unwrap();
    assert!(
        s.git(
            &s.repo,
            &["ls-tree", "-r", "--name-only", &format!("{stash}^3")]
        )
        .contains(".cache/.env")
    );
    fs::create_dir_all(s.repo.join(".cache")).unwrap();
    fs::write(
        s.repo.join(".cache/.env"),
        "SYNTHETIC_CONFIG=source-cache\n",
    )
    .unwrap();
    approve_path(&s, ".cache/.env");
    let worktree = super::merge_recovery::task_worktree(&s);
    fs::write(worktree.join("build/cache.txt"), "operator edit\n").unwrap();
    crate::core::implementation::mark_resume_started(&s.repo, &s.ticket).unwrap();
    let (_, agent) = super::merge_recovery::agent();
    let error = run_with_agent(&s, &agent, Some("Resume verification"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("already contains newly granted"), "{error}");
    assert_eq!(fs::read(path).unwrap(), before);
}
