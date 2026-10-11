use super::*;

fn share_push_destination(fixture: &Fixture, planning_repo: &Path) {
    git_at(
        planning_repo,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            fixture.remote.to_str().unwrap(),
        ],
    );
}

#[test]
fn same_remote_publication_atomically_renews_claim_with_code_branch() {
    let fixture = Fixture::new();
    let planning_repo = &fixture.clones[0];
    let code_repo = &fixture.code_clones[0];
    share_push_destination(&fixture, planning_repo);
    let base = git_at(code_repo, &["rev-parse", "HEAD"]);
    let mut lease = ClaimLease::acquire(
        planning_repo,
        fixture.project_uid,
        "task-same-remote-atomic-publish",
        "web",
        &base,
    )
    .unwrap()
    .unwrap();
    let mut original = lease.record().unwrap();
    original.expires_at_utc = original.issued_at_utc + 60;
    let old_object = git::create_object(planning_repo, &original).unwrap();
    git_at(
        planning_repo,
        &[
            "push",
            "-q",
            "--force",
            "origin",
            &format!("{old_object}:{}", lease.reference),
        ],
    );
    lease.object = old_object.clone();
    original = lease.record().unwrap();
    fs::write(code_repo.join("same-remote.txt"), "published atomically\n").unwrap();
    git_at(code_repo, &["add", "same-remote.txt"]);
    git_at(
        code_repo,
        &["commit", "-qm", "prepare same-remote candidate"],
    );
    let candidate = git_at(code_repo, &["rev-parse", "HEAD"]);
    let destination = "refs/heads/koolade/same-remote-atomic-candidate";

    lease
        .fenced_push(code_repo, &candidate, destination)
        .unwrap();

    assert_ne!(lease.object, old_object);
    let renewed = lease.record().unwrap();
    assert_eq!(renewed.session_id, original.session_id);
    assert!(renewed.expires_at_utc > original.expires_at_utc);
    assert_eq!(
        git_at(
            &fixture.root,
            &[
                "ls-remote",
                "--heads",
                fixture.remote.to_str().unwrap(),
                destination,
            ]
        )
        .split_whitespace()
        .next(),
        Some(candidate.as_str())
    );
}

#[test]
fn same_remote_atomic_publish_loses_takeover_after_final_claim_read() {
    let fixture = Fixture::new();
    let planning_repo = &fixture.clones[0];
    let code_repo = &fixture.code_clones[0];
    share_push_destination(&fixture, planning_repo);
    let base = git_at(code_repo, &["rev-parse", "HEAD"]);
    let lease = ClaimLease::acquire(
        planning_repo,
        fixture.project_uid,
        "task-same-remote-takeover-race",
        "web",
        &base,
    )
    .unwrap()
    .unwrap();
    fs::write(code_repo.join("raced.txt"), "must remain local\n").unwrap();
    git_at(code_repo, &["add", "raced.txt"]);
    git_at(code_repo, &["commit", "-qm", "prepare raced candidate"]);
    let candidate = git_at(code_repo, &["rev-parse", "HEAD"]);
    let expected_object = lease.object.clone();
    let reference = lease.reference.clone();
    let remote = lease.remote.clone();
    let destination = "refs/heads/koolade/same-remote-raced-candidate";
    let mut replacement = lease.record().unwrap();
    replacement.operator_id = uuid::Uuid::new_v4().to_string();
    replacement.session_id = uuid::Uuid::new_v4().to_string();
    replacement.issued_at_utc = chrono::Utc::now().timestamp();
    replacement.expires_at_utc = replacement.issued_at_utc + 15 * 60;
    let replacement_object = git::create_object(planning_repo, &replacement).unwrap();

    let result = git::fenced_push_after_prepare(
        planning_repo,
        &remote,
        &reference,
        &expected_object,
        code_repo,
        &candidate,
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
    assert_eq!(
        git_at(&fixture.remote, &["rev-parse", &reference]),
        replacement_object
    );
    assert!(
        !Command::new("git")
            .args(["show-ref", "--verify", "--quiet", destination])
            .current_dir(&fixture.remote)
            .status()
            .unwrap()
            .success(),
        "an atomic failure must not leave the code branch published"
    );
}
