use super::support::change;
use super::*;
#[cfg(unix)]
#[path = "publish/race.rs"]
mod race;
#[cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

/// Production boundary: prove the publication coordinator fetches the pinned
/// base into an isolated object store, writes only approved docs, pushes one
/// unique branch, and calls GitHub with explicit PR base/head. Unit tests of
/// path validators cannot prove this multi-process Git/gh sequencing.
/// Dependencies: local Git and a fake `gh` executable; no Docker or network.
/// Setup cost: moderate (temporary work repository, bare remote, and fake CLI).
#[cfg(unix)]
#[test]
fn coordinator_publishes_only_the_approved_docs_branch_and_explicit_pull_request() {
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
    fs::create_dir_all(source.join("docs/koolade/features")).unwrap();
    fs::write(
        source.join("docs/koolade/product/overview.md"),
        "# Current\n",
    )
    .unwrap();
    fs::write(source.join("README.md"), "# Code\n").unwrap();
    git(&source, &["add", "."]);
    git(&source, &["commit", "-m", "initial docs"]);
    git(
        &source,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&source, &["push", "-u", "origin", "main"]);
    git(&source, &["fetch", "origin", "main"]);

    let gh_directory = temp.path().join("bin");
    fs::create_dir(&gh_directory).unwrap();
    let gh_log = temp.path().join("gh-args.txt");
    let gh_created = temp.path().join("gh-created");
    let gh_response = temp.path().join("gh-response.json");
    let gh_retargeted = temp.path().join("gh-retargeted");
    let gh_head_moved = temp.path().join("gh-head-moved");
    let gh = gh_directory.join("gh");
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = repo ]; then echo zbarno/spec-test; exit 0; fi
if [ "$1" = api ]; then printf '987654321\ttest-operator\n'; exit 0; fi
if [ "$1" = pr ] && [ "$2" = view ]; then
  if [ -f '{}' ]; then
    if [ -f '{}' ]; then sed 's/"baseRefName":"main"/"baseRefName":"release"/' '{}';
    elif [ -f '{}' ]; then sed -E 's/"headRefOid":"[0-9a-f]+"/"headRefOid":"0000000000000000000000000000000000000000"/' '{}';
    else cat '{}'; fi
    exit 0
  fi
  exit 1
