use super::*;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

/// Production boundary: the approved spec publication coordinator performs Git
/// pin checks, pushes an isolated branch, and creates a PR through the GitHub
/// CLI boundary. This proves a remote advance during post-create PR verification
/// is rejected and the stale PR is closed; validator unit tests cannot prove
/// that cross-process race. Uses local Git, a bare remote, and fake `gh`; no
/// Docker or network, with moderate temporary-repository setup.
#[test]
fn remote_advance_during_pr_verification_closes_created_spec_pull_request() {
    let temp = tempfile::tempdir().unwrap();
    let remote = temp.path().join("remote.git");
    let source = temp.path().join("source");
    git(temp.path(), &["init", "--bare", remote.to_str().unwrap()]);
    git(
        temp.path(),
        &["init", "--initial-branch=main", source.to_str().unwrap()],
    );
    git(&source, &["config", "user.name", "Test Operator"]);
    git(&source, &["config", "user.email", "test@example.invalid"]);
    fs::create_dir_all(source.join("docs/koolade/product")).unwrap();
    fs::write(
        source.join("docs/koolade/product/overview.md"),
        "# Current\n",
    )
    .unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial docs"]);
    git(
        &source,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&source, &["push", "-u", "origin", "main"]);
    git(&source, &["fetch", "origin", "main"]);
    let pin = PublishedSpecReader::new(source.clone(), "primary-code", "refs/remotes/origin/main")
        .unwrap()
        .pin()
        .unwrap();

    git(&source, &["checkout", "-b", "fixture-advanced-baseline"]);
    fs::write(source.join("advanced.txt"), "remote advanced\n").unwrap();
    git(&source, &["add", "advanced.txt"]);
    git(&source, &["commit", "-m", "advance published baseline"]);
    let advanced_sha = git(&source, &["rev-parse", "HEAD"]);
    git(
        &source,
        &[
            "push",
            "origin",
            &format!("{advanced_sha}:refs/heads/fixture-advanced-baseline"),
        ],
    );
    git(&source, &["checkout", "main"]);

    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let gh = bin.join("gh");
    let response = temp.path().join("pr.json");
    let created = temp.path().join("created");
    let closed = temp.path().join("closed");
    let close_args = temp.path().join("close-args");
    let log = temp.path().join("create-count");
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = repo ]; then echo zbarno/spec-test; exit 0; fi
if [ "$1" = api ]; then printf '987654321\ttest-operator\n'; exit 0; fi
if [ "$1" = pr ] && [ "$2" = view ]; then
  if [ -f '{}' ]; then
    cat '{}'
    git --git-dir='{}' update-ref refs/heads/main '{}'
    exit 0
  fi
  exit 1
fi
if [ "$1" = pr ] && [ "$2" = close ]; then printf '%s\n' "$@" > '{}'; touch '{}'; exit 0; fi
if [ "$1" = pr ] && [ "$2" = create ]; then
  echo create >> '{}'
  touch '{}'
  branch=""
  while [ "$#" -gt 0 ]; do if [ "$1" = --head ]; then branch="$2"; shift 2; else shift; fi; done
  head_sha=$(git ls-remote origin "refs/heads/$branch" | cut -f1)
  printf '{{"url":"https://github.com/zbarno/spec-test/pull/19","baseRefName":"main","baseRefOid":"{}","headRefName":"%s","headRefOid":"%s","headRepository":{{"nameWithOwner":"zbarno/spec-test"}}}}\n' "$branch" "$head_sha" > '{}'
  echo https://github.com/zbarno/spec-test/pull/19
  exit 0
fi
exit 1
"#,
        response.display(),
        response.display(),
        remote.display(),
        advanced_sha,
        close_args.display(),
        closed.display(),
        log.display(),
        created.display(),
        pin.commit_sha,
        response.display(),
    );
    fs::write(&gh, script).unwrap();
    let mut permissions = fs::metadata(&gh).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&gh, permissions).unwrap();

    let changes = vec![change("docs/koolade/product/overview.md", "# Proposed\n")];
    let consent = PublicationConsent::confirmed(&changes).unwrap();
    let error = publish_with_gh(
        &source,
        "primary-code",
        &pin,
        &changes,
        &consent,
        gh.as_os_str(),
    )
    .unwrap_err();
    assert!(
        format!("{error:#}")
            .contains("published documentation advanced during pull request creation"),
        "unexpected error: {error:#}"
    );
    assert!(created.exists(), "fake provider created a pull request");
    assert!(
        closed.exists(),
        "stale pull request is closed after detection"
    );
    let close_args = fs::read_to_string(close_args).unwrap();
    assert!(close_args.starts_with("pr\nclose\nhttps://github.com/zbarno/spec-test/pull/19\n"));
    assert!(close_args.contains("--repo\nzbarno/spec-test\n"));
    assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 1);
    assert_eq!(
        git(&source, &["ls-remote", "origin", "refs/heads/main"])
            .split_whitespace()
            .next(),
        Some(advanced_sha.as_str()),
        "the simulated provider race advanced the actual remote ref"
    );
    assert_eq!(
        git(&source, &["rev-parse", "refs/heads/main"]),
        pin.commit_sha
    );
    assert!(git(&source, &["status", "--porcelain"]).is_empty());
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
