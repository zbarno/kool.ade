use super::*;

#[test]
fn independent_planning_clones_race_for_one_atomic_project_task_claim() {
    let fixture = Fixture::new();
    let barrier = Arc::new(Barrier::new(3));
    let threads = fixture
        .clones
        .clone()
        .into_iter()
        .map(|repo| {
            let barrier = barrier.clone();
            let project_uid = fixture.project_uid;
            let base = git_at(&fixture.code_clones[0], &["rev-parse", "HEAD"]);
            std::thread::spawn(move || {
                barrier.wait();
                ClaimLease::acquire(&repo, project_uid, "task-123", "web", &base)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Ok(Some(_))))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(ClaimError::AlreadyClaimed { .. })))
            .count(),
        1
    );
    let winner_index = results
        .iter()
        .position(|result| matches!(result, Ok(Some(_))))
        .unwrap();
    let lease = match results.into_iter().nth(winner_index).unwrap().unwrap() {
        Some(lease) => lease,
        None => unreachable!(),
    };
    let record = lease.record().unwrap();
    assert_eq!(
        record.project_uid,
        fixture.project_uid.hyphenated().to_string()
    );
    assert_eq!(record.task_uid, "task-123");
    assert_eq!(record.code_repository_id, "web");
    assert!(record.operator_id.parse::<uuid::Uuid>().is_ok());
    assert!(record.session_id.parse::<uuid::Uuid>().is_ok());
    assert!(record.expires_at_utc > record.issued_at_utc);
    let payload = serde_json::to_value(&record).unwrap();
    for field in [
        "projectUid",
        "taskUid",
        "operatorId",
        "sessionId",
        "codeRepositoryId",
        "baseCommit",
        "issuedAtUtc",
        "expiresAtUtc",
    ] {
        assert!(payload.get(field).is_some(), "missing claim field {field}");
    }
    assert_eq!(
        record.base_commit,
        git_at(&fixture.code_clones[winner_index], &["rev-parse", "HEAD"])
    );
    let reference = reference(fixture.project_uid, "task-123").unwrap();
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
    assert!(
        !git_at(
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
    drop(lease);
    assert!(
        git_at(
            &fixture.planning_remote,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads/koolade/claims"
            ]
        )
        .is_empty()
    );
}

#[test]
fn distinct_task_uids_and_project_uuids_do_not_contend() {
    let fixture = Fixture::new();
    let base = git_at(&fixture.code_clones[0], &["rev-parse", "HEAD"]);
    let first = claim(&fixture, &fixture.clones[0], "task-a")
        .unwrap()
        .unwrap();
    let second = claim(&fixture, &fixture.clones[1], "task-b")
        .unwrap()
        .unwrap();
    let other_project = ClaimLease::acquire(
        &fixture.clones[1],
        uuid::Uuid::new_v4(),
        "task-a",
        "web",
        &base,
    )
    .unwrap()
    .unwrap();
    assert_ne!(first.reference, second.reference);
    assert_ne!(first.reference, other_project.reference);
    assert!(matches!(
        ClaimLease::acquire(
            &fixture.clones[1],
            fixture.project_uid,
            "task-a",
            "api",
            &base
        ),
        Err(ClaimError::AlreadyClaimed { .. })
    ));
}

#[test]
fn stale_claim_takeover_requires_the_observed_session_and_rechecks_expiry() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    let code = &fixture.code_clones[0];
    let base = git_at(code, &["rev-parse", "HEAD"]);
    let reference = reference(fixture.project_uid, "task-456").unwrap();
    let stale = record(fixture.project_uid, "task-456", "stale-session", &base, 2);
    let object = git::create_object(repo, &stale).unwrap();
    git_at(
        repo,
        &["push", "-q", "origin", &format!("{object}:{reference}")],
    );
    let recovered = ClaimLease::take_over_stale(
        repo,
        fixture.project_uid,
        "task-456",
        "web",
        "stale-session",
        "1111111111111111111111111111111111111111",
    )
    .unwrap();
    let current = recovered.record().unwrap();
    assert_ne!(current.session_id, "stale-session");
    assert_eq!(
        current.base_commit,
        "1111111111111111111111111111111111111111"
    );
    assert_eq!(current.schema_version, 4);
    assert_eq!(current.takeover_history.len(), 1);
    assert_eq!(current.takeover_history[0].session_id, "stale-session");
    assert_eq!(current.takeover_history[0].claimed_at, 1);
    assert!(matches!(
        ClaimLease::take_over_stale(
            repo,
            fixture.project_uid,
            "task-456",
            "web",
            "stale-session",
            "other"
        ),
        Err(ClaimError::AlreadyClaimed { .. })
    ));
}

#[test]
fn stale_claim_takeover_can_change_code_repository_route_with_history() {
    let fixture = Fixture::new();
    let repo = &fixture.clones[0];
    let base = git_at(&fixture.code_clones[0], &["rev-parse", "HEAD"]);
    let task_uid = "task-route-recovery";
    let reference = reference(fixture.project_uid, task_uid).unwrap();
    let mut stale = record(fixture.project_uid, task_uid, "stale-session", &base, 2);
    stale.code_repository_id = "old-web-route".into();
    let object = git::create_object(repo, &stale).unwrap();
    git_at(
        repo,
        &["push", "-q", "origin", &format!("{object}:{reference}")],
    );

    let recovered = ClaimLease::take_over_stale(
        repo,
        fixture.project_uid,
        task_uid,
        "new-api-route",
        "stale-session",
        &base,
    )
    .unwrap();
    let current = recovered.record().unwrap();
    assert_eq!(current.code_repository_id, "new-api-route");
    assert_eq!(current.takeover_history.len(), 1);
    assert_eq!(
        current.takeover_history[0].code_repository_id,
        "old-web-route"
    );
}