fi
printf '%s\n' "$@" > '{}'
branch=""
while [ "$#" -gt 0 ]; do if [ "$1" = --head ]; then branch="$2"; shift 2; else shift; fi; done
touch '{}'
base=main
head_sha=$(git ls-remote origin "refs/heads/$branch" | cut -f1)
base_sha=$(git rev-parse refs/remotes/origin/main)
if [ -f '{}' ]; then base=release; fi
if [ -f '{}' ]; then head_sha=0000000000000000000000000000000000000000; fi
printf '{{"url":"https://github.com/zbarno/spec-test/pull/19","baseRefName":"%s","baseRefOid":"%s","headRefName":"%s","headRefOid":"%s","headRepository":{{"nameWithOwner":"zbarno/spec-test"}}}}\n' "$base" "$base_sha" "$branch" "$head_sha" > '{}'
echo https://github.com/zbarno/spec-test/pull/19
"#,
        gh_created.display(),
        gh_retargeted.display(),
        gh_response.display(),
        gh_head_moved.display(),
        gh_response.display(),
        gh_response.display(),
        gh_log.display(),
        gh_created.display(),
        gh_retargeted.display(),
        gh_head_moved.display(),
        gh_response.display()
    );
    fs::write(&gh, script).unwrap();
    let mut permissions = fs::metadata(&gh).unwrap().permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&gh, permissions).unwrap();

    let reader =
        PublishedSpecReader::new(source.clone(), "primary-code", "refs/remotes/origin/main")
            .unwrap();
    let pin = reader.pin().unwrap();
    let changes = vec![
        change("docs/koolade/product/overview.md", "# Updated\n"),
        change("docs/koolade/features/search.md", "# Search\n"),
    ];
    let consent = PublicationConsent::confirmed(&changes).unwrap();
    let request = publish_with_gh(
        &source,
        "primary-code",
        &pin,
        &changes,
        &consent,
        gh.as_os_str(),
    )
    .unwrap();
    let retried = publish_with_gh(
        &source,
        "primary-code",
        &pin,
        &changes,
        &consent,
        gh.as_os_str(),
    )
    .unwrap();

    assert_eq!(request.base_commit, pin.commit_sha);
    assert_eq!(
        retried, request,
        "publication retry should reuse the same PR"
    );
    let created_args = fs::read_to_string(&gh_log).unwrap();
    fs::write(&gh_retargeted, "retargeted").unwrap();
    let retargeted = publish_with_gh(
        &source,
        "primary-code",
        &pin,
        &changes,
        &consent,
        gh.as_os_str(),
    )
    .unwrap_err();
    assert_eq!(
        retargeted.to_string(),
        "existing publication pull request no longer matches the approved base and head",
        "retargeted PR must fail closed without preparing a replacement"
    );
    assert_eq!(fs::read_to_string(&gh_log).unwrap(), created_args);
    fs::remove_file(&gh_retargeted).unwrap();
    fs::write(&gh_head_moved, "moved").unwrap();
    let moved = publish_with_gh(
        &source,
        "primary-code",
        &pin,
        &changes,
        &consent,
        gh.as_os_str(),
    )
    .unwrap_err();
    assert_eq!(
        moved.to_string(),
        "existing publication pull request no longer matches the approved base and head",
        "moved PR head must fail closed without preparing a replacement"
    );
    assert_eq!(fs::read_to_string(&gh_log).unwrap(), created_args);
    assert!(request.branch.starts_with("docs/koolade-spec-"));
    assert_eq!(request.url, "https://github.com/zbarno/spec-test/pull/19");
    let default_head = git(&source, &["rev-parse", "refs/heads/main"]);
    assert_eq!(default_head, pin.commit_sha);
    let remote_head = git(
        &source,
        &[
            "ls-remote",
            "origin",
            &format!("refs/heads/{}", request.branch),
        ],
    );
    let pushed = remote_head.split_whitespace().next().unwrap();
    assert_eq!(pushed, request.head_commit);
    git(&source, &["fetch", "origin", &request.branch]);
    assert_eq!(
        git(&source, &["show", "-s", "--format=%P", pushed]),
        pin.commit_sha
    );
    assert_eq!(
        git(&source, &["show", "-s", "--format=%ae", pushed]),
        "987654321+test-operator@users.noreply.github.com"
    );
    let paths = git(
        &source,
        &["diff-tree", "--no-commit-id", "--name-only", "-r", pushed],
    );
    assert_eq!(
        paths.lines().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "docs/koolade/features/search.md",
            "docs/koolade/product/overview.md"
        ])
    );
    let gh_args = fs::read_to_string(gh_log).unwrap();
    assert!(gh_args.contains(&format!("--base\nmain\n--head\n{}", request.branch)));
    assert!(gh_args.contains("--body-file"));
    assert!(gh_args.contains("--repo\nzbarno/spec-test"));
    assert!(!gh_args.contains("--force"));
    assert!(git(&source, &["status", "--porcelain"]).is_empty());
}

#[cfg(unix)]
#[test]
fn existing_symlink_parent_is_rejected_without_following_it() {
    let temp = tempfile::tempdir().unwrap();
    let repository = temp.path().join("repo");
    git(
        temp.path(),
        &[
            "init",
            "--initial-branch=main",
            repository.to_str().unwrap(),
        ],
    );
    git(&repository, &["config", "user.name", "Test Operator"]);
    git(
        &repository,
        &["config", "user.email", "test@example.invalid"],
    );
    fs::create_dir_all(repository.join("docs/koolade")).unwrap();
    std::os::unix::fs::symlink("../../outside", repository.join("docs/koolade/features")).unwrap();
    git(&repository, &["add", "."]);
    git(&repository, &["commit", "-m", "symlink fixture"]);
    let commit = git(&repository, &["rev-parse", "HEAD"]);

    let error = ensure_existing_path_is_safe(&repository, &commit, "docs/koolade/features/new.md")
        .unwrap_err();
    assert!(error.to_string().contains("symbolic-link parent"));
    assert!(!repository.join("outside/new.md").exists());
}

#[cfg(unix)]
fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
