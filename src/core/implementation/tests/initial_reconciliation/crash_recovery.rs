use super::*;

#[test]
fn failed_required_baseline_check_prevents_reconciliation_commit_and_task_agent() {
    let s = Sandbox::new();
    fs::write(
        s.repo.join("AGENTS.md"),
        "# Project instructions\n\n- Before marking work complete, run the repository quality gates: `test -f required-baseline-marker`.\n",
    )
    .unwrap();
    s.git(&s.repo, &["add", "AGENTS.md"]);
    s.git(&s.repo, &["commit", "-qm", "require baseline marker"]);
    s.git(&s.repo, &["push", "-q", "origin", "main"]);
    let local_tip_before = s.git(&s.repo, &["rev-parse", "HEAD"]);
    s.advance_remote();
    let peer = s.root.join("peer");
    s.git(&peer, &["rm", "AGENTS.md"]);
    s.git(
        &peer,
        &["commit", "-qm", "remove remote quality instructions"],
    );
    s.git(&peer, &["push", "-q", "origin", "main"]);
    let remote_tip = s.git(&s.root.join("remote.git"), &["rev-parse", "main"]);
    fs::write(s.repo.join("local.txt"), "local change\n").unwrap();
    s.git(&s.repo, &["add", "local.txt"]);
    s.git(&s.repo, &["commit", "-qm", "local change"]);
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: false,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };

    let error = run_with_agent(&s, &agent, None).unwrap_err().to_string();

    assert!(error.contains("required-baseline-marker"), "{error}");
    assert_eq!(agent.calls.load(Ordering::SeqCst), 2);
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), local);
    assert_eq!(
        s.git(&s.root.join("remote.git"), &["rev-parse", "main"]),
        remote_tip
    );
    let state = load(&s.repo, &s.ticket).unwrap();
    assert_eq!(state.status, ImplementationStatus::Blocked);
    assert!(!state.worktree.join("implemented.txt").exists());
    assert!(!state.worktree.join("AGENTS.md").exists());
    assert!(!state.worktree.join("required-baseline-marker").exists());
    assert!(
        !s.git(&state.worktree, &["rev-parse", "--verify", "MERGE_HEAD"])
            .is_empty()
    );
    let evidence =
        fs::read_dir(super::super::super::state_paths::state_dir(&s.repo, &s.ticket).unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("base-reconciliation-")
                            && name.ends_with("-verification.json")
                    })
            })
            .unwrap();
    let evidence: serde_json::Value = serde_json::from_slice(&fs::read(evidence).unwrap()).unwrap();
    assert_eq!(evidence[0]["command"], "test -f required-baseline-marker");
    assert!(evidence[0]["error"].as_str().is_some());
    assert_ne!(local_tip_before, local);
}

#[test]
fn missing_task_state_resumes_the_saved_commit_snapshots() {
    let s = Sandbox::new();
    let original_remote = s.advance_remote();
    s.git(&s.repo, &["fetch", "-q", "origin", "main"]);
    fs::write(s.repo.join("local.txt"), "local change\n").unwrap();
    s.git(&s.repo, &["add", "local.txt"]);
    s.git(&s.repo, &["commit", "-qm", "local change"]);
    let original_local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let common = s.git(&s.repo, &["merge-base", &original_local, &original_remote]);
    let dir = super::super::super::state_paths::state_dir(&s.repo, &s.ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    super::super::super::initial_reconciliation::save_plan(
        &dir,
        "main",
        &original_local,
        &original_remote,
        &common,
        &[],
    )
    .unwrap();

    let peer = s.root.join("peer-later");
    s.git(
        &s.root,
        &[
            "clone",
            "-q",
            "-b",
            "main",
            s.root.join("remote.git").to_str().unwrap(),
            peer.to_str().unwrap(),
        ],
    );
    s.git(&peer, &["config", "user.name", "Fixture"]);
    s.git(&peer, &["config", "user.email", "fixture@example.test"]);
    fs::write(peer.join("remote-later.txt"), "later remote change\n").unwrap();
    s.git(&peer, &["add", "remote-later.txt"]);
    s.git(&peer, &["commit", "-qm", "later remote change"]);
    s.git(&peer, &["push", "-q", "origin", "main"]);
    let later_remote = s.git(&peer, &["rev-parse", "HEAD"]);
    fs::write(s.repo.join("local-later.txt"), "later local change\n").unwrap();
    s.git(&s.repo, &["add", "local-later.txt"]);
    s.git(&s.repo, &["commit", "-qm", "later local change"]);
    let later_local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let agent = ReconcilingAgent {
        calls: Arc::new(AtomicUsize::new(0)),
        conflict: false,
        block: false,
        expect_snapshot: true,
        wrap_report: false,
    };

    let result = run_with_agent(&s, &agent, None).unwrap();

    assert_eq!(agent.calls.load(Ordering::SeqCst), 1);
    assert!(
        s.git(
            &result.worktree,
            &[
                "merge-base",
                "--is-ancestor",
                &original_local,
                &result.base_commit
            ]
        )
        .is_empty()
    );
    assert!(
        s.git(
            &result.worktree,
            &[
                "merge-base",
                "--is-ancestor",
                &original_remote,
                &result.base_commit
            ]
        )
        .is_empty()
    );
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), later_local);
    assert_eq!(
        s.git(&s.root.join("remote.git"), &["rev-parse", "main"]),
        later_remote
    );
}

#[test]
fn unrelated_histories_are_reported_as_a_human_action() {
    let s = Sandbox::new();
    let local = s.git(&s.repo, &["rev-parse", "HEAD"]);
    let unrelated = s.root.join("unrelated");
    fs::create_dir_all(&unrelated).unwrap();
    s.git(&unrelated, &["init", "-q", "-b", "main"]);
    s.git(&unrelated, &["config", "user.name", "Fixture"]);
    s.git(
        &unrelated,
        &["config", "user.email", "fixture@example.test"],
    );
    fs::write(
        unrelated.join("replacement.txt"),
        "new independent history\n",
    )
    .unwrap();
    s.git(&unrelated, &["add", "replacement.txt"]);
    s.git(&unrelated, &["commit", "-qm", "independent history"]);
    s.git(
        &unrelated,
        &[
            "remote",
            "add",
            "origin",
            s.root.join("remote.git").to_str().unwrap(),
        ],
    );
    s.git(&unrelated, &["push", "-q", "--force", "origin", "main"]);
    let remote = s.git(&s.root.join("remote.git"), &["rev-parse", "main"]);
    let calls = Arc::new(AtomicUsize::new(0));
    let agent = ReconcilingAgent {
        calls: calls.clone(),
        conflict: false,
        block: false,
        expect_snapshot: false,
        wrap_report: false,
    };

    let error = run_with_agent(&s, &agent, None).unwrap_err().to_string();

    assert!(error.contains("no common history"), "{error}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(s.git(&s.repo, &["rev-parse", "HEAD"]), local);
    assert_eq!(
        s.git(&s.root.join("remote.git"), &["rev-parse", "main"]),
        remote
    );
}
