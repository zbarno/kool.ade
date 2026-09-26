use super::*;
use std::fs;
use std::path::PathBuf;

mod index_isolation;

fn fresh_dir(prefix: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("packet_git_{prefix}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn git_in(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn mkrepo(prefix: &str) -> PathBuf {
    let path = fresh_dir(prefix);
    let _ = git_in(&path, &["init", "-q", "-b", "main"]);
    let _ = git_in(&path, &["config", "user.name", "T"]);
    let _ = git_in(&path, &["config", "user.email", "t@x"]);
    fs::write(path.join("a.txt"), "one").unwrap();
    let _ = git_in(&path, &["add", "a.txt"]);
    let _ = git_in(&path, &["commit", "-qm", "init"]);
    path
}

fn plain_dir(prefix: &str) -> PathBuf {
    fresh_dir(prefix)
}

#[test]
fn snapshot_reports_branch_dirty_and_subject() {
    let repo = mkrepo("snap");
    let initial = snapshot(&repo);
    assert_eq!(initial.branch, "main");
    assert!(!initial.head_short.is_empty());
    assert_eq!(initial.last_subject, "init");
    assert_eq!(initial.dirty, 0);
    fs::write(repo.join("b.txt"), "two").unwrap();
    assert_eq!(snapshot(&repo).dirty, 1);
    let _ = fs::remove_dir_all(repo);
}

#[test]
fn commit_lands_a_checkpoint_with_planner_authorship() {
    let repo = mkrepo("commit");
    fs::write(repo.join("planning.md"), "spec").unwrap();
    let sha = commit(
        &repo,
        "planner: establish initial specification",
        &["planning.md".into()],
    )
    .unwrap();
    let log = git_in(&repo, &["log", "-1", "--pretty=%an %s"]);
    assert!(log.contains(AUTHOR_NAME));
    assert!(log.contains("planner: establish initial specification"));
    assert!(!sha.is_empty());
    let _ = fs::remove_dir_all(repo);
}

#[test]
fn successful_git_add_may_write_a_warning_to_stderr() {
    assert!(require_exit_success("add", 0, String::new(), "warning".into()).is_ok());
    assert!(require_exit_success("add", 1, String::new(), "fatal error".into()).is_err());
}

#[test]
fn git_failures_redact_credentials_from_urls_in_commands_and_stderr() {
    let secret = "https://alice:ghp_private@github.com/acme/private.git";
    let error = require_exit_success(
        &format!("clone {secret}"),
        1,
        String::new(),
        format!("fatal: failed to fetch {secret}"),
    )
    .unwrap_err();
    let crate::error::AppError::Git { cmd, detail } = error else {
        panic!("expected Git error");
    };
    for rendered in [cmd, detail] {
        assert!(rendered.contains("github.com/acme/private.git"));
        assert!(!rendered.contains("alice"));
        assert!(!rendered.contains("ghp_private"));
    }
}

#[test]
fn clone_repo_clones_local_source_into_dest() {
    let source = mkrepo("clonesrc");
    let dest = source.with_file_name(format!(
        "{}_cloned",
        source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("repo")
    ));
    clone_repo(source.to_str().unwrap(), &dest).unwrap();
    assert!(is_work_tree(&dest));
    assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "one");
    let _ = fs::remove_dir_all(dest);
    let _ = fs::remove_dir_all(source);
}

#[test]
fn clone_repo_missing_source_errors_git() {
    let dir = fresh_dir("clonesrcmiss");
    let missing = dir.join("never-init-as-repo");
    let dest = dir.join("cloned");
    match clone_repo(missing.to_str().unwrap(), &dest) {
        Err(AppError::Git { cmd, detail }) => {
            assert!(cmd.starts_with("clone "));
            assert!(!detail.trim().is_empty());
        }
        other => panic!("expected a Git error for a missing clone source, got {other:?}"),
    }
    assert!(!dest.exists());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn clone_repo_missing_dest_parent_errors() {
    let dir = fresh_dir("clonparent");
    let source = mkrepo("clonparent-src");
    let dest = dir.join("does-not-exist").join("deeper").join("clone");
    assert!(clone_repo(source.to_str().unwrap(), &dest).is_err());
    let _ = fs::remove_dir_all(source);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn non_repo_detection() {
    let dir = plain_dir("norepo");
    assert!(!is_work_tree(&dir));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn read_config_returns_local_values_via_standard_resolution() {
    let repo = mkrepo("cfg");
    assert_eq!(read_config(&repo, "user.name"), Some("T".into()));
    assert_eq!(read_config(&repo, "user.email"), Some("t@x".into()));
    assert_eq!(read_config(&repo, "packet.probe.no.such.key"), None);
    let _ = fs::remove_dir_all(repo);
}

#[test]
fn read_config_reads_email_only_under_shielded_ambient() {
    let _shield = test_support::shield("cfg-email");
    let repo = fresh_dir("cfe");
    let _ = git_in(&repo, &["init", "-q", "-b", "main"]);
    let _ = git_in(&repo, &["config", "user.email", "eve@example.org"]);
    assert_eq!(read_config(&repo, "user.name"), None);
    assert_eq!(
        read_config(&repo, "user.email"),
        Some("eve@example.org".into())
    );
    let _ = fs::remove_dir_all(repo);
}

#[test]
fn read_config_swallows_non_repo_and_blank_value_failures() {
    let plain = plain_dir("cfgplain");
    assert_eq!(read_config(&plain, "user.name"), None);
    assert_eq!(read_config(&plain, "user.email"), None);
    let _ = fs::remove_dir_all(plain);

    let _shield = test_support::shield("cfgblank");
    let repo = fresh_dir("cfgb");
    let _ = git_in(&repo, &["init", "-q", "-b", "main"]);
    let _ = git_in(&repo, &["config", "user.name", "   "]);
    assert_eq!(read_config(&repo, "user.name"), None);
    let _ = fs::remove_dir_all(repo);
}

#[test]
fn non_existent_cwd_degrades_to_none() {
    assert_eq!(
        read_config(Path::new("/no/such/cwd-xyz"), "user.name"),
        None
    );
}
