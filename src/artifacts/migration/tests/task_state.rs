use super::*;

#[test]
fn task_move_preserves_queue_workflow_approvals_and_implementation_evidence() {
    let root = repo("task-identity");
    let old_ticket = "planning/tasks/demo/001-task.md";
    let new_ticket = format!("{}/{old_ticket}", crate::artifacts::layout::canonical::ROOT);
    let task_text = "# Keep the saved task identity\n\n## Acceptance criteria\n\n- Existing work remains attached.\n";
    fs::create_dir_all(root.join(old_ticket).parent().unwrap()).unwrap();
    fs::write(root.join(old_ticket), task_text).unwrap();
    fs::create_dir_all(root.join(".planner")).unwrap();
    fs::write(
        root.join(".planner/workflow.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "brief": null,
            "reviewedSpecification": null,
            "taskBatches": [{"feature": "demo", "directory": "planning/tasks/demo", "count": 1}],
            "approvedFeatures": {"CHG-001": "frozen feature contract"}
        }))
        .unwrap(),
    )
    .unwrap();
    commit_all(&root, "legacy task");

    let old_record_dir = common_dir(&root)
        .unwrap()
        .join("koolade-implementations")
        .join(crate::core::implementation::key_for_ticket(old_ticket));
    let old_report = old_record_dir.join("123-report.json");
    let blocker = format!(
        "## Waiting for user action\n\nThe saved report needs review.\n\n### Next action(s)\n\n- Adjudicator: choose (a) accept or (b) revise.\n\nFull report: {}",
        old_report.display()
    );
    let mut queue = crate::core::implementation_queue::Queue::default();
    queue.current_ticket = Some(old_ticket.into());
    queue.in_flight.insert(old_ticket.into());
    queue.blocked.insert(
        old_ticket.into(),
        crate::core::implementation::Failure::other(blocker.clone()),
    );
    queue.recovery_attempts.insert(old_ticket.into(), 2);
    let common = common_dir(&root).unwrap();
    fs::write(
        common.join("koolade-queue.json"),
        serde_json::to_vec_pretty(&queue).unwrap(),
    )
    .unwrap();

    fs::create_dir_all(&old_record_dir).unwrap();
    fs::write(old_record_dir.join("run.lock"), "").unwrap();
    fs::write(
        old_record_dir.join("123-report.json"),
        "saved blocker report",
    )
    .unwrap();
    let record = crate::core::implementation::Implementation {
        ticket: old_ticket.into(),
        task_uid: None,
        ticket_text: task_text.into(),
        approved_specification: Some("frozen approved specification".into()),
        approved_product_context: Some("frozen product context".into()),
        completed_dependency_context: None,
        branch: "koolade/old-task-branch".into(),
        source_branch: None,
        destination_branch: None,
        base: "main".into(),
        base_commit: "base-commit".into(),
        worktree: root.join("../.koolade-worktrees/demo/old-ticket"),
        status: crate::core::implementation::ImplementationStatus::Blocked,
        detail: blocker.clone(),
        pr_url: None,
        verified_head: None,
        auto_merge: false,
        merged_commit: None,
        pr_state: None,
        pr_checked_at: None,
        pr_check_attempted_at: None,
        pr_check_error: None,
        independent_check: None,
        cleanup: Default::default(),
    };
    fs::write(
        old_record_dir.join("state.json"),
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();

    run(&root).unwrap();
    let ticket_path = root.join(&new_ticket);
    assert_eq!(
        crate::artifacts::task_docs::visible_content(&fs::read_to_string(&ticket_path).unwrap()),
        task_text
    );
    assert!(!root.join(old_ticket).exists());

    let new_record_dir = root
        .join(crate::artifacts::layout::canonical::IMPLEMENTATION)
        .join(crate::core::implementation::key_for_ticket(&new_ticket));
    let migrated: crate::core::implementation::Implementation =
        serde_json::from_slice(&fs::read(new_record_dir.join("state.json")).unwrap()).unwrap();
    assert_eq!(migrated.ticket, new_ticket);
    let task_identity =
        crate::domain::ArtifactIdentity::from_markdown(&fs::read_to_string(&ticket_path).unwrap())
            .unwrap()
            .unwrap();
    assert_eq!(
        migrated.task_uid.as_deref(),
        Some(task_identity.uid.as_str())
    );
    assert_eq!(migrated.ticket_text, task_text);
    assert_eq!(migrated.worktree, record.worktree);
    assert_eq!(
        migrated.status,
        crate::core::implementation::ImplementationStatus::Blocked
    );
    assert_eq!(
        fs::read_to_string(new_record_dir.join("123-report.json")).unwrap(),
        "saved blocker report"
    );
    assert!(migrated.detail.contains(&format!(
        "Full report: {}",
        new_record_dir.join("123-report.json").display()
    )));
    assert!(!old_record_dir.exists());

    let migrated_queue: crate::core::implementation_queue::Queue =
        serde_json::from_slice(&fs::read(common.join("koolade-queue.json")).unwrap()).unwrap();
    assert_eq!(
        migrated_queue.current_ticket.as_deref(),
        Some(new_ticket.as_str())
    );
    assert!(migrated_queue.in_flight.contains(&new_ticket));
    assert!(
        migrated_queue.blocked[&new_ticket]
            .message
            .contains(&format!(
                "Full report: {}",
                new_record_dir.join("123-report.json").display()
            ))
    );
    assert_eq!(migrated_queue.recovery_attempts[&new_ticket], 2);
    assert_eq!(
        crate::core::attention::source_path(
            &root,
            &new_ticket,
            &migrated_queue.blocked[&new_ticket].message
        ),
        Some(new_record_dir.join("123-report.json"))
    );

    let workflow: crate::core::workflow::Workflow = serde_json::from_slice(
        &fs::read(root.join(crate::artifacts::layout::canonical::WORKFLOW)).unwrap(),
    )
    .unwrap();
    assert_eq!(
        workflow.task_batches[0].directory,
        format!("{}/demo", crate::artifacts::layout::canonical::TASKS)
    );
    assert_eq!(
        workflow.approved_features["CHG-001"],
        "frozen feature contract"
    );
    assert!(run(&root).unwrap().is_empty(), "migration is idempotent");
    let _ = fs::remove_dir_all(root);
}
