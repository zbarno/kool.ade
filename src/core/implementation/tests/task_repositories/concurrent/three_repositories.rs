use super::*;

/// Production boundary: concurrent workers resolve three approved Git intents
/// through task persistence, repository routing and real clone allocation.
/// A unit test cannot prove the saved IDs/SHAs land in isolated on-disk clones.
/// The fixture uses local Git/bare remotes and a synthetic SSH shim only; no
/// external service is contacted. Setup cost is two repositories and workers.
#[test]
fn concurrent_three_tasks_keep_repository_scoped_refs_and_isolated_clones() {
    let sandbox = Sandbox::new();
    let root_sha = sandbox.git(&sandbox.repo, &["rev-parse", "refs/heads/main"]);
    let root_status = sandbox.git(&sandbox.repo, &["status", "--porcelain"]);
    for branch in ["task/source-b", "integration"] {
        sandbox.git(&sandbox.repo, &["branch", branch, "main"]);
    }
    sandbox.git(
        &sandbox.repo,
        &["push", "-q", "origin", "task/source-b", "integration"],
    );
    let root_refs = refs(&sandbox.root.join("remote.git"));

    let api = sandbox.root.join("api-repo");
    let api_remote = sandbox.root.join("api-remote.git");
    fs::create_dir_all(&api).unwrap();
    sandbox.git(&api, &["init", "-q", "-b", "main"]);
    sandbox.git(&api, &["config", "user.name", "Fixture"]);
    sandbox.git(&api, &["config", "user.email", "fixture@example.test"]);
    fs::create_dir_all(api.join("docs/koolade")).unwrap();
    fs::write(api.join("README.md"), "Synthetic API code repository.\n").unwrap();
    fs::write(
        api.join("docs/koolade/overview.md"),
        "# Published API spec\n",
    )
    .unwrap();
    sandbox.git(&api, &["add", "."]);
    sandbox.git(&api, &["commit", "-qm", "API baseline"]);
    sandbox.git(
        &sandbox.root,
        &["init", "--bare", "-q", api_remote.to_str().unwrap()],
    );
    let api_url = "ssh://git@github.com/fixture/api.git";
    sandbox.git(&api, &["remote", "add", "origin", api_url]);
    for branch in ["release/3.0", "stable"] {
        sandbox.git(&api, &["branch", branch, "main"]);
    }
    let ssh = sandbox.root.join("ssh-fixture");
    fs::write(
        &ssh,
        "#!/bin/sh\nroot=$(dirname \"$0\")\ncase \"$*\" in\n  *git-upload-pack*fixture/api.git*) exec git-upload-pack \"$root/api-remote.git\" ;;\n  *git-receive-pack*fixture/api.git*) exec git-receive-pack \"$root/api-remote.git\" ;;\n  *git-upload-pack*) exec git-upload-pack \"$root/remote.git\" ;;\n  *git-receive-pack*) exec git-receive-pack \"$root/remote.git\" ;;\n  *) exit 2 ;;\nesac\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o700)).unwrap();
    }
    // Sandbox owns/restores process environment under its shared fixture lock.
    unsafe { std::env::set_var("GIT_SSH_COMMAND", &ssh) };
    sandbox.git(
        &api,
        &["push", "-q", "origin", "main", "release/3.0", "stable"],
    );
    let api_sha = sandbox.git(&api, &["rev-parse", "refs/heads/release/3.0"]);

    let mut manifest =
        crate::core::project_repos::ProjectManifest::load(&sandbox.planning_store).unwrap();
    manifest
        .repositories
        .push(crate::core::project_repos::Repository {
            id: "api".into(),
            role: "API code".into(),
            remote: api_url.into(),
            display_name: None,
        });
    sandbox
        .planning_store
        .atomic_write(
            crate::core::project_repos::PROJECT_FILE,
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    let checkouts = crate::core::project_repos::RepositoryCheckoutMapV1 {
        schema_version: 1,
        checkouts: std::collections::BTreeMap::from([("api".into(), api.clone())]),
    };
    sandbox
        .planning_store
        .atomic_write(
            "config/repository-checkouts.json",
            &serde_json::to_vec_pretty(&checkouts).unwrap(),
        )
        .unwrap();

    let second = "planning/tasks/feature/002-root-task.md";
    super::super::super::sandbox::write_lifecycle_task_with_main_intent(
        &sandbox,
        second,
        "# Root task\n\nFeature: F1\n\n## Acceptance criteria\n\n- File contains implemented.\n",
        "TASK-002",
        "Root task",
    );
    let third = "planning/tasks/feature/003-api-task.md";
    super::super::super::sandbox::write_lifecycle_task_with_main_intent(
        &sandbox,
        third,
        "# API task\n\nFeature: F1\n\n## Acceptance criteria\n\n- File contains implemented.\n",
        "TASK-003",
        "API task",
    );
    set_intent(&sandbox, &sandbox.ticket, "root", "main", "main", &root_sha);
    set_intent(
        &sandbox,
        second,
        "root",
        "task/source-b",
        "integration",
        &root_sha,
    );
    set_intent(&sandbox, third, "api", "release/3.0", "stable", &api_sha);

    let api_status = sandbox.git(&api, &["status", "--porcelain"]);
    let api_refs = refs(&api_remote);
    let states = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            let _executor = super::super::super::sandbox::install_verification_executor();
            super::super::run_on_worker(&sandbox, &sandbox.ticket)
        });
        let second_worker = scope.spawn(|| {
            let _executor = super::super::super::sandbox::install_verification_executor();
            super::super::run_on_worker(&sandbox, second)
        });
        let third_worker = scope.spawn(|| {
            let _executor = super::super::super::sandbox::install_verification_executor();
            super::super::run_on_worker(&sandbox, third)
        });
        [
            first.join().unwrap(),
            second_worker.join().unwrap(),
            third_worker.join().unwrap(),
        ]
    });
    let states = states.map(Result::unwrap);
    let expected = [
        ("root", "main", "main", root_sha.as_str()),
        ("root", "task/source-b", "integration", root_sha.as_str()),
        ("api", "release/3.0", "stable", api_sha.as_str()),
    ];
    for (state, (repo, source, destination, sha)) in states.iter().zip(expected) {
        assert_eq!(state.repository_id.as_deref(), Some(repo));
        assert_eq!(state.source_ref.as_deref(), Some(source));
        assert_eq!(state.source_branch.as_deref(), Some(source));
        assert_eq!(state.source_commit.as_deref(), Some(sha));
        assert_eq!(state.destination_branch.as_deref(), Some(destination));
        assert_eq!(state.status, ImplementationStatus::AwaitingApproval);
        assert!(state.branch.starts_with("koolade/"));
        assert!(state.task_repository.join(".git").is_dir());
        assert_eq!(
            sandbox.git(&state.task_repository, &["branch", "--show-current"]),
            state.branch
        );
        assert_eq!(
            sandbox.git(&state.task_repository, &["rev-parse", "HEAD"]),
            state.verified_head.as_deref().unwrap()
        );
        assert!(state.task_repository.join("implemented.txt").is_file());
    }
    for left in 0..states.len() {
        for right in left + 1..states.len() {
            assert_ne!(states[left].branch, states[right].branch);
            assert_ne!(states[left].task_repository, states[right].task_repository);
            assert_ne!(
                common(&states[left].task_repository).unwrap(),
                common(&states[right].task_repository).unwrap()
            );
        }
    }
    for (ticket, state) in [
        (sandbox.ticket.as_str(), &states[0]),
        (second, &states[1]),
        (third, &states[2]),
    ] {
        let persisted =
            crate::core::implementation::load(&sandbox.planning_store.root, ticket).unwrap();
        let raw = String::from_utf8(sandbox.planning_store.read(ticket).unwrap()).unwrap();
        let intent = crate::artifacts::task_docs::parse_metadata(&raw)
            .unwrap()
            .unwrap()
            .git_intent
            .expect("approved local TaskIntent is persisted");
        intent.validate().unwrap();
        assert_eq!(
            intent.repository_id,
            state.repository_id.as_deref().unwrap()
        );
        assert_eq!(intent.source_ref, state.source_ref.as_deref().unwrap());
        assert_eq!(intent.source_sha, state.source_commit.as_deref().unwrap());
        assert_eq!(
            intent.destination_ref,
            state.destination_branch.as_deref().unwrap()
        );
        assert_eq!(persisted.task_uid, state.task_uid);
        assert_eq!(persisted.repository_id, state.repository_id);
        assert_eq!(persisted.source_ref, state.source_ref);
        assert_eq!(persisted.source_commit, state.source_commit);
        assert_eq!(persisted.destination_branch, state.destination_branch);
        assert_eq!(persisted.task_repository, state.task_repository);
    }
    let resumed_api = super::super::run_on_worker(&sandbox, third).unwrap();
    assert_eq!(resumed_api.repository_id.as_deref(), Some("api"));
    assert_eq!(resumed_api.source_ref.as_deref(), Some("release/3.0"));
    assert_eq!(resumed_api.source_commit.as_deref(), Some(api_sha.as_str()));
    assert_eq!(resumed_api.destination_branch.as_deref(), Some("stable"));
    assert_eq!(resumed_api.task_repository, states[2].task_repository);
    assert_eq!(resumed_api.status, ImplementationStatus::AwaitingApproval);
    let marker = states[0].task_repository.join("clone-isolation-marker.txt");
    fs::write(&marker, "private to first clone\n").unwrap();
    assert!(marker.is_file());
    for state in &states[1..] {
        assert!(
            !state
                .task_repository
                .join("clone-isolation-marker.txt")
                .exists()
        );
    }
    assert_eq!(
        sandbox.git(&sandbox.repo, &["status", "--porcelain"]),
        root_status
    );
    assert_eq!(sandbox.git(&api, &["status", "--porcelain"]), api_status);
    assert_eq!(refs(&sandbox.root.join("remote.git")), root_refs);
    assert_eq!(refs(&api_remote), api_refs);
}

