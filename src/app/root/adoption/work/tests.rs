use super::*;

#[test]
fn documentation_refresh_cannot_finish_without_document_updates() {
    use crate::core::planning_work::{Work, WorkKind, WorkStatus};
    let temp = tempfile::tempdir().unwrap();
    let code_root = temp.path().join("code");
    std::fs::create_dir_all(&code_root).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(["-C"])
            .arg(&code_root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Fixture User"]);
    git(&["config", "user.email", "fixture@example.invalid"]);
    std::fs::write(code_root.join("README.md"), "fixture\n").unwrap();
    git(&["add", "README.md"]);
    git(&["commit", "-qm", "fixture code"]);
    let mut project = crate::app::session::test_support::project_from(&code_root);
    let store = crate::artifacts::planning_store::PlanningStore::for_code_repository(
        project.state.planning_store.project_id,
        temp.path().join("private-state"),
        &code_root,
    );
    project.state = crate::core::state::PlannerState::load_with_store(&code_root, &store).unwrap();
    let mut work = Work::new(
        "task:docs".into(),
        "Refresh docs".into(),
        "Inspect docs".into(),
        "".into(),
    );
    work.kind = WorkKind::DocumentationRefresh;
    project.planning_work.push(work);
    for (updates, expected) in [
        (Vec::new(), WorkStatus::NeedsAttention),
        (
            vec![("product:overview".into(), "Verified documentation".into())],
            WorkStatus::Done,
        ),
    ] {
        let outcome = TurnOutcome::Applied {
            state: Box::new(project.state.clone()),
            receipt: crate::core::apply::ApplyReceipt {
                draft_written: false,
                items_written: false,
                summary: "Fixture".into(),
                changed_store_paths: vec![],
            },
            normalized: Box::new(crate::core::validation::NormalizedTurn {
                assistant_message: "Refresh complete".into(),
                change_summary: None,
                document_updates: updates,
                planning_tasks: vec![],
                task_intent_context: None,
                change_status_updates: Default::default(),
                additional_planning_artifacts: vec![],
                added: vec![],
                updates: vec![],
                resolved: vec![],
                next_question_id: None,
                requested_action: None,
                follow_up_task: None,
                warnings: vec![],
                workflow: None,
                task_batch: None,
                plan_comparison: None,
            }),
            save_result: Ok("fixture".into()),
            elapsed: std::time::Duration::ZERO,
            stderr_tail: String::new(),
        };
        update(&mut project, "task:docs", &outcome, false, false);
        assert_eq!(project.planning_work[0].status, expected);
    }
}
