use super::*;

/// Production boundary: repeated app liveness sweeps classify multiple
/// persisted TaskIntent cycles and create one durable recovery for each of
/// three stranded statuses while preserving owned cycles. Reducer tests cannot
/// prove multi-cycle filtering and PlanningStore idempotency together. Local
/// private stores only, no Docker/network; setup is one app fixture and reload.
#[test]
fn mixed_taskintent_graphs_converge_stranded_cycles_without_recovering_owned_cycles() {
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
    stranded.status = WorkStatus::Todo;
    let stranded_uid = stranded.uid.clone();

    let mut needs_attention = Work::new(
        "task:matrix-stranded-attention".into(),
        "Recover the interrupted attention cycle".into(),
        "Continue the exact authorized operation.".into(),
        "The recovery owner stopped before creating its next action.".into(),
    );
    needs_attention.task_intent_uid = Some(needs_attention.uid.clone());
    set_cycle(&mut needs_attention);
    needs_attention.status = WorkStatus::NeedsAttention;
    let needs_attention_uid = needs_attention.uid.clone();

    let mut interrupted = Work::new(
        "task:matrix-stranded-interrupted".into(),
        "Recover the abandoned Planner cycle".into(),
        "Continue the exact authorized operation.".into(),
        "The Planner owner disappeared before recording an outcome.".into(),
    );
    interrupted.task_intent_uid = Some(interrupted.uid.clone());
    set_cycle(&mut interrupted);
    interrupted.status = WorkStatus::InProgress;
    let interrupted_uid = interrupted.uid.clone();

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
        needs_attention,
        interrupted,
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
    let initial_recoveries = project
        .planning_work
        .iter()
        .filter(|work| work.key.starts_with("task:manager-liveness:"))
        .map(|work| {
            (
                work.parent_uid
                    .clone()
                    .expect("recovery has a parent cycle"),
                work.uid.clone(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        initial_recoveries.len(),
        3,
        "each stranded cycle gets one action"
    );
    let recovery_uid = initial_recoveries
        .get(&stranded_uid)
        .unwrap_or_else(|| {
            panic!(
                "the original stranded cycle needs recovery; pending={:?}; work={:?}",
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
        })
        .clone();

    for _ in 0..3 {
        reconcile(project);
    }
    let persisted = crate::core::planning_work::load(&project.state.planning_store)
        .expect("reload converged mixed graph");
    let recoveries = persisted
        .iter()
        .filter(|work| work.key.starts_with("task:manager-liveness:"))
        .collect::<Vec<_>>();
    assert_eq!(recoveries.len(), 3);
    for (parent_uid, expected_uid) in [
        (stranded_uid.as_str(), recovery_uid.as_str()),
        (
            needs_attention_uid.as_str(),
            initial_recoveries
                .get(&needs_attention_uid)
                .unwrap()
                .as_str(),
        ),
        (
            interrupted_uid.as_str(),
            initial_recoveries.get(&interrupted_uid).unwrap().as_str(),
        ),
    ] {
        let recovery = recoveries
            .iter()
            .find(|work| work.parent_uid.as_deref() == Some(parent_uid))
            .expect("each stranded cycle keeps its recovery owner");
        assert_eq!(recovery.uid, expected_uid);
        assert_eq!(recovery.status, WorkStatus::Todo);
    }
    for uid in &non_stranded_uids {
        assert!(
            persisted
                .iter()
                .filter(|work| work.key.starts_with("task:manager-liveness:"))
                .all(|work| work.parent_uid.as_deref() != Some(uid.as_str())),
            "non-stranded cycle must not receive a duplicate liveness action"
        );
    }
}
