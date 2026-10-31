use super::*;
use std::process::Command;

// Production boundary: setup retry pushes a preserved planning checkout only
// when its requested remote has no refs. A unit test of the decision branch
// alone cannot prove Git publishes the intended saved branch without
// replacing remote history. This uses local Git and a bare repository only;
// setup is a few init/add/commit/push commands, with no network or Docker.
#[test]
fn interrupted_setup_pushes_to_empty_remote_and_refuses_a_populated_one() {
    let temp = tempfile::tempdir().unwrap();
    let planning = temp.path().join("planning");
    let remote = temp.path().join("planning.git");
    std::fs::create_dir(&planning).unwrap();
    std::fs::create_dir(&remote).unwrap();
    git(
        &remote,
        ["init", "--quiet", "--bare", "--initial-branch=main"],
    );
    git(&planning, ["init", "--quiet", "--initial-branch=main"]);
    git(&planning, ["config", "user.name", "Fixture Operator"]);
    git(
        &planning,
        ["config", "user.email", "fixture@example.invalid"],
    );
    std::fs::write(planning.join("README.md"), "fixture planning data\n").unwrap();
    git(&planning, ["add", "README.md"]);
    git(
        &planning,
        ["commit", "--quiet", "-m", "seed planning fixture"],
    );
    git(
        &planning,
        ["remote", "add", "origin", remote.to_str().unwrap()],
    );

    ensure_remote_empty(&planning, remote.to_str().unwrap()).unwrap();
    push_saved_branch(&planning, remote.to_str().unwrap()).unwrap();

    let populated = ensure_remote_empty(&planning, remote.to_str().unwrap()).unwrap_err();
    assert!(format!("{populated:#}").contains("already has Git refs"));
    let refs = Command::new("git")
        .args(["--git-dir"])
        .arg(&remote)
        .args(["show-ref", "--heads"])
        .output()
        .unwrap();
    assert!(refs.status.success());
    assert!(String::from_utf8_lossy(&refs.stdout).contains("refs/heads/main"));
}

// Production boundary: the remote branch can appear between `ls-remote` and
// `push`. This makes that race deterministic and verifies create-only lease
// semantics. It uses local Git plus a bare remote, with no network, Docker,
// or SSH; setup cost is two local commits and one competing push.
#[test]
fn interrupted_setup_refuses_a_branch_created_after_the_empty_check() {
    let temp = tempfile::tempdir().unwrap();
    let planning = temp.path().join("planning");
    let remote = temp.path().join("planning.git");
    std::fs::create_dir(&planning).unwrap();
    std::fs::create_dir(&remote).unwrap();
    git(
        &remote,
        ["init", "--quiet", "--bare", "--initial-branch=main"],
    );
    git(&planning, ["init", "--quiet", "--initial-branch=main"]);
    git(&planning, ["config", "user.name", "Fixture Operator"]);
    git(
        &planning,
        ["config", "user.email", "fixture@example.invalid"],
    );
    std::fs::write(planning.join("README.md"), "ancestor\n").unwrap();
    git(&planning, ["add", "README.md"]);
    git(&planning, ["commit", "--quiet", "-m", "ancestor commit"]);
    let ancestor = git_stdout(&planning, &["rev-parse", "HEAD"]);
    git(
        &planning,
        ["remote", "add", "origin", remote.to_str().unwrap()],
    );
    std::fs::write(planning.join("README.md"), "descendant\n").unwrap();
    git(&planning, ["add", "README.md"]);
    git(&planning, ["commit", "--quiet", "-m", "descendant commit"]);
    let prepared = prepare_saved_branch_push(&planning, remote.to_str().unwrap()).unwrap();

    // Simulate another actor publishing an ancestor after the empty check.
    // An ordinary push could fast-forward this populated branch.
    git(
        &planning,
        [
            "push",
            "--quiet",
            "origin",
            &format!("{ancestor}:refs/heads/main"),
        ],
    );
    assert!(
        publish_prepared_branch(&planning, &prepared).is_err(),
        "the create-only lease must refuse a branch published after inspection"
    );
    let remote_head = Command::new("git")
        .args(["--git-dir"])
        .arg(&remote)
        .args(["rev-parse", "refs/heads/main"])
        .output()
        .unwrap();
    assert!(remote_head.status.success());
    assert_eq!(
        String::from_utf8_lossy(&remote_head.stdout).trim(),
        ancestor
    );
}

fn git<const N: usize>(root: &std::path::Path, arguments: [&str; N]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?}: {}",
        arguments,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(root: &std::path::Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
