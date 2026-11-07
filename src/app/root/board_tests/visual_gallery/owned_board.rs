use super::*;

/// Production GPU painter boundary: intent containers and agent ownership remain
/// visible across board lanes. Unit projections cannot prove clipping/layout.
/// Synthetic local state and a GPU adapter (software is sufficient), no provider,
/// Docker or network. Opt-in render setup cost is a few seconds.
#[test]
#[ignore = "GPU board review; requires isolated KOOLADE_HOME and KOOLADE_UI_REVIEW_DIR"]
fn render_owned_work_board() {
    use crate::core::planning_work::{Work, WorkKind, WorkStatus, operational_recovery};
    let dir = std::path::PathBuf::from(std::env::var("KOOLADE_UI_REVIEW_DIR").unwrap());
    assert!(std::env::var_os("KOOLADE_HOME").is_some());
    std::fs::create_dir_all(&dir).unwrap();
    for size in [[1600, 900], [1280, 720]] {
        let mut gallery = Gallery::new(size, dir.clone());
        let Screen::Connected(project) = &mut gallery.app.screen else {
            unreachable!()
        };
        project.task_documents.clear();
        project.active_implementations.clear();
        project.implementation_states.clear();
        project.activity.tasks.clear();
        project.queue.current_ticket = None;
        project.queue.in_flight.clear();
        project.queue.waiting_for_capacity.clear();
        project.state.items.clear();
        project.state.resolved_items.clear();
        let mut root = Work::new(
            "task:request".into(),
            "Improve the specification reader".into(),
            "Make reading specifications comfortable".into(),
            "Agent recovery is blocked on a stale draft.".into(),
        );
        root.kind = WorkKind::Feature;
        root.task_intent_uid = Some(root.uid.clone());
        root.status = WorkStatus::NeedsAttention;
        let mut planned = Work::new(
            "task:planning".into(),
            "Prepare the specification change".into(),
            "Continue with the saved decisions".into(),
            "Inspecting current evidence".into(),
        );
        planned.task_intent_uid = Some(root.uid.clone());
        planned.parent_uid = Some(root.uid.clone());
        planned.status = WorkStatus::Todo;
        let mut recovery = operational_recovery::task(
            &planned,
            "saved-answer",
            operational_recovery::FailureKind::FeatureIdentity,
            "The draft is based on an older published revision.".into(),
        );
        recovery.status = WorkStatus::NeedsAttention;
        recovery.detail = "Agent recovery blocked: reconcile the saved draft with the current published revision. Your answer is preserved.".into();
        let mut complete = planned.clone();
        complete.uid = uuid::Uuid::new_v4().to_string();
        complete.key = format!("task:{}", complete.uid);
        complete.title = "Record the background preference".into();
        complete.status = WorkStatus::Done;
        project.archived_tasks.insert(complete.key.clone());
        project.planning_work = vec![root, planned, recovery, complete];
        gallery.save("owned-work-board");
    }
}