fn set_intent(
    sandbox: &Sandbox,
    ticket: &str,
    repository: &str,
    source: &str,
    destination: &str,
    sha: &str,
) {
    let raw = String::from_utf8(sandbox.planning_store.read(ticket).unwrap()).unwrap();
    let mut metadata = crate::artifacts::task_docs::parse_metadata(&raw)
        .unwrap()
        .unwrap();
    let intent = crate::core::git_intent::GitIntentV1::approve(
        repository.into(),
        source.into(),
        sha.into(),
        destination.into(),
        crate::core::git_intent::PublicationModeV1::HoldForReview,
        None,
    )
    .unwrap();
    metadata.repository_id = repository.into();
    metadata.source_branch = Some(source.into());
    metadata.destination_branch = Some(destination.into());
    metadata.source_sha = Some(sha.into());
    metadata.git_intent = Some(intent);
    let updated = crate::artifacts::task_docs::embed(&raw, &metadata).unwrap();
    sandbox
        .planning_store
        .atomic_write(ticket, updated.as_bytes())
        .unwrap();
}

fn refs(remote: &Path) -> String {
    let output = std::process::Command::new("git")
        .args([
            "--git-dir",
            remote.to_str().unwrap(),
            "for-each-ref",
            "--format=%(refname)=%(objectname)",
            "refs/heads",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}
