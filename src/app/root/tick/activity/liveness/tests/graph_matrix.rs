use super::*;

/// Production boundary: one app liveness sweep classifies several persisted
/// TaskIntent cycles with different owner/action states and converges only the
/// stranded cycle to one durable recovery task. Separate reducer tests cannot
/// prove multi-cycle filtering and PlanningStore idempotency together. Uses
/// local private stores only, no Docker/network; moderate one-fixture setup.
#[test]
fn mixed_taskintent_graphs_converge_only_the_unowned_stranded_cycle() {
    let mut app = crate::app::root::board_tests::fixture();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let mut stranded = Work::new(
        "task:matrix-stranded".into(),
        "Resume the approved goal".into(),
        "Continue the exact authorized operation.".into(),
        "The previous worker ended without an outcome.".into(),
    );
    stranded.task_intent_uid = Some(stranded.uid.clone());
    set_cycle(&mut stranded);
    let stranded_uid = stranded.uid.clone();

    let mut awaiting_decision = Work::new(
        "task:matrix-decision".into(),
        "Wait for the required choice".into(),
        "Keep the unresolved decision task scoped.".into(),
        "A specific user decision is already open.".into(),
    );
    awaiting_decision.task_intent_uid = Some(awaiting_decision.uid.clone());
    set_cycle(&mut awaiting_decision);
    let mut decision = crate::domain::item::OpenItem::new(
        "Q-581-MATRIX".into(),
        crate::domain::item::Priority::Blocking,
        crate::domain::item::ItemKind::Question,
        "Product".into(),
        "Choose the supported behavior".into(),
        "The answer is required before work continues.".into(),
    );
    decision.task_intent_uid = Some(awaiting_decision.uid.clone());
    decision.task_intent_cycle_uid = awaiting_decision.task_intent_cycle_uid.clone();

    let mut action_root = Work::new(
        "task:matrix-action".into(),
        "Continue the existing action".into(),
        "Keep one canonical action for this cycle.".into(),
        "A review action already owns the next step.".into(),
    );
    action_root.task_intent_uid = Some(action_root.uid.clone());
    set_cycle(&mut action_root);
    let mut action = Work::new(
        "task:matrix-review-action".into(),
        "Complete the existing plan review".into(),
        "Continue the canonical review action.".into(),
        "The review remains actionable.".into(),
    );
    action.parent_uid = Some(action_root.uid.clone());
    action.task_intent_uid = action_root.task_intent_uid.clone();
    action.task_intent_cycle_uid = action_root.task_intent_cycle_uid.clone();
    action.status = WorkStatus::InReview;

    let mut active = Work::new(
        "task:matrix-active".into(),
        "Continue active planning".into(),
        "Keep the existing scoped Planner turn.".into(),
        "A Planner turn still owns this cycle.".into(),
    );
    active.task_intent_uid = Some(active.uid.clone());
    set_cycle(&mut active);
    let active_key = active.key.clone();

    let mut cancelled = Work::new(
        "task:matrix-cancelled".into(),
        "Keep the cancelled cycle closed".into(),
        "Do not re-open explicitly cancelled work.".into(),
        "The operator cancelled this cycle.".into(),
    );
    cancelled.task_intent_uid = Some(cancelled.uid.clone());
    set_cycle(&mut cancelled);
    let cancelled_id = crate::persistence::cancelled_work::planning_id(&cancelled.uid);
    let non_stranded_uids = [
        awaiting_decision.uid.clone(),
        action_root.uid.clone(),
        active.uid.clone(),
        cancelled.uid.clone(),
    ];

    project.planning_work = vec![
        stranded,
        awaiting_decision,
        action_root,
        action,
        active,
        cancelled,
    ];
    project.state.items.push(decision);
    project.active_planning_work = Some(active_key);
    project.cancelled_work.insert(cancelled_id);
    project
        .save_planning_work()
        .expect("seed the mixed graph in the private store");

    reconcile(project);
    let recovery = project
        .planning_work
        .iter()
        .find(|work| work.key.starts_with("task:manager-liveness:"))
        .unwrap_or_else(|| {
            panic!(
                "only the stranded cycle needs recovery; pending={:?}; work={:?}",
                project.activity.pending,
                project
                    .planning_work
                    .iter()
                    .map(|work| (
                        &work.key,
                        work.status,
                        &work.parent_uid,
                        &work.task_intent_uid
                    ))
                    .collect::<Vec<_>>()
            )
        });
    let recovery_uid = recovery.uid.clone();
    assert_eq!(recovery.parent_uid.as_deref(), Some(stranded_uid.as_str()));
    assert_eq!(recovery.status, WorkStatus::Todo);

    for _ in 0..3 {
        reconcile(project);
    }
    let persisted = crate::core::planning_work::load(&project.state.planning_store)
        .expect("reload converged mixed graph");
    let recoveries = persisted
        .iter()
        .filter(|work| work.key.starts_with("task:manager-liveness:"))
        .collect::<Vec<_>>();
    assert_eq!(recoveries.len(), 1);
    assert_eq!(recoveries[0].uid, recovery_uid);
    assert_eq!(recoveries[0].status, WorkStatus::Todo);
    for uid in &non_stranded_uids {
        assert!(
            persisted
                .iter()
                .filter(|work| work.key.starts_with("task:manager-liveness:"))
                .all(|work| work.parent_uid.as_deref() != Some(uid.as_str())),
            "non-stranded cycle {uid} must not receive a duplicate liveness action"
        );
    }
}
