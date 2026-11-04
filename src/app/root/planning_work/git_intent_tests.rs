use super::*;

#[test]
fn partial_child_selection_inherits_unmodified_repositories() {
    let parent = std::collections::BTreeMap::from([
        (
            "root".into(),
            crate::test_support::approved_git_intent("root", "develop", &"a".repeat(40), "develop"),
        ),
        (
            "api".into(),
            crate::test_support::approved_git_intent("api", "release/2.0", &"b".repeat(40), "main"),
        ),
    ]);
    let child = std::collections::BTreeMap::from([(
        "api".into(),
        crate::core::git_intent::GitIntentSelectionV1 {
            source_ref: "release/2.1".into(),
            destination_ref: "main".into(),
            publication_mode: crate::core::git_intent::PublicationModeV1::CreatePullRequest,
        },
    )]);

    let effective = inherit_selections(Some(&parent), child);
    assert_eq!(effective.len(), 2);
    assert_eq!(effective["root"].source_ref, "develop");
    assert_eq!(effective["root"].destination_ref, "develop");
    assert_eq!(effective["api"].source_ref, "release/2.1");
    assert_eq!(effective["api"].destination_ref, "main");
}

// Production boundary: branch approval resolves local refs before consulting
// the live remote advertisement. A pure validator test cannot prove the
// selected commit comes from the correct Git ref. Dependency: local bare Git;
// setup cost is two commits and one push.
#[test]
fn source_pinning_prefers_local_branch_when_origin_has_same_name() {
    let temp = tempfile::tempdir().unwrap();
    let checkout = temp.path().join("checkout");
    let remote = temp.path().join("remote.git");
    std::fs::create_dir_all(&checkout).unwrap();
    let git = |path: &std::path::Path, args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    git(
        temp.path(),
        &["init", "--bare", "-q", remote.to_str().unwrap()],
    );
    git(&checkout, &["init", "-q", "-b", "main"]);
    git(&checkout, &["config", "user.name", "Koolade fixture"]);
    git(
        &checkout,
        &["config", "user.email", "fixture@koolade.invalid"],
    );
    std::fs::write(checkout.join("README.md"), "base\n").unwrap();
    git(&checkout, &["add", "README.md"]);
    git(&checkout, &["commit", "-q", "-m", "base"]);
    git(
        &checkout,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&checkout, &["push", "-q", "origin", "main"]);
    git(&checkout, &["checkout", "-q", "-b", "source"]);
    std::fs::write(checkout.join("README.md"), "origin source\n").unwrap();
    git(&checkout, &["commit", "-qam", "origin source"]);
    git(&checkout, &["push", "-q", "origin", "source"]);
    let origin_sha = git(&checkout, &["rev-parse", "HEAD"]);
    std::fs::write(checkout.join("README.md"), "local source\n").unwrap();
    git(&checkout, &["commit", "-qam", "local source"]);
    let local_sha = git(&checkout, &["rev-parse", "HEAD"]);

    assert_ne!(local_sha, origin_sha);
    assert_eq!(pin_source_sha(&checkout, "source").unwrap(), local_sha);
}

// Production boundary: source approval must not use a stale remote-tracking
// ref after the origin branch was deleted. Unit validation cannot prove live
// ls-remote is queried. Dependency: local bare Git; setup cost is two commits
// and one push.
#[test]
fn deleted_remote_only_source_is_rejected_even_with_stale_tracking_ref() {
    let temp = tempfile::tempdir().unwrap();
    let checkout = temp.path().join("checkout");
    let remote = temp.path().join("remote.git");
    std::fs::create_dir_all(&checkout).unwrap();
    let git = |path: &std::path::Path, args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    git(
        temp.path(),
        &["init", "--bare", "-q", remote.to_str().unwrap()],
    );
    git(&checkout, &["init", "-q", "-b", "main"]);
    git(&checkout, &["config", "user.name", "Koolade fixture"]);
    git(
        &checkout,
        &["config", "user.email", "fixture@koolade.invalid"],
    );
    std::fs::write(checkout.join("README.md"), "base\n").unwrap();
    git(&checkout, &["add", "README.md"]);
    git(&checkout, &["commit", "-q", "-m", "base"]);
    git(
        &checkout,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&checkout, &["push", "-q", "origin", "main"]);
    git(&checkout, &["checkout", "-q", "-b", "remote-only"]);
    std::fs::write(checkout.join("README.md"), "temporary remote source\n").unwrap();
    git(&checkout, &["commit", "-qam", "remote source"]);
    git(&checkout, &["push", "-q", "origin", "remote-only"]);
    git(&checkout, &["checkout", "-q", "main"]);
    git(&checkout, &["branch", "-D", "remote-only"]);
    git(&checkout, &["fetch", "-q", "origin"]);
    let stale_source_sha = git(&checkout, &["rev-parse", "refs/remotes/origin/remote-only"]);
    git(&checkout, &["push", "-q", "origin", ":remote-only"]);
    git(
        &checkout,
        &[
            "update-ref",
            "refs/remotes/origin/remote-only",
            &stale_source_sha,
        ],
    );

    let tracking_ref = std::process::Command::new("git")
        .args(["show-ref", "--verify", "refs/remotes/origin/remote-only"])
        .current_dir(&checkout)
        .status()
        .unwrap();
    assert!(tracking_ref.success(), "stale tracking ref should remain");
    assert!(pin_source_sha(&checkout, "remote-only").is_err());
    assert!(validate_selection_checkout(&checkout, "remote-only", "main").is_err());
}
