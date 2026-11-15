use super::*;

/// Production boundary: a stale PlanningStore revision must not lose the
/// liveness finding or leave an in-memory-only recovery card. A reducer test
/// cannot prove optimistic persistence failure and next-sweep convergence.
/// Dependencies: private local stores only. Setup cost: one app fixture and a
/// conflicting local store write.
#[test]
fn liveness_store_conflict_is_reported_then_converges_after_resync() {
    let mut app = crate::app::root::board_tests::fixture();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let mut root = Work::new(
        "task:liveness-conflict".into(),
        "A planning goal".into(),
        "Keep the approved goal.".into(),
        "Planning started".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    set_cycle(&mut root);
    project.planning_work = vec![root];

    project
        .state
        .planning_store
        .atomic_write("state/liveness-conflict-marker", b"advance store revision")
        .unwrap();
    reconcile(project);
    assert_eq!(
        project.planning_work.len(),
        1,
        "failed save rolls back the card"
    );
    assert!(
        project
            .activity
            .pending
            .iter()
            .any(|event| { event.starts_with("TaskIntent liveness findings could not be saved") })
    );

    project.state.resync().unwrap();
    reconcile(project);
    let recovery = project
        .planning_work
        .iter()
        .find(|work| work.key.starts_with("task:manager-liveness:"))
        .expect("the next sweep persists the recovery after refreshing its revision");
    assert_eq!(recovery.status, WorkStatus::Todo);
    let recovery_uid = recovery.uid.clone();

    reconcile(project);
    assert_eq!(
        project
            .planning_work
            .iter()
            .filter(|work| work.key.starts_with("task:manager-liveness:"))
            .map(|work| work.uid.as_str())
            .collect::<Vec<_>>(),
        [recovery_uid.as_str()],
        "repeated sweeps keep the same durable recovery identity"
    );
}
