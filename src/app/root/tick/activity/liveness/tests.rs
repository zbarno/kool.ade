use super::*;

#[path = "tests/existing_child_action.rs"]
mod existing_child_action;
#[path = "tests/filesystem_failure.rs"]
mod filesystem_failure;
#[path = "tests/graph_matrix.rs"]
mod graph_matrix;
#[path = "tests/persist_failure.rs"]
mod persist_failure;
#[path = "tests/repeated_ticks.rs"]
mod repeated_ticks;
use crate::core::planning_work::Work;

struct ImmediateRecoveryFailure;

fn set_cycle(root: &mut Work) {
    let cycle_uid = root.uid.clone();
    root.task_intent_cycles = vec![crate::core::planning_work::TaskIntentCycleV1 {
        uid: cycle_uid.clone(),
        sequence: 1,
        goal: root
            .original_goal
            .clone()
            .unwrap_or_else(|| root.request.clone()),
        work_uid: root.uid.clone(),
        started_at_ms: None,
    }];
    root.task_intent_cycle_uid = Some(cycle_uid);
}

impl crate::harness::AiHarness for ImmediateRecoveryFailure {
    fn label(&self) -> String {
        "offline liveness test harness".into()
    }

    fn check_available(&self) -> Result<String, crate::AppError> {
        Ok("ready".into())
    }

    fn execute(
        &self,
        _: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::AppError> {
        Err(crate::AppError::Other("synthetic interruption".into()))
    }
}

#[test]
fn a_stranded_current_cycle_gets_one_persisted_action_task() {
    let mut app = crate::app::root::board_tests::fixture();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let mut root = Work::new(
        "task:liveness-root".into(),
        "A planning goal".into(),
        "Keep the approved user goal.".into(),
        "Planning started".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    set_cycle(&mut root);
    let intent_uid = root.uid.clone();
    let cycle_uid = root.task_intent_cycle_uid.clone().unwrap();
    project.planning_work = vec![root];

    reconcile(project);
    let first = project
        .planning_work
        .iter()
        .find(|work| work.key.starts_with("task:manager-liveness:"))
        .unwrap();
    assert_eq!(first.parent_uid.as_deref(), Some(intent_uid.as_str()));
    assert_eq!(first.task_intent_uid.as_deref(), Some(intent_uid.as_str()));
    assert_eq!(
        first.task_intent_cycle_uid.as_deref(),
        Some(cycle_uid.as_str())
    );
    assert_eq!(first.status, WorkStatus::Todo);
    let first_uid = first.uid.clone();
    assert!(
        project
            .activity
            .pending
            .iter()
            .any(|event| event.starts_with("A durable TaskIntent recovery task was created"))
    );

    reconcile(project);
    assert_eq!(
        project
            .planning_work
            .iter()
            .filter(|work| work.key.starts_with("task:manager-liveness:"))
            .map(|work| work.uid.as_str())
            .collect::<Vec<_>>(),
        vec![first_uid.as_str()]
    );
    assert!(
        project
            .planning_work
            .iter()
            .any(|work| work.uid == first_uid)
    );
}

#[test]
fn a_live_turn_and_open_task_scoped_decision_prevent_duplicate_recovery_tasks() {
    let mut app = crate::app::root::board_tests::fixture();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let mut root = Work::new(
        "task:liveness-live".into(),
        "A live planning goal".into(),
        "Keep the original goal.".into(),
        "Planning started".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    set_cycle(&mut root);
    let intent_uid = root.uid.clone();
    let cycle_uid = root.task_intent_cycle_uid.clone().unwrap();
    let key = root.key.clone();
    project.planning_work = vec![root];
    project.active_planning_work = Some(key);
    reconcile(project);
    assert!(
        project
            .planning_work
            .iter()
            .all(|work| !work.key.starts_with("task:manager-liveness:"))
    );

    project.active_planning_work = None;
    let mut decision = crate::domain::item::OpenItem::new(
        "Q-001".into(),
        crate::domain::item::Priority::Blocking,
        crate::domain::item::ItemKind::Question,
        "Product".into(),
        "Choose the supported behavior".into(),
        "The answer is required to proceed.".into(),
    );
    decision.task_intent_uid = Some(intent_uid);
    decision.task_intent_cycle_uid = Some(cycle_uid);
    project.state.items.push(decision);
    reconcile(project);
    assert!(
        project
            .planning_work
            .iter()
            .all(|work| !work.key.starts_with("task:manager-liveness:"))
    );
}

#[test]
fn stranded_needs_attention_roots_and_unowned_children_get_a_recovery_task() {
    let mut app = crate::app::root::board_tests::fixture();
    let crate::app::root::Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let mut root = Work::new(
        "task:liveness-orphan".into(),
        "A stalled goal".into(),
        "Keep the approved scope.".into(),
        "Waiting for progress".into(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    set_cycle(&mut root);
    root.status = WorkStatus::NeedsAttention;
    let mut child = Work::new(
        "task:liveness-orphan-child".into(),
        "An abandoned child operation".into(),
        "Resume this operation if still authorized.".into(),
        "Worker disappeared".into(),
    );
    child.parent_uid = Some(root.uid.clone());
    child.task_intent_uid = root.task_intent_uid.clone();
    child.task_intent_cycle_uid = root.task_intent_cycle_uid.clone();
    child.status = WorkStatus::InProgress;
    project.planning_work = vec![root, child];

    reconcile(project);

    let recovery = project
        .planning_work
        .iter()
        .find(|work| work.key.starts_with("task:manager-liveness:"))
        .expect("stalled root receives an executable recovery task");
    assert_eq!(recovery.status, WorkStatus::Todo);
}

/// Production boundary: persisted liveness findings are dispatched by the app's ordinary scoped
/// Planner turn and recovered after app restart. A reducer test cannot prove durable work claims,
/// task-scoped dispatch, or that restart selects the same recovery task. This uses local
/// private stores and a synthetic failing harness; no Docker, SSH, provider, or network. Setup is
/// one headless app fixture with two store reloads.
#[path = "tests/restart_bounded.rs"]
mod restart_bounded;
