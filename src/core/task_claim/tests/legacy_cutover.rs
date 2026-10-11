use super::*;

#[test]
fn retiring_a_stale_legacy_ref_fences_an_old_atomic_publish() {
    let fixture = Fixture::new();
    let code_repo = &fixture.code_clones[0];
    let task_uid = "task-legacy-publication-fence";
    let session = "prior-session";
    let legacy_ref = legacy_reference(task_uid).unwrap();
    let now = chrono::Utc::now().timestamp();
    let legacy_record: ClaimRecord = serde_json::from_value(serde_json::json!({
        "schemaVersion": 2,
        "taskUid": task_uid,
        "owner": "Kool.ad/e session",
        "sessionId": session,
        "baseCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "claimedAt": now - 16 * 60,
        "takeoverHistory": []
    }))
    .unwrap();
    let stale_object = git::create_object(code_repo, &legacy_record).unwrap();
    git_at(
        code_repo,
        &[
            "push",
            "-q",
            "origin",
            &format!("{stale_object}:{legacy_ref}"),
        ],
    );
    let observed = git::read_legacy_claim(code_repo, task_uid)
        .unwrap()
        .unwrap();

    git::remove_expired_legacy_claim(code_repo, &observed, session).unwrap();

    // An older worker prepares an atomic branch-plus-lease renewal against
    // its observed SHA. If it races after retirement, the whole transaction
    // must fail instead of publishing only the code branch.
    let mut renewed_record = legacy_record;
    renewed_record.issued_at_utc = now;
    let renewed_object = git::create_object(code_repo, &renewed_record).unwrap();
    let branch = "refs/heads/koolade/legacy-publication-race";
    let candidate = git_at(code_repo, &["rev-parse", "HEAD"]);
    let lease = format!("--force-with-lease={legacy_ref}:{stale_object}");
    let branch_update = format!("{candidate}:{branch}");
    let claim_update = format!("{renewed_object}:{legacy_ref}");
    let raced_publish = git::run(
        code_repo,
        &[
            "push",
            "--atomic",
            "--porcelain",
            &lease,
            "origin",
            &branch_update,
            &claim_update,
        ],
        None,
    )
    .unwrap();
    assert!(
        !raced_publish.status.success(),
        "legacy atomic publication must lose after its observed claim is retired"
    );
    assert!(git_at(code_repo, &["ls-remote", "--heads", "origin", branch]).is_empty());
}
