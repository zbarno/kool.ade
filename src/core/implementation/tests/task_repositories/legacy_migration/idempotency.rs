use super::*;

struct StopAfterMigration;

impl AiHarness for StopAfterMigration {
    fn label(&self) -> String {
        "migration interruption fixture".into()
    }

    fn check_available(&self) -> Result<String, AppError> {
        Ok("fixture".into())
    }

    fn execute(
        &self,
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, AppError> {
        assert_eq!(
            fs::read_to_string(request.repo_root.join("tracked.txt")).unwrap(),
            "unstaged change\n"
        );
        assert!(request.repo_root.join("untracked\nname.txt").is_file());
        Err(AppError::HarnessFailed {
            reason: "simulated stop after legacy migration".into(),
            stderr_tail: String::new(),
        })
    }
}

#[test]
fn completed_migration_is_idempotent_after_task_state_save_interruption() {
    let sandbox = Sandbox::new();
    let (state, worktree, original_status) = setup_legacy_workspace(&sandbox);
    let destination = clone_destination(&sandbox, &state);
    let first_error = resume_with_harness(&sandbox, &StopAfterMigration)
        .unwrap_err()
        .to_string();
    assert!(first_error.contains("simulated stop after legacy migration"));

    // Simulate a crash after the migration record was marked complete but before
    // the migrated task state reached its final clone identity on disk.
    let state_path = state_dir(&sandbox.repo, &sandbox.ticket)
        .unwrap()
        .join("state.json");
    let mut saved: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    saved["task_repository_kind"] = serde_json::json!("legacy_worktree");
    saved["task_repository"] = serde_json::json!(worktree);
    saved["task_repositories"] = serde_json::json!([worktree]);
    saved["task_repository_ready"] = serde_json::json!(false);
    saved["status"] = serde_json::json!("preparing");
    fs::write(&state_path, serde_json::to_vec_pretty(&saved).unwrap()).unwrap();

    let second_error = resume_with_harness(&sandbox, &StopAfterMigration)
        .unwrap_err()
        .to_string();
    assert!(second_error.contains("simulated stop after legacy migration"));
    let migrated: serde_json::Value =
        serde_json::from_slice(&fs::read(&state_path).unwrap()).unwrap();
    assert_eq!(migrated["task_repository_kind"], "clone");
    assert_eq!(migrated["task_repository"], serde_json::json!(destination));
    assert_eq!(
        fs::read_to_string(destination.join("untracked\nname.txt")).unwrap(),
        "untracked data\n"
    );
    assert_eq!(
        sandbox.git(&worktree, &["status", "--porcelain"]),
        original_status
    );
    let migration_record: serde_json::Value = serde_json::from_slice(
        &fs::read(
            state_dir(&sandbox.repo, &sandbox.ticket)
                .unwrap()
                .join("legacy-migration/legacy-migration.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(migration_record["snapshot_complete"], true);
    assert_eq!(migration_record["workspace_complete"], true);
}
