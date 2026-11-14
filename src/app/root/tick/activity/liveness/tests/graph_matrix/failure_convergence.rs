use super::*;

/// Production boundary: multiple stranded cycles must converge after both an
/// unreadable lifecycle collection and a stale PlanningStore revision. A unit
/// test cannot prove app-store rollback, resync, filtering, and idempotency
/// together. Uses local private stores and one app fixture; no Docker/network.
#[test]
fn mixed_graph_recovers_canonically_after_lifecycle_and_store_failures() {
    let mut app = crate::app::root::board_tests::fixture();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let mut todo = root("task:matrix-failure-todo", WorkStatus::Todo);
    let todo_uid = todo.uid.clone();
    let mut attention = root("task:matrix-failure-attention", WorkStatus::NeedsAttention);
    let attention_uid = attention.uid.clone();
    let mut interrupted = root("task:matrix-failure-interrupted", WorkStatus::InProgress);
    let interrupted_uid = interrupted.uid.clone();
    let mut active = root("task:matrix-failure-active", WorkStatus::InProgress);
    let active_uid = active.uid.clone();
    let active_key = active.key.clone();
    let mut cancelled = root("task:matrix-failure-cancelled", WorkStatus::Todo);
    let cancelled_uid = cancelled.uid.clone();
    for cycle in [
        &mut todo,
        &mut attention,
        &mut interrupted,
        &mut active,
        &mut cancelled,
    ] {
        cycle.task_intent_uid = Some(cycle.uid.clone());
        set_cycle(cycle);
    }
    let cancelled_id = crate::persistence::cancelled_work::planning_id(&cancelled.uid);
    project.planning_work = vec![todo, attention, interrupted, active, cancelled];
    project.task_documents.clear();
    project.active_planning_work = Some(active_key);
    project.cancelled_work.insert(cancelled_id);
    project.save_planning_work().unwrap();

    let mut external = project.planning_work.clone();
    let mut concurrent = Work::new(
        "planning:matrix-revision-probe".into(),
        "Preserve unrelated planning state".into(),
        "Keep the unrelated PlanningStore update.".into(),
        "Written by a separate PlanningStore owner.".into(),
    );
    concurrent.kind = crate::core::planning_work::WorkKind::ProjectPlanning;
    external.push(concurrent);
    crate::core::planning_work::save(&project.state.planning_store, &external).unwrap();

    let lifecycle_collection = project
        .state
        .planning_store
        .root
        .join("state/lifecycle_features");
    std::fs::create_dir_all(lifecycle_collection.parent().unwrap()).unwrap();
    std::fs::write(&lifecycle_collection, b"not a directory").unwrap();
    reconcile(project);
    assert!(
        project
            .planning_work
            .iter()
            .all(|work| { !work.key.starts_with("task:manager-liveness:") })
    );
    assert!(
        project
            .activity
            .pending
            .iter()
            .any(|event| { event.starts_with("TaskIntent liveness findings could not be saved") })
    );

    project.state.resync().unwrap();
    assert!(lifecycle_collection.is_file());
    assert_eq!(project.planning_work.len(), 5);
    reconcile(project);
    let first = recoveries(project);
    assert_eq!(first.len(), 3, "only the stranded cycles need blockers");
    assert!(first.contains_key(&todo_uid));
    assert!(first.contains_key(&attention_uid));
    assert!(first.contains_key(&interrupted_uid));
    assert!(!first.contains_key(&active_uid));
    assert!(!first.contains_key(&cancelled_uid));
    assert!(
        project
            .planning_work
            .iter()
            .any(|work| work.key == "planning:matrix-revision-probe")
    );

    for _ in 0..3 {
        reconcile(project);
    }
    let persisted = crate::core::planning_work::load(&project.state.planning_store).unwrap();
    let final_recoveries = persisted
        .iter()
        .filter(|work| work.key.starts_with("task:manager-liveness:"))
        .map(|work| (work.parent_uid.clone().unwrap(), work.uid.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert!(
        final_recoveries == first,
        "stable recovery parent and UID mapping must survive resync and repeated sweeps"
    );
    assert_eq!(final_recoveries.len(), 3);
    std::fs::remove_file(lifecycle_collection).unwrap();
}

fn root(key: &str, status: WorkStatus) -> Work {
    let mut work = Work::new(
        key.into(),
        "Resume this approved goal".into(),
        "Continue the exact authorized operation.".into(),
        "Prior work stopped without a durable outcome.".into(),
    );
    work.status = status;
    work
}

fn recoveries(
    project: &crate::app::session::Project,
) -> std::collections::BTreeMap<String, String> {
    project
        .planning_work
        .iter()
        .filter(|work| work.key.starts_with("task:manager-liveness:"))
        .map(|work| (work.parent_uid.clone().unwrap(), work.uid.clone()))
        .collect()
}
