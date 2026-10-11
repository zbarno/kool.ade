use super::*;

#[test]
fn previous_claim_payloads_keep_their_original_expiry_semantics() {
    let record: ClaimRecord = serde_json::from_value(serde_json::json!({
        "schemaVersion": 2,
        "taskUid": "legacy-task",
        "owner": "Kool.ad/e session",
        "sessionId": "legacy-session",
        "baseCommit": "abc",
        "claimedAt": 1_000,
        "takeoverHistory": []
    }))
    .unwrap();
    assert_eq!(record.issued_at_utc, 1_000);
    assert_eq!(record.expires_at(), 1_000 + 15 * 60);
    assert!(record.project_uid.is_empty());
}

#[test]
fn previous_takeover_history_without_repository_route_deserializes() {
    let record: ClaimRecord = serde_json::from_value(serde_json::json!({
        "schemaVersion": 3,
        "projectUid": uuid::Uuid::new_v4().to_string(),
        "taskUid": "legacy-task",
        "owner": "Kool.ad/e session",
        "operatorId": "operator",
        "sessionId": "session",
        "codeRepositoryId": "web",
        "baseCommit": "abc",
        "issuedAtUtc": 1_000,
        "expiresAtUtc": 2_000,
        "takeoverHistory": [{
            "owner": "prior owner",
            "sessionId": "prior-session",
            "baseCommit": "prior-base",
            "claimedAt": 900,
            "replacedAt": 1_000
        }]
    }))
    .unwrap();
    assert_eq!(record.takeover_history[0].code_repository_id, "");
}

#[test]
fn conflicting_legacy_and_workspace_claims_are_both_preserved() {
    let fixture = Fixture::new();
    let planning_repo = &fixture.clones[0];
    let code_repo = &fixture.code_clones[0];
    let task_uid = "task-dual-claim-conflict";
    let legacy_ref = legacy_reference(task_uid).unwrap();
    let workspace_ref = reference(fixture.project_uid, task_uid).unwrap();
    let now = chrono::Utc::now().timestamp();
    let mut legacy: ClaimRecord = serde_json::from_value(serde_json::json!({
        "schemaVersion": 2,
        "taskUid": task_uid,
        "owner": "legacy owner",
        "sessionId": "stale-legacy-session",
        "baseCommit": "legacy-base",
        "claimedAt": now - 16 * 60,
        "takeoverHistory": []
    }))
    .unwrap();
    legacy.issued_at_utc = now - 16 * 60;
    legacy.expires_at_utc = 0;
    let legacy_object = git::create_object(code_repo, &legacy).unwrap();
    git_at(
        code_repo,
        &[
            "push",
            "-q",
            "origin",
            &format!("{legacy_object}:{legacy_ref}"),
        ],
    );

    let workspace = record(
        fixture.project_uid,
        task_uid,
        "workspace-session",
        "workspace-base",
        now + 15 * 60,
    );
    let workspace_object = git::create_object(planning_repo, &workspace).unwrap();
    git_at(
        planning_repo,
        &[
            "push",
            "-q",
            "origin",
            &format!("{workspace_object}:{workspace_ref}"),
        ],
    );

    let result = ClaimRequest::new(
        planning_repo.clone(),
        code_repo.clone(),
        fixture.project_uid,
        task_uid.into(),
        "web".into(),
        "candidate-base".into(),
        Some("stale-legacy-session".into()),
        ClaimScope::SharedRequired,
        false,
    )
    .acquire();
    assert!(matches!(
        result,
        Err(ClaimError::CoordinationRejected(message))
            if message.contains("both refs are preserved")
    ));
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
        .contains(&legacy_object)
    );
    assert!(
        git_at(
            &fixture.root,
            &[
                "ls-remote",
                "--refs",
                fixture.planning_remote.to_str().unwrap(),
                &workspace_ref,
            ]
        )
        .contains(&workspace_object)
    );
}
