use super::*;

#[test]
fn documentation_refresh_cannot_finish_without_document_updates() {
    use crate::core::planning_work::{Work, WorkKind, WorkStatus};
    let temp = tempfile::tempdir().unwrap();
    let mut project = crate::app::session::test_support::project_from(temp.path());
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
                spec_written: false,
                items_written: false,
                commit_message: "Fixture".into(),
                repo_relative_paths: vec![],
                synthesized_open_items: vec![],
            },
            normalized: Box::new(crate::core::validation::NormalizedTurn {
                assistant_message: "Refresh complete".into(),
                change_summary: None,
                spec_markdown: None,
                document_updates: updates,
                planning_tasks: vec![],
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
            commit_result: Ok("fixture".into()),
            elapsed: std::time::Duration::ZERO,
            stderr_tail: String::new(),
        };
        update(&mut project, "task:docs", &outcome, false, false);
        assert_eq!(project.planning_work[0].status, expected);
    }
}
