use super::*;
use crate::{
    core::workflow::{TaskBatchRef, Workflow},
    domain::ArtifactIdentity,
};

#[test]
fn board_state_follows_task_uid_after_story_path_changes() {
    let root = std::env::temp_dir().join(format!(
        "koolade_board_identity_{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let directory = ".koolade-packet/planning/tasks/renamed-batch";
    let ticket = format!("{directory}/F7-TASK-renamed.md");
    let readme =
        ArtifactIdentity::preserve_markdown("# Renamed batch\n", None, "batch-1", "Renamed batch")
            .unwrap();
    let batch_identity = ArtifactIdentity::from_markdown(&readme).unwrap().unwrap();
    let task = ArtifactIdentity::preserve_markdown_with_parent(
        "# F7-TASK-renamed — Continue saved work\n\nKeep the original worktree.\n",
        None,
        "F7-TASK-renamed",
        "Continue saved work",
        Some(&batch_identity.uid),
    )
    .unwrap();
    let task_uid = ArtifactIdentity::from_markdown(&task).unwrap().unwrap().uid;
    let batch_path = root.join(directory);
    fs::create_dir_all(&batch_path).unwrap();
    fs::write(batch_path.join("README.md"), readme).unwrap();
    fs::write(root.join(&ticket), &task).unwrap();
    let workflow = Workflow {
        task_batches: vec![TaskBatchRef {
            identity: Some(batch_identity),
            feature: "Saved searches".into(),
            directory: directory.into(),
            count: 1,
        }],
        ..Workflow::default()
    };
    crate::artifacts::task_docs::save_workflow(&root, &workflow).unwrap();

    let old_ticket = "planning/tasks/old-name/001-task.md";
    let record = Implementation {
        ticket: old_ticket.into(),
        task_uid: Some(task_uid.clone()),
        ticket_text: crate::artifacts::task_docs::visible_content(&task),
        approved_specification: None,
        approved_product_context: None,
        completed_dependency_context: None,
        branch: "koolade/old-name".into(),
        base: "main".into(),
        base_commit: "base".into(),
        worktree: root.join("worktree"),
        status: ImplementationStatus::Blocked,
        detail: "Saved work remains available.".into(),
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
    let state_dir = state_dir(&root, old_ticket).unwrap();
    fs::create_dir_all(&state_dir).unwrap();
    fs::write(
        state_dir.join("state.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();

    let board = load_board_states(&root);
    let matched = &board[&ticket];
    assert_eq!(matched.ticket, ticket);
    assert_eq!(matched.task_uid.as_deref(), Some(task_uid.as_str()));
    assert_eq!(matched.status, ImplementationStatus::Blocked);
    assert!(!board.contains_key(old_ticket));
    let _ = fs::remove_dir_all(root);
}
