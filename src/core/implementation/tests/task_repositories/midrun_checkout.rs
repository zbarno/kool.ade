use super::*;
use std::collections::BTreeMap;

/// Production boundary: re-enter a persisted task after the operator switches
/// and dirties the normal code checkout, then publish through the normal task
/// workflow. The invariant is that the task retains its approved source SHA,
/// clone, branch, PR head/base and remote target without changing operator
/// index/worktree state. A unit test cannot prove the saved-state, Git push and
/// provider publication path together. Uses local Git/bare remote and fake gh;
/// no Docker or external service, with one temporary repository setup.
#[test]
fn active_task_stays_pinned_after_human_checkout_switch_and_edits() {
    let sandbox = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let started = sandbox.run("complete", calls.clone()).unwrap();
    assert_eq!(started.status, ImplementationStatus::AwaitingApproval);

    let source_ref = started.source_ref.clone().unwrap();
    let source_commit = started.source_commit.clone().unwrap();
    let task_branch = started.branch.clone();
    let task_clone = started.task_repository.clone();
    let reviewed_head = started.verified_head.clone().unwrap();
    let destination = started.base.clone();
    let destination_commit = started.base_commit.clone();
    assert_eq!(
        sandbox.git(&task_clone, &["rev-parse", "HEAD"]),
        reviewed_head
    );

    let human_branch = "human/checkout-after-task-start";
    sandbox.git(&sandbox.repo, &["checkout", "-b", human_branch]);
    sandbox.git(&sandbox.repo, &["push", "-q", "origin", human_branch]);
    fs::write(sandbox.repo.join("src/lib.rs"), "staged operator edit\n").unwrap();
    sandbox.git(&sandbox.repo, &["add", "src/lib.rs"]);
    fs::write(sandbox.repo.join("src/lib.rs"), "unstaged operator edit\n").unwrap();
    fs::write(
        sandbox.repo.join("operator-only.txt"),
        "untracked operator edit\n",
    )
    .unwrap();

    let checkout_head = sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);
    let checkout_branch = sandbox.git(&sandbox.repo, &["symbolic-ref", "--short", "HEAD"]);
    let checkout_status = sandbox.git(
        &sandbox.repo,
        &["status", "--porcelain", "--untracked-files=all"],
    );
    let checkout_index = sandbox.git(&sandbox.repo, &["show", ":src/lib.rs"]);
    let checkout_worktree = fs::read_to_string(sandbox.repo.join("src/lib.rs")).unwrap();
    let checkout_staged_diff = sandbox.git(&sandbox.repo, &["diff", "--cached", "--binary"]);
    let checkout_worktree_diff = sandbox.git(&sandbox.repo, &["diff", "--binary"]);
    let operator_remote_refs = remote_refs(&sandbox);
    assert!(operator_remote_refs.contains_key(&format!("refs/heads/{human_branch}")));

    super::super::publication_checks::authorize_fixture_publication(&sandbox, &started);
    let published = sandbox.run("complete", calls.clone()).unwrap();

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "resume reuses verified work"
    );
    assert_eq!(published.source_ref.as_deref(), Some(source_ref.as_str()));
    assert_eq!(
        published.source_commit.as_deref(),
        Some(source_commit.as_str())
    );
    assert_eq!(published.base, destination);
    assert_eq!(published.base_commit, destination_commit);
    assert_eq!(published.branch, task_branch);
    assert_eq!(published.task_repository, task_clone);
    assert_eq!(
        published.verified_head.as_deref(),
        Some(reviewed_head.as_str())
    );
    assert_eq!(
        published.pr_url.as_deref(),
        Some("https://github.com/fixture/repo/pull/1")
    );
    let pr = published.pr_details.as_ref().unwrap();
    assert_eq!(pr.head_sha, reviewed_head);
    assert_eq!(pr.base_branch, destination);

    let args = fs::read_to_string(sandbox.root.join("pr-args")).unwrap();
    let args = args.lines().collect::<Vec<_>>();
    assert_eq!(arg_value(&args, "--repo"), Some("github.com/fixture/repo"));
    assert_eq!(arg_value(&args, "--head"), Some(task_branch.as_str()));
    assert_eq!(arg_value(&args, "--base"), Some(destination.as_str()));

    let mut expected_remote_refs = operator_remote_refs;
    expected_remote_refs.insert(format!("refs/heads/{task_branch}"), reviewed_head);
    assert_eq!(remote_refs(&sandbox), expected_remote_refs);
    assert_eq!(
        sandbox.git(&task_clone, &["symbolic-ref", "--short", "HEAD"]),
        task_branch
    );
    assert_eq!(
        sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]),
        checkout_head
    );
    assert_eq!(
        sandbox.git(&sandbox.repo, &["symbolic-ref", "--short", "HEAD"]),
        checkout_branch
    );
    assert_eq!(
        sandbox.git(
            &sandbox.repo,
            &["status", "--porcelain", "--untracked-files=all"]
        ),
        checkout_status
    );
    assert_eq!(
        sandbox.git(&sandbox.repo, &["show", ":src/lib.rs"]),
        checkout_index
    );
    assert_eq!(
        fs::read_to_string(sandbox.repo.join("src/lib.rs")).unwrap(),
        checkout_worktree
    );
    assert_eq!(
        sandbox.git(&sandbox.repo, &["diff", "--cached", "--binary"]),
        checkout_staged_diff
    );
    assert_eq!(
        sandbox.git(&sandbox.repo, &["diff", "--binary"]),
        checkout_worktree_diff
    );
    assert_eq!(
        fs::read_to_string(sandbox.repo.join("operator-only.txt")).unwrap(),
        "untracked operator edit\n"
    );
}

fn remote_refs(sandbox: &Sandbox) -> BTreeMap<String, String> {
    sandbox
        .git(
            &sandbox.root,
            &[
                "--git-dir",
                sandbox.root.join("remote.git").to_str().unwrap(),
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
            ],
        )
        .lines()
        .map(|line| {
            let (name, sha) = line.split_once(' ').unwrap();
            (name.to_owned(), sha.to_owned())
        })
        .collect()
}

fn arg_value<'a>(args: &'a [&'a str], key: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| *arg == key)
        .and_then(|index| args.get(index + 1).copied())
}
