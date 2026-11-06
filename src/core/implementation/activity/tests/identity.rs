use super::*;

#[test]
fn uid_scoped_legacy_activity_is_sanitized_then_removed_after_migration() {
    let repo = temp_repo();
    let ticket = "planning/tasks/001-task.md";
    let uid = "task-a";
    let legacy_dir = super::super::state_dir(&repo, ticket).unwrap();
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::write(
        legacy_dir.join("activity.json"),
        serde_json::to_vec(&crate::harness::LiveProgress {
            task_uid: Some(uid.into()),
            response: "Authorization: Bearer synthetic-legacy-secret".into(),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();

    let loaded = load_activity_for_task(&repo, ticket, Some(uid)).unwrap();

    assert!(!loaded.response.contains("synthetic-legacy-secret"));
    assert!(!legacy_dir.join("activity.json").exists());
    assert!(
        super::super::activity_path_for_task(&repo, ticket, Some(uid))
            .unwrap()
            .exists()
    );
    std::fs::remove_dir_all(repo).unwrap();
}

#[test]
fn activity_snapshots_are_isolated_by_stable_task_identity() {
    let repo = temp_repo();
    let ticket = "planning/tasks/001-task.md";
    for (uid, content) in [("task-a", "alpha"), ("task-b", "beta")] {
        let progress = crate::harness::LiveProgress {
            task_uid: Some(uid.into()),
            response: content.into(),
            ..Default::default()
        };
        save_activity(&repo, ticket, &progress).unwrap();
    }
    let legacy_dir = super::super::state_dir(&repo, ticket).unwrap();
    fs::create_dir_all(&legacy_dir).unwrap();
    fs::write(
        legacy_dir.join("activity.json"),
        serde_json::to_vec(&crate::harness::LiveProgress {
            response: "unscoped legacy history".into(),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();

    assert_eq!(
        load_activity_for_task(&repo, ticket, Some("task-a"))
            .unwrap()
            .response,
        "alpha"
    );
    assert_eq!(
        load_activity_for_task(&repo, ticket, Some("task-b"))
            .unwrap()
            .response,
        "beta"
    );
    assert!(load_activity_for_task(&repo, ticket, Some("task-c")).is_none());
    fs::remove_dir_all(repo).unwrap();
}

#[test]
fn stable_task_identity_finds_a_live_lock_after_path_change() {
    let repo = temp_repo();
    let old_ticket = "planning/tasks/old/001-task.md";
    let new_ticket = "planning/tasks/renamed/001-task.md";
    let task_uid = uuid::Uuid::new_v4().to_string();
    let dir = super::super::state_dir(&repo, old_ticket).unwrap();
    fs::create_dir_all(&dir).unwrap();
    let state = crate::core::implementation::Implementation {
        ticket: old_ticket.into(),
        task_uid: Some(task_uid.clone()),
        ticket_text: String::new(),
        approved_specification: None,
        approved_product_context: None,
        completed_dependency_context: None,
        manager_repair_context: None,
        branch: "koolade/task".into(),
        source_branch: None,
        destination_branch: None,
        source_ref: None,
        source_commit: None,
        repository_id: None,
        project_id: None,
        workspace_uid: None,
        repository_identity: None,
        push_repository: None,
        repository_cache: None,
        task_repository_allocation_key: None,
        base: "main".into(),
        base_commit: "base".into(),
        task_repository: repo.join("worktree"),
        task_repository_kind: TaskRepositoryKind::Clone,
        task_repository_ready: false,
        task_repositories: vec![repo.join("worktree")],
        task_repository_commits: std::collections::BTreeMap::new(),
        status: ImplementationStatus::Interrupted,
        detail: String::new(),
        pr_url: None,
        verified_head: None,
        auto_merge: false,
        merged_commit: None,
        pr_state: None,
        pr_details: None,
        pr_checked_at: None,
        pr_check_attempted_at: None,
        pr_check_error: None,
        independent_check: None,
        cleanup: Default::default(),
    };
    fs::write(
        dir.join("state.json"),
        crate::core::implementation::serialize_state(&state).unwrap(),
    )
    .unwrap();
    save_activity(
        &repo,
        old_ticket,
        &crate::harness::LiveProgress {
            task_uid: Some(task_uid.clone()),
            response: "retained task activity".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        load_activity_for_task(&repo, new_ticket, Some(&task_uid))
            .unwrap()
            .response,
        "retained task activity"
    );
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("run.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let mut activity = unfinished_activity();
    assert!(!finalize_terminal_activity_if_stale(
        &repo,
        new_ticket,
        Some(&task_uid),
        ImplementationStatus::Interrupted,
        &mut activity,
        30,
    ));
    assert_eq!(activity.telemetry.finished_ms, None);
    drop(lock);
    fs::remove_dir_all(repo).unwrap();
}
