use super::*;

#[test]
fn claim_reads_reject_remotes_without_bounded_fetch_support() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    let task_uid = "task-filter-required";
    let reference = reference(fixture.project_uid, task_uid).unwrap();
    let active = record(
        fixture.project_uid,
        task_uid,
        "other-session",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        chrono::Utc::now().timestamp() + 300,
    );
    let object = git::create_object(repo, &active).unwrap();
    git_at(
        repo,
        &["push", "-q", "origin", &format!("{object}:{reference}")],
    );
    git_at(
        &fixture.planning_remote,
        &["config", "--unset", "uploadpack.allowFilter"],
    );

    let error = ClaimLease::acquire(
        repo,
        fixture.project_uid,
        task_uid,
        "web",
        "1111111111111111111111111111111111111111",
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ClaimError::CoordinationRejected(message)
            if message.contains("bounded task-claim fetches")
    ));
}

#[test]
fn stale_takeover_rechecks_a_renewed_remote_claim_before_cas() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    let task_uid = "task-takeover-renewal-race";
    let reference = reference(fixture.project_uid, task_uid).unwrap();
    let expired = record(
        fixture.project_uid,
        task_uid,
        "stale-session",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        2,
    );
    let object = git::create_object(repo, &expired).unwrap();
    git_at(
        repo,
        &["push", "-q", "origin", &format!("{object}:{reference}")],
    );

    let result = git::take_over_stale_after_prepare(
        repo,
        fixture.project_uid,
        task_uid,
        "web",
        "stale-session",
        "1111111111111111111111111111111111111111",
        || {
            let now = chrono::Utc::now().timestamp();
            let mut renewed = expired.clone();
            renewed.issued_at_utc = now;
            renewed.expires_at_utc = now + 15 * 60;
            let renewed_object = git::create_object(repo, &renewed).unwrap();
            git_at(
                repo,
                &[
                    "push",
                    "-q",
                    "--force",
                    "origin",
                    &format!("{renewed_object}:{reference}"),
                ],
            );
        },
    );
    assert!(matches!(
        result,
        Err(ClaimError::AlreadyClaimed { stale: false, .. })
    ));
    let current: ClaimRecord = serde_json::from_str(&git_at(
        repo,
        &[
            "show",
            "-s",
            "--format=%B",
            &git_at(&fixture.planning_remote, &["rev-parse", &reference]),
        ],
    ))
    .unwrap();
    assert!(
        current.expires_at_utc > chrono::Utc::now().timestamp(),
        "renewed claim must retain its extended expiry"
    );
}

#[test]
fn failed_renewal_marks_the_old_owner_lost_without_releasing_the_new_claim() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    let base = git_at(&fixture.code_clones[0], &["rev-parse", "HEAD"]);
    let lease = ClaimLease::acquire(repo, fixture.project_uid, "task-renewal-lost", "web", &base)
        .unwrap()
        .unwrap();
    let reference = reference(fixture.project_uid, "task-renewal-lost").unwrap();
    let mut replacement = lease.record().unwrap();
    replacement.operator_id = uuid::Uuid::new_v4().to_string();
    replacement.session_id = uuid::Uuid::new_v4().to_string();
    replacement.issued_at_utc = chrono::Utc::now().timestamp();
    replacement.expires_at_utc = replacement.issued_at_utc + 15 * 60;
    let replacement_object = git::create_object(repo, &replacement).unwrap();
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

    let handle = ClaimLeaseHandle::new(lease);
    assert!(handle.refresh().is_err());
    assert!(handle.is_lost());
    drop(handle);
    assert_eq!(
        git_at(&fixture.planning_remote, &["rev-parse", &reference]),
        replacement_object
    );
}

