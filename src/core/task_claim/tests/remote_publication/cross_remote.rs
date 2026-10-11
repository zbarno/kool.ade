use super::*;

#[test]
fn claim_loss_retracts_only_the_stale_candidate_branch() {
    let fixture = Fixture::new();
    let planning_repo = &fixture.clones[0];
    let code_repo = &fixture.code_clones[0];
    let base = git_at(code_repo, &["rev-parse", "HEAD"]);
    let lease = ClaimLease::acquire(
        planning_repo,
        fixture.project_uid,
        "task-cross-remote-post-push-takeover",
        "web",
        &base,
    )
    .unwrap()
    .unwrap();
    let expected_object = lease.object.clone();
    let reference = lease.reference.clone();
    let planning_remote = lease.remote.clone();
    fs::write(
        code_repo.join("stale-branch.txt"),
        "keep as local evidence\n",
    )
    .unwrap();
    git_at(code_repo, &["add", "stale-branch.txt"]);
    git_at(
        code_repo,
        &["commit", "-qm", "prepare stale cross-remote candidate"],
    );
    let commit = git_at(code_repo, &["rev-parse", "HEAD"]);
    let replacement_object = replacement_claim(planning_repo, &lease);
    let destination = "refs/heads/koolade/cross-remote-raced-candidate";

    let result = git::fenced_push_after_code_push(
        planning_repo,
        &planning_remote,
        &reference,
        &expected_object,
        code_repo,
        &commit,
        destination,
        || push_claim_replacement(planning_repo, &replacement_object, &reference),
    );

    assert!(result.is_err());
    assert_eq!(
        git_at(&fixture.planning_remote, &["rev-parse", &reference]),
        replacement_object
    );
    assert!(
        !Command::new("git")
            .args(["show-ref", "--verify", "--quiet", destination])
            .current_dir(&fixture.remote)
            .status()
            .unwrap()
            .success(),
        "the stale worker must retract its candidate after losing the lease"
    );
    assert_eq!(
        fs::read_to_string(code_repo.join("stale-branch.txt")).unwrap(),
        "keep as local evidence\n"
    );
}

#[test]
fn cleanup_preserves_a_candidate_replaced_by_a_newer_writer() {
    let fixture = Fixture::new();
    let planning_repo = &fixture.clones[0];
    let code_repo = &fixture.code_clones[0];
    let base = git_at(code_repo, &["rev-parse", "HEAD"]);
    let lease = ClaimLease::acquire(
        planning_repo,
        fixture.project_uid,
        "task-cross-remote-cleanup-cas",
        "web",
        &base,
    )
    .unwrap()
    .unwrap();
    let expected_object = lease.object.clone();
    let reference = lease.reference.clone();
    let planning_remote = lease.remote.clone();
    fs::write(code_repo.join("candidate.txt"), "stale candidate\n").unwrap();
    git_at(code_repo, &["add", "candidate.txt"]);
    git_at(code_repo, &["commit", "-qm", "prepare stale candidate"]);
    let stale_commit = git_at(code_repo, &["rev-parse", "HEAD"]);
    fs::write(code_repo.join("candidate.txt"), "newer writer candidate\n").unwrap();
    git_at(code_repo, &["commit", "-qam", "prepare newer candidate"]);
    let newer_commit = git_at(code_repo, &["rev-parse", "HEAD"]);
    git_at(code_repo, &["reset", "--hard", &stale_commit]);
    let replacement_object = replacement_claim(planning_repo, &lease);
    let destination = "refs/heads/koolade/cross-remote-replaced-candidate";

    let result = git::fenced_push_after_code_push(
        planning_repo,
        &planning_remote,
        &reference,
        &expected_object,
        code_repo,
        &stale_commit,
        destination,
        || {
            push_claim_replacement(planning_repo, &replacement_object, &reference);
            git_at(
                code_repo,
                &[
                    "push",
                    "-q",
                    "--force",
                    "origin",
                    &format!("{newer_commit}:{destination}"),
                ],
            );
        },
    );

    assert!(result.is_err());
    assert_eq!(
        git_at(&fixture.remote, &["rev-parse", destination]),
        newer_commit,
        "cleanup must not remove a branch another writer replaced"
    );
    assert_eq!(
        git_at(&fixture.planning_remote, &["rev-parse", &reference]),
        replacement_object
    );
}

