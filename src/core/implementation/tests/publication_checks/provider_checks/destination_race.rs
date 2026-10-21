use super::*;

// Production boundary: publication preflights a refreshed destination, pushes
// the task clone, calls the provider, then reads back the actual PR base. A
// unit comparison cannot prove the destination race between those operations
// is detected or that retry avoids a duplicate. Local Git/SSH and fake provider
// only; one temporary repository and task clone, moderate setup, no Docker or
// external network.
#[test]
fn destination_advance_during_provider_creation_is_detected_and_retry_is_safe() {
    let sandbox = Sandbox::new();
    let mut state = sandbox
        .run_with_publication_policy(
            "complete",
            Arc::new(AtomicUsize::new(0)),
            PublicationMode::HoldForReview,
            None,
            false,
        )
        .unwrap();
    let dir = state_dir(&sandbox.repo, &sandbox.ticket).unwrap();

    sandbox.git(&sandbox.repo, &["checkout", "-b", "fixture-next-base"]);
    fs::write(sandbox.repo.join("destination-update.txt"), "advanced base").unwrap();
    sandbox.git(&sandbox.repo, &["add", "destination-update.txt"]);
    sandbox.git(
        &sandbox.repo,
        &["commit", "-qm", "advance fixture destination"],
    );
    let next_base = sandbox.git(&sandbox.repo, &["rev-parse", "HEAD"]);
    sandbox.git(
        &sandbox.repo,
        &[
            "push",
            "-q",
            "origin",
            &format!("{next_base}:refs/heads/fixture-next-base"),
        ],
    );
    fs::write(sandbox.root.join("advance-main-on-create"), &next_base).unwrap();

    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: sandbox.gh.to_string_lossy().into_owned(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };

    let error = publication::create_pull_request(&dir, &mut state, &runner, None).unwrap_err();
    assert!(error.to_string().contains("destination"), "{error:#}");
    assert!(state.pr_url.is_none());
    assert!(state.pr_details.is_none());
    assert_eq!(
        fs::read_to_string(sandbox.root.join("pr-created"))
            .unwrap()
            .lines()
            .count(),
        1,
        "the provider accepted one create before returning its changed base"
    );

    let retry = publication::create_pull_request(&dir, &mut state, &runner, None).unwrap_err();
    assert!(
        retry.to_string().contains("Destination branch moved"),
        "{retry:#}"
    );
    assert_eq!(
        fs::read_to_string(sandbox.root.join("pr-created"))
            .unwrap()
            .lines()
            .count(),
        1,
        "retry refuses the stale publication before creating a second PR"
    );
}
