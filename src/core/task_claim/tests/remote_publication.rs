use super::*;

#[path = "remote_publication/process_fence.rs"]
mod process_fence;
#[path = "remote_publication/same_remote.rs"]
mod same_remote;

#[test]
fn publication_checks_planning_claim_again_after_preflight_and_pushes_code_remote() {
    let fixture = Fixture::new();
    let planning_repo = &fixture.clones[0];
    let code_repo = &fixture.code_clones[0];
    let base = git_at(code_repo, &["rev-parse", "HEAD"]);
    let lease = ClaimLease::acquire(
        planning_repo,
        fixture.project_uid,
        "task-fenced-push",
        "web",
        &base,
    )
    .unwrap()
    .unwrap();
    let expected_object = lease.object.clone();
    let reference = lease.reference.clone();
    let planning_remote = lease.remote.clone();
    fs::write(code_repo.join("published.txt"), "only current owner\n").unwrap();
    git_at(code_repo, &["add", "published.txt"]);
    git_at(code_repo, &["commit", "-qm", "prepare candidate"]);
    let commit = git_at(code_repo, &["rev-parse", "HEAD"]);
    let mut replacement = lease.record().unwrap();
    replacement.session_id = uuid::Uuid::new_v4().to_string();
    replacement.operator_id = uuid::Uuid::new_v4().to_string();
    replacement.issued_at_utc = chrono::Utc::now().timestamp();
    replacement.expires_at_utc = replacement.issued_at_utc + 15 * 60;
    let replacement_object = git::create_object(planning_repo, &replacement).unwrap();
    let destination = "refs/heads/koolade/fenced-candidate";

    let result = git::fenced_push_after_prepare(
        planning_repo,
        &planning_remote,
        &reference,
        &expected_object,
        code_repo,
        &commit,
        destination,
        || {
            git_at(
                planning_repo,
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
        git_at(&fixture.planning_remote, &["rev-parse", &reference]),
        replacement_object
    );
}

#[test]
fn unavailable_planning_remote_blocks_code_publication_and_preserves_local_evidence() {
    let fixture = Fixture::new();
    let planning_repo = &fixture.clones[0];
    let code_repo = &fixture.code_clones[0];
    let task_uid = "task-planning-remote-unavailable-publication";
    let base = git_at(code_repo, &["rev-parse", "HEAD"]);
    let mut lease = ClaimLease::acquire(planning_repo, fixture.project_uid, task_uid, "web", &base)
        .unwrap()
        .unwrap();
    let reference = reference(fixture.project_uid, task_uid).unwrap();
    let claim_object = git_at(&fixture.planning_remote, &["rev-parse", &reference]);

    fs::write(
        code_repo.join("unverified.txt"),
        "must stay local while offline\n",
    )
    .unwrap();
    git_at(code_repo, &["add", "unverified.txt"]);
    git_at(code_repo, &["commit", "-qm", "prepare offline candidate"]);
    let candidate = git_at(code_repo, &["rev-parse", "HEAD"]);
    let destination = "refs/heads/koolade/offline-candidate";
    let moved_remote = fixture.root.join("planning-offline.git");
    fs::rename(&fixture.planning_remote, &moved_remote).unwrap();
    let result = lease.fenced_push(code_repo, &candidate, destination);
    fs::rename(&moved_remote, &fixture.planning_remote).unwrap();

    assert!(result.is_err());
    assert_eq!(
        git_at(&fixture.planning_remote, &["rev-parse", &reference]),
        claim_object
    );
    assert!(
        !Command::new("git")
            .args(["show-ref", "--verify", "--quiet", destination])
            .current_dir(&fixture.remote)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        fs::read_to_string(code_repo.join("unverified.txt")).unwrap(),
        "must stay local while offline\n"
    );
    drop(lease);
}

#[test]
fn implementation_controller_reports_global_conflicts_before_running_the_harness() {
    let fixture = Fixture::new();
    let owner = &fixture.clones[0];
    let other = &fixture.clones[1];
    let base = git_at(&fixture.code_clones[0], &["rev-parse", "HEAD"]);
    let _owner_claim = ClaimLease::acquire(owner, fixture.project_uid, "task-worker", "web", &base)
        .unwrap()
        .unwrap();
    let request = ClaimRequest::new(
        other.clone(),
        fixture.code_clones[1].clone(),
        fixture.project_uid,
        "task-worker".into(),
        "web".into(),
        git_at(&fixture.code_clones[1], &["rev-parse", "HEAD"]),
        None,
        ClaimScope::SharedRequired,
        false,
    );
    let controller =
        crate::core::implementation::Controller::start_project_with_policy_and_claim_request(
            other.clone(),
            fixture.code_clones[1].clone(),
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
        if let Some(crate::core::task_claim::ClaimError::AlreadyClaimed { .. }) =
            controller.poll().and_then(|event| match event {
                crate::core::implementation::Event::ClaimBlocked(error) => Some(*error),
                _ => None,
            })
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "claim conflict was not reported"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