#[test]
fn stale_cleanup_cannot_remove_same_commit_after_new_owner_fences_it() {
    let fixture = Fixture::new();
    let planning_repo = &fixture.clones[0];
    let code_repo = &fixture.code_clones[0];
    let base = git_at(code_repo, &["rev-parse", "HEAD"]);
    let lease = ClaimLease::acquire(
        planning_repo,
        fixture.project_uid,
        "task-cross-remote-same-commit-takeover",
        "web",
        &base,
    )
    .unwrap()
    .unwrap();
    let expected_object = lease.object.clone();
    let reference = lease.reference.clone();
    let planning_remote = lease.remote.clone();
    fs::write(code_repo.join("same-commit.txt"), "shared candidate\n").unwrap();
    git_at(code_repo, &["add", "same-commit.txt"]);
    git_at(code_repo, &["commit", "-qm", "prepare reusable candidate"]);
    let commit = git_at(code_repo, &["rev-parse", "HEAD"]);
    let replacement_object = replacement_claim(planning_repo, &lease);
    let destination = "refs/heads/koolade/cross-remote-same-commit";

    let result = git::fenced_push_after_code_push(
        planning_repo,
        &planning_remote,
        &reference,
        &expected_object,
        code_repo,
        &commit,
        destination,
        || {
            push_claim_replacement(planning_repo, &replacement_object, &reference);
            git::fenced_push(
                planning_repo,
                &planning_remote,
                &reference,
                &replacement_object,
                code_repo,
                &commit,
                destination,
            )
            .unwrap();
        },
    );

    assert!(result.is_err());
    assert_eq!(git_at(&fixture.remote, &["rev-parse", destination]), commit);
    let fence_ref = reference.replace(
        "refs/heads/koolade/claims/",
        "refs/heads/koolade/claim-fences/",
    );
    let fence_object = git_at(&fixture.remote, &["rev-parse", &fence_ref]);
    let fence_session = git_at(code_repo, &["show", "-s", "--format=%s", &fence_object]);
    let replacement_record: ClaimRecord = serde_json::from_str(&git_at(
        planning_repo,
        &["show", "-s", "--format=%B", &replacement_object],
    ))
    .unwrap();
    assert_eq!(fence_session, replacement_record.session_id,);
    assert_eq!(
        git_at(&fixture.planning_remote, &["rev-parse", &reference]),
        replacement_object
    );
}

#[test]
fn lost_push_ack_is_reconciled_from_the_code_remote() {
    let fixture = Fixture::new();
    let planning_repo = &fixture.clones[0];
    let code_repo = &fixture.code_clones[0];
    let base = git_at(code_repo, &["rev-parse", "HEAD"]);
    let lease = ClaimLease::acquire(
        planning_repo,
        fixture.project_uid,
        "task-cross-remote-lost-ack",
        "web",
        &base,
    )
    .unwrap()
    .unwrap();
    fs::write(code_repo.join("ack.txt"), "push applied\n").unwrap();
    git_at(code_repo, &["add", "ack.txt"]);
    git_at(
        code_repo,
        &["commit", "-qm", "prepare acknowledged candidate"],
    );
    let commit = git_at(code_repo, &["rev-parse", "HEAD"]);
    let destination = "refs/heads/koolade/cross-remote-lost-ack";

    let result = git::fenced_push_with_lost_ack(
        planning_repo,
        &lease.remote,
        &lease.reference,
        &lease.object,
        code_repo,
        &commit,
        destination,
    );

    assert_eq!(result.unwrap(), lease.object);
    assert_eq!(git_at(&fixture.remote, &["rev-parse", destination]), commit);
}

fn replacement_claim(planning_repo: &Path, lease: &ClaimLease) -> String {
    let mut replacement = lease.record().unwrap();
    replacement.session_id = uuid::Uuid::new_v4().to_string();
    replacement.operator_id = uuid::Uuid::new_v4().to_string();
    replacement.issued_at_utc = chrono::Utc::now().timestamp();
    replacement.expires_at_utc = replacement.issued_at_utc + 15 * 60;
    git::create_object(planning_repo, &replacement).unwrap()
}

fn push_claim_replacement(planning_repo: &Path, replacement: &str, reference: &str) {
    git_at(
        planning_repo,
        &[
            "push",
            "-q",
            "--force",
            "origin",
            &format!("{replacement}:{reference}"),
        ],
    );
}
