use super::*;

#[test]
fn claims_use_the_planning_push_url_while_code_publication_uses_code_remote() {
    let fixture = Fixture::new();
    let push_remote = fixture.root.join("planning-push-only.git");
    git_at(
        &fixture.root,
        &["init", "--bare", "-q", push_remote.to_str().unwrap()],
    );
    git_at(&push_remote, &["config", "uploadpack.allowFilter", "true"]);
    let planning_repo = &fixture.clones[0];
    git_at(
        planning_repo,
        &[
            "remote",
            "set-url",
            "--push",
            "origin",
            push_remote.to_str().unwrap(),
        ],
    );
    let base = git_at(&fixture.code_clones[0], &["rev-parse", "HEAD"]);
    let mut lease = ClaimLease::acquire(
        planning_repo,
        fixture.project_uid,
        "task-push-url",
        "web",
        &base,
    )
    .unwrap()
    .unwrap();
    let reference = reference(fixture.project_uid, "task-push-url").unwrap();

    lease.verify().unwrap();
    lease.refresh().unwrap();
    lease.verify().unwrap();
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
        !git_at(
            &fixture.root,
            &[
                "ls-remote",
                "--refs",
                push_remote.to_str().unwrap(),
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
                "--refs",
                fixture.remote.to_str().unwrap(),
                &reference,
            ]
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
            ]
        )
        .is_empty()
    );
}

#[test]
fn multiple_planning_push_urls_are_rejected_before_creating_a_claim() {
    let fixture = Fixture::new();
    let first = fixture.root.join("first-planning-push.git");
    let second = fixture.root.join("second-planning-push.git");
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
    let base = git_at(&fixture.code_clones[0], &["rev-parse", "HEAD"]);

    assert!(matches!(
        ClaimLease::acquire(
            repo,
            fixture.project_uid,
            "task-multiple-push-urls",
            "web",
            &base
        ),
        Err(ClaimError::CoordinationRejected(_))
    ));
    let reference = reference(fixture.project_uid, "task-multiple-push-urls").unwrap();
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
fn unsafe_planning_git_configuration_blocks_claim_execution() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    git_at(
        repo,
        &[
            "config",
            "--local",
            "core.fsmonitor",
            "!echo injected-fsmonitor",
        ],
    );
    let base = git_at(&fixture.code_clones[0], &["rev-parse", "HEAD"]);
    assert!(matches!(
        ClaimLease::acquire(repo, fixture.project_uid, "unsafe-config", "web", &base),
        Err(ClaimError::RemoteUnavailable(message)) if message.contains("security validation")
    ));
    let refname = reference(fixture.project_uid, "unsafe-config").unwrap();
    assert!(
        git_at(
            &fixture.root,
            &[
                "ls-remote",
                "--refs",
                fixture.planning_remote.to_str().unwrap(),
                &refname,
            ]
        )
        .is_empty()
    );
}

#[test]
fn prior_code_remote_claims_block_and_require_explicit_stale_takeover() {
    let fixture = Fixture::new();
    let planning_repo = fixture.clones[0].clone();
    let code_repo = fixture.code_clones[0].clone();
    let task_uid = "task-cutover-legacy-claim";
    let legacy_ref = legacy_reference(task_uid).unwrap();
    let project_ref = reference(fixture.project_uid, task_uid).unwrap();
    let session = "prior-session";
    let now = chrono::Utc::now().timestamp();
    let legacy_record: ClaimRecord = serde_json::from_value(serde_json::json!({
        "schemaVersion": 2,
        "taskUid": task_uid,
        "owner": "Kool.ad/e session",
        "sessionId": session,
        "baseCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "claimedAt": now,
        "takeoverHistory": []
    }))
    .unwrap();
    let active_object = git::create_object(&code_repo, &legacy_record).unwrap();
    git_at(
        &code_repo,
        &[
            "push",
            "-q",
            "origin",
            &format!("{active_object}:{legacy_ref}"),
        ],
    );
    let base = git_at(&code_repo, &["rev-parse", "HEAD"]);
    let request = |stale_session| {
        ClaimRequest::new(
            planning_repo.clone(),
            code_repo.clone(),
            fixture.project_uid,
            task_uid.into(),
            "web".into(),
            base.clone(),
            stale_session,
            ClaimScope::SharedRequired,
            false,
        )
    };

    assert!(matches!(
        request(None).acquire(),
        Err(ClaimError::AlreadyClaimed { stale: false, .. })
    ));
    assert!(
        git_at(
            &fixture.root,
            &[
                "ls-remote",
                "--refs",
                fixture.planning_remote.to_str().unwrap(),
                &project_ref,
            ]
        )
        .is_empty(),
        "an active legacy claim must prevent new project-scoped acquisition"
    );

    let mut expired_record = legacy_record;
    expired_record.issued_at_utc = now - 16 * 60;
    expired_record.expires_at_utc = 0;
    let expired_object = git::create_object(&code_repo, &expired_record).unwrap();
    git_at(
        &code_repo,
        &[
            "push",
            "-q",
            "--force",
            "origin",
            &format!("{expired_object}:{legacy_ref}"),
        ],
    );
    assert!(matches!(
        request(None).acquire(),
        Err(ClaimError::AlreadyClaimed { stale: true, .. })
    ));

    let attempt = request(Some(session.into())).acquire().unwrap();
    assert_eq!(attempt.mode, ClaimAttemptMode::Shared);
    let lease = attempt.lease.as_ref().unwrap();
    assert_eq!(
        lease.record().unwrap().project_uid,
        fixture.project_uid.hyphenated().to_string()
    );
    assert!(
        git_at(
            &fixture.root,
            &[
                "ls-remote",
                "--refs",
                fixture.remote.to_str().unwrap(),
                &legacy_ref,
            ]
        )
        .is_empty(),
        "stale takeover must CAS-remove the old code-remote claim"
    );
    assert!(
        !git_at(
            &fixture.root,
            &[
                "ls-remote",
                "--refs",
                fixture.planning_remote.to_str().unwrap(),
                &project_ref,
            ]
        )
        .is_empty(),
        "the replacement global claim belongs on the planning remote"
    );
}
