use super::*;

#[test]
fn publication_targets_the_origin_repository() {
    assert_eq!(
        remote_repository("git@github.com:team/project.git"),
        "github.com/team/project"
    );
    assert_eq!(
        remote_repository("https://github.com/team/project.git"),
        "github.com/team/project"
    );
    assert_eq!(
        remote_repository("ssh://git@github.company.test/team/project.git"),
        "github.company.test/team/project"
    );
}

#[test]
fn verification_receives_corrections_even_after_report_retries_are_used() {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let state = s.run("repair_mixed", calls.clone()).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 5);
    assert_eq!(state.status, ImplementationStatus::AwaitingReview);
    assert!(state.worktree.join("missing-file").exists());
}

#[test]
fn verification_worktree_path_survives_cd_and_spaces() {
    let s = Sandbox::new();
    let cwd = s
        .root
        .join(".koolade-worktrees")
        .join(crate::persistence::project_slug(&s.repo))
        .join("worktree with spaces");
    fs::create_dir_all(cwd.parent().unwrap()).unwrap();
    s.git(
        &s.repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "koolade/verify-path-test",
            cwd.to_str().unwrap(),
        ],
    );
    fs::write(cwd.join("marker"), "proof").unwrap();
    let (progress, _rx) = mpsc::channel();
    let runner = Runner {
        gh: "unused".into(),
        deadline: Instant::now() + Duration::from_secs(5),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
    };
    runner
        .verify(
            &cwd,
            "test -d /tmp/koolade-home/.npm-prepared/_cacache/content-v2 && \
             if touch /tmp/koolade-home/.npm-prepared/_cacache/content-v2/.koolade-write-guard 2>/dev/null; then exit 31; fi",
        )
        .unwrap();
    runner
        .verify(
            &cwd,
            "cd / && test \"$(cat \"$KOOLADE_WORKTREE/marker\")\" = proof",
        )
        .unwrap();
    runner.verify(&cwd, "test -f marker && test -z \"${PREVIOUS_CHECK_VARIABLE+x}\" && PREVIOUS_CHECK_VARIABLE=value").unwrap();
    runner
        .verify(&cwd, "test -z \"${PREVIOUS_CHECK_VARIABLE+x}\"")
        .unwrap();
}

#[test]
fn failed_command_retains_both_streams_for_correction() {
    let (progress, _rx) = mpsc::channel();
    let runner = Runner {
        gh: "unused".into(),
        deadline: Instant::now() + Duration::from_secs(5),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
    };
    let error = runner
        .command(
            &std::env::temp_dir(),
            "/bin/sh",
            &["-c", "echo assertion-detail; echo diagnostic >&2; exit 1"],
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("assertion-detail"));
    assert!(error.contains("diagnostic"));
}

#[test]
fn harness_failures_retry_and_saved_diagnostics_survive_lost_final_message() {
    for (mode, expected) in [("harness_retry", 2), ("sidecar", 2), ("healing", 5)] {
        let s = Sandbox::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let result = s.run(mode, calls.clone()).unwrap();
        assert_eq!(result.status, ImplementationStatus::AwaitingReview);
        assert_eq!(calls.load(Ordering::SeqCst), expected);
        assert_eq!(
            s.git(
                &result.worktree,
                &[
                    "rev-list",
                    "--count",
                    &format!("{}..HEAD", result.base_commit)
                ]
            ),
            "1"
        );
    }
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(
        s.run("harness_dead", calls.clone())
            .unwrap_err()
            .to_string()
            .contains("Harness recovery exhausted")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    assert!(!s.root.join("pr-created").exists());
}
