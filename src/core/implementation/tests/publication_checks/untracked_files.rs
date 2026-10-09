use super::*;

#[test]
fn untracked_source_changes_block_publication_without_losing_the_file() {
    assert_untracked_source_is_rejected("untracked-source.rs", false);
}

#[test]
fn ignored_untracked_source_changes_block_publication_without_losing_the_file() {
    assert_untracked_source_is_rejected("ignored-source.rs", true);
}

fn assert_untracked_source_is_rejected(file_name: &str, ignored: bool) {
    let s = Sandbox::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut state = s
        .run_with_publication_policy(
            "complete",
            calls.clone(),
            PublicationMode::HoldForReview,
            None,
            false,
        )
        .unwrap();
    assert_eq!(state.status, ImplementationStatus::AwaitingApproval);
    let source = state.task_repository.join(file_name);
    if ignored {
        let exclude = PathBuf::from(s.git(
            &state.task_repository,
            &["rev-parse", "--git-path", "info/exclude"],
        ));
        let exclude = if exclude.is_absolute() {
            exclude
        } else {
            state.task_repository.join(exclude)
        };
        fs::create_dir_all(exclude.parent().unwrap()).unwrap();
        let mut patterns = fs::read_to_string(&exclude).unwrap_or_default();
        patterns.push_str(&format!("/{file_name}\n"));
        fs::write(exclude, patterns).unwrap();
    }
    fs::write(&source, "must be preserved\n").unwrap();
    if ignored {
        assert!(
            s.git(
                &state.task_repository,
                &["ls-files", "--others", "--exclude-standard", "-z"]
            )
            .is_empty(),
            "ordinary untracked listing should omit the ignored source file"
        );
        assert!(
            s.git(
                &state.task_repository,
                &[
                    "ls-files",
                    "--others",
                    "--ignored",
                    "--exclude-standard",
                    "-z"
                ]
            )
            .contains(file_name)
        );
    }

    let dir = state_dir(&s.repo, &s.ticket).unwrap();
    let (progress, _) = mpsc::channel();
    let runner = Runner {
        gh: s.gh.to_string_lossy().into_owned(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
    };
    let policy = ExecutionPolicy {
        user_context: None,
        publication_mode: PublicationMode::CreatePullRequest,
        require_independent_checks: false,
        auto_publish_gate: None,
        claim_lease: None,
        accrual: None,
    };
    let error = integration::prepare_for_pull_request(
        &s.repo,
        &dir,
        &mut state,
        &Fixture {
            mode: "complete",
            calls,
        },
        &runner,
        &policy,
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("unverified changes before integration"),
        "{error}"
    );
    assert_eq!(fs::read_to_string(source).unwrap(), "must be preserved\n");
    assert!(!s.root.join("pr-created").exists());
}