#[test]
fn deleted_claim_blocks_publication_and_is_not_recreated_by_drop() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    let code_repo = &fixture.code_clones[0];
    let base = git_at(code_repo, &["rev-parse", "HEAD"]);
    let lease = ClaimLease::acquire(
        repo,
        fixture.project_uid,
        "task-deleted-claim",
        "web",
        &base,
    )
    .unwrap()
    .unwrap();
    let reference = reference(fixture.project_uid, "task-deleted-claim").unwrap();
    git_at(repo, &["push", "-q", "origin", &format!(":{reference}")]);

    assert!(lease.verify().is_err());
    let handle = ClaimLeaseHandle::new(lease);
    assert!(
        handle
            .fenced_push(code_repo, &base, "refs/heads/koolade/deleted-claim")
            .is_err()
    );
    drop(handle);
    assert!(
        git_at(
            &fixture.root,
            &[
                "ls-remote",
                "--refs",
                fixture.planning_remote.to_str().unwrap(),
                &reference,
            ]
        )
        .is_empty()
    );
    assert!(
        git_at(
            &fixture.root,
            &[
                "ls-remote",
                "--heads",
                fixture.remote.to_str().unwrap(),
                "refs/heads/koolade/deleted-claim",
            ]
        )
        .is_empty()
    );
}

#[test]
fn claim_clock_and_ref_identity_fail_closed() {
    let project_uid = uuid::Uuid::new_v4();
    let mut stale = record(
        project_uid,
        "task-456",
        "stale-session",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        2,
    );
    stale.issued_at_utc = 1_000;
    stale.expires_at_utc = 2_000;
    assert!(!stale.appears_stale(999));
    assert!(git::validate_clock(&stale, 999).is_err());
    assert!(!stale.appears_stale(1_999));
    assert!(stale.appears_stale(2_000));
    assert!(reference(project_uid, "../other").is_err());
}

#[test]
fn legacy_project_identity_ignores_transport_and_userinfo_variants() {
    let fixture = Fixture::new();
    git_at(
        &fixture.clones[0],
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            "https://user@GitHub.com/Org/Repo.git",
        ],
    );
    git_at(
        &fixture.clones[1],
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            "git@github.com:org/repo.git",
        ],
    );
    assert_eq!(
        git::legacy_project_uid(&fixture.clones[0]).unwrap(),
        git::legacy_project_uid(&fixture.clones[1]).unwrap()
    );
}

#[test]
fn repositories_without_a_planning_remote_keep_local_coordination_available() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    git_at(repo, &["remote", "remove", "origin"]);
    let base = git_at(&fixture.code_clones[0], &["rev-parse", "HEAD"]);
    assert!(
        ClaimLease::acquire(repo, fixture.project_uid, "task-offline", "web", &base)
            .unwrap()
            .is_none()
    );
}

#[test]
fn unavailable_shared_planning_remote_requires_explicit_offline_override() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    let missing = fixture.root.join("nonexistent-planning-remote.git");
    git_at(
        repo,
        &["remote", "set-url", "origin", missing.to_str().unwrap()],
    );
    let base = git_at(&fixture.code_clones[0], &["rev-parse", "HEAD"]);
    let request = || {
        ClaimRequest::new(
            repo.clone(),
            fixture.code_clones[0].clone(),
            fixture.project_uid,
            "task-no-remote".into(),
            "web".into(),
            base.clone(),
            None,
            ClaimScope::SharedRequired,
            false,
        )
    };
    assert!(matches!(
        request().acquire(),
        Err(ClaimError::RemoteUnavailable(_))
    ));
    let override_run = ClaimRequest::new(
        repo.clone(),
        fixture.code_clones[0].clone(),
        fixture.project_uid,
        "task-no-remote".into(),
        "web".into(),
        base.clone(),
        None,
        ClaimScope::SharedRequired,
        true,
    )
    .acquire()
    .unwrap();
    assert!(override_run.lease.is_none());
    assert_eq!(override_run.mode, ClaimAttemptMode::UnsafeOffline);
    assert!(
        override_run
            .warning
            .as_deref()
            .is_some_and(|text| text.contains("local queue lock only"))
    );
    let blocked = ClaimLeaseHandle::block_publication();
    assert!(
        blocked
            .fenced_push(repo, &base, "refs/heads/koolade/offline")
            .is_err()
    );
    assert!(matches!(
        request().acquire(),
        Err(ClaimError::RemoteUnavailable(_))
    ));
}
