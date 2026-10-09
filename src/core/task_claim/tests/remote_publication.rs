use super::*;

#[test]
fn task_claim_uses_the_push_url_for_reads_refreshes_and_release() {
    let fixture = Fixture::new();
    let push_remote = fixture.root.join("push-only.git");
    git_at(
        &fixture.root,
        &["init", "--bare", "-q", push_remote.to_str().unwrap()],
    );
    let repo = &fixture.clones[0];
    git_at(
        repo,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            push_remote.to_str().unwrap(),
        ],
    );
    let base = git_at(repo, &["rev-parse", "HEAD"]);
    let mut lease = ClaimLease::acquire(repo, "task-push-url", &base)
        .unwrap()
        .unwrap();
    let reference = reference("task-push-url").unwrap();

    lease.verify().unwrap();
    lease.refresh().unwrap();
    lease.verify().unwrap();
    assert!(
        git_at(
            &fixture.root,
            &[
                "ls-remote",
                "--refs",
                fixture.remote.to_str().unwrap(),
                &reference,
            ],
        )
        .is_empty()
    );
    assert!(
        !git_at(
            &fixture.root,
            &[
                "ls-remote",
                "--refs",
                push_remote.to_str().unwrap(),
                &reference,
            ],
        )
        .is_empty()
    );

    drop(lease);
    assert!(
        git_at(
            &fixture.root,
            &[
                "ls-remote",
                "--refs",
                push_remote.to_str().unwrap(),
                &reference,
            ],
        )
        .is_empty()
    );
}

#[test]
fn multiple_push_urls_are_rejected_before_creating_a_claim() {
    let fixture = Fixture::new();
    let first = fixture.root.join("first-push.git");
    let second = fixture.root.join("second-push.git");
    for path in [&first, &second] {
        git_at(
            &fixture.root,
            &["init", "--bare", "-q", path.to_str().unwrap()],
        );
    }
    let repo = &fixture.clones[0];
    git_at(
        repo,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            first.to_str().unwrap(),
        ],
    );
    git_at(
        repo,
        &[
            "remote",
            "set-url",
            "--add",
            "--push",
            "origin",
            second.to_str().unwrap(),
        ],
    );
    let base = git_at(repo, &["rev-parse", "HEAD"]);

    assert!(matches!(
        ClaimLease::acquire(repo, "task-multiple-push-urls", &base),
        Err(ClaimError::CoordinationRejected(_))
    ));
    let reference = reference("task-multiple-push-urls").unwrap();
    for path in [&first, &second] {
        assert!(
            git_at(
                &fixture.root,
                &["ls-remote", "--refs", path.to_str().unwrap(), &reference,],
            )
            .is_empty()
        );
    }
}

#[test]
fn atomic_publication_push_rejects_takeover_after_preflight() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    let base = git_at(repo, &["rev-parse", "HEAD"]);
    let lease = ClaimLease::acquire(repo, "task-fenced-push", &base)
        .unwrap()
        .unwrap();
    let expected_object = lease.object.clone();
    let reference = lease.reference.clone();
    let remote = lease.remote.clone();
    fs::write(repo.join("published.txt"), "only current owner\n").unwrap();
    git_at(repo, &["add", "published.txt"]);
    git_at(repo, &["commit", "-qm", "prepare candidate"]);
    let commit = git_at(repo, &["rev-parse", "HEAD"]);
    let mut replacement = lease.record().unwrap();
    replacement.session_id = "replacement-owner-session".into();
    replacement.claimed_at = chrono::Utc::now().timestamp();
    let replacement_object = git::create_object(repo, &replacement).unwrap();
    let destination = "refs/heads/koolade/fenced-candidate";

    let result = git::fenced_push_after_prepare(
        repo,
        &remote,
        &reference,
        &expected_object,
        repo,
        &commit,
        destination,
        || {
            git_at(
                repo,
                &[
                    "push",
                    "-q",
                    "--force",
                    "origin",
                    &format!("{replacement_object}:{reference}"),
                ],
            );
        },
    );
    assert!(result.is_err());
    assert!(
        !Command::new("git")
            .args(["show-ref", "--verify", "--quiet", destination])
            .current_dir(&fixture.remote)
            .status()
            .unwrap()
            .success()
    );
    drop(lease);
    assert_eq!(
        git_at(&fixture.remote, &["rev-parse", &reference]),
        replacement_object
    );
}

#[test]
fn implementation_controller_reports_remote_conflicts_before_running_the_harness() {
    let fixture = Fixture::new();
    let owner = &fixture.clones[0];
    let other = &fixture.clones[1];
    let base = git_at(owner, &["rev-parse", "HEAD"]);
    let _owner_claim = ClaimLease::acquire(owner, "task-worker", &base)
        .unwrap()
        .unwrap();
    let request = ClaimRequest::new(
        other.clone(),
        "task-worker".into(),
        git_at(other, &["rev-parse", "HEAD"]),
        None,
        false,
    );
    let controller =
        crate::core::implementation::Controller::start_project_with_policy_and_claim_request(
            other.clone(),
            other.clone(),
            "task.md".into(),
            crate::core::implementation::StartPolicy {
                publication_mode: crate::core::implementation::PublicationMode::HoldForReview,
                require_independent_checks: false,
            },
            None,
            Box::new(crate::harness::PiHarness),
            request,
        );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if let Some(crate::core::implementation::Event::ClaimBlocked(error)) = controller.poll() {
            assert!(matches!(*error, ClaimError::AlreadyClaimed { .. }));
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "claim conflict was not reported"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
