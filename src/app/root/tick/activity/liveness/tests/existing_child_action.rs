use super::*;

#[test]
fn an_existing_child_action_prevents_a_duplicate_liveness_task() {
    for status in [
        WorkStatus::Todo,
        WorkStatus::InReview,
        WorkStatus::NeedsAttention,
    ] {
        let mut app = crate::app::root::board_tests::fixture();
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            unreachable!()
        };
        let mut root = Work::new(
            format!("task:liveness-existing-action:{status:?}"),
            "A planning goal".into(),
            "Keep the approved goal.".into(),
            "Planning started".into(),
        );
        root.task_intent_uid = Some(root.uid.clone());
        set_cycle(&mut root);
        let mut child = Work::new(
            format!("task:liveness-existing-child:{status:?}"),
            "An existing canonical action".into(),
            "Complete the existing task-scoped action.".into(),
            "This action already owns the next step.".into(),
        );
        child.parent_uid = Some(root.uid.clone());
        child.task_intent_uid = root.task_intent_uid.clone();
        child.task_intent_cycle_uid = root.task_intent_cycle_uid.clone();
        child.status = status;
        if status == WorkStatus::Todo {
            child.kind = crate::core::planning_work::WorkKind::PlanConfirmation;
        }
        project.planning_work = vec![root, child];

        reconcile(project);

        assert!(
            project
                .planning_work
                .iter()
                .all(|work| !work.key.starts_with("task:manager-liveness:")),
            "existing {status:?} child action must not get a duplicate liveness card"
        );
    }
}
