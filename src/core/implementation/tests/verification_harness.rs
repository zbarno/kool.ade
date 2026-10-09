use super::*;

struct StateHomeGuard(Option<std::ffi::OsString>);

impl StateHomeGuard {
    fn set(path: &Path) -> Self {
        let previous = std::env::var_os("KOOLADE_HOME");
        // SAFETY: the repository quality gate runs tests serially.
        unsafe { std::env::set_var("KOOLADE_HOME", path) };
        Self(previous)
    }
}

impl Drop for StateHomeGuard {
    fn drop(&mut self) {
        // SAFETY: the repository quality gate runs tests serially.
        unsafe {
            match self.0.take() {
                Some(previous) => std::env::set_var("KOOLADE_HOME", previous),
                None => std::env::remove_var("KOOLADE_HOME"),
            }
        }
    }
}

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
    assert!(state.task_repository.join("missing-file").exists());
}

#[test]
fn verification_task_clone_path_survives_cd_and_spaces() {
    let s = Sandbox::new();
    let state_home = s.root.join("state home with spaces");
    let _state_home = StateHomeGuard::set(&state_home);
    let project_id = crate::persistence::project_slug(&s.repo.canonicalize().unwrap());
    let repository_id = "repo";
    let task_key = crate::core::implementation::key_for_ticket(&s.ticket);
    let cwd = state_home
        .join("projects")
        .join(project_id)
        .join("task-repositories")
        .join(repository_id)
        .join(task_key);
    fs::create_dir_all(cwd.parent().unwrap()).unwrap();
    s.git(
        &s.repo,
        &[
            "clone",
            "--quiet",
            s.repo.to_str().unwrap(),
            cwd.to_str().unwrap(),
        ],
    );
    s.git(
        &cwd,
        &["switch", "--quiet", "-c", "koolade/verify-path-test"],
    );
    fs::write(cwd.join("marker"), "proof").unwrap();
    let (progress, _rx) = mpsc::channel();
    let runner = Runner {
        gh: "unused".into(),
        runtime_config_source: None,
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
        runtime_config_source: None,
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
                &result.task_repository,
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
