use super::*;

/// Production boundary: durable investigation ownership -> restored exact
/// feature context -> one remaining dispatch claim. Pure reference validators
/// cannot prove persisted budgets or scheduler admission. Temporary local state
/// only, no provider/Docker/network; low setup cost.
#[test]
fn restored_feature_rearms_only_the_remaining_investigation_claim() {
    use crate::app::manager::operational_recovery as manager;
    use crate::core::planning_work::{WorkStatus, operational_recovery as recovery};
    let mut app = crate::app::root::board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        unreachable!()
    };
    let feature = crate::test_support::current_change_fixture(
        "# F1: Original feature\n\n## Intent\nKeep the original task.\n\n## Current Behavior\nContext was hidden.\n\n## Desired Behavior\nResume original investigation.\n\n## Scope\nRecovery only.\n\n## Affected Product Areas\nPlanning.\n\n## Requirements\nKeep identity.\n\n## Decisions and Assumptions\nNo new decisions.\n\n## Acceptance Criteria\nBounded retry succeeds.\n",
        crate::domain::ChangeStatus::Draft).unwrap();
    let changes = crate::core::apply::private_document_draft_changes(
        &project.state.planning_store,
        &[("feature:F1".into(), feature)],
    )
    .unwrap();
    project
        .state
        .planning_store
        .transaction(&changes, Some(&project.state.baseline_planning_revision))
        .unwrap();
    project.state = crate::core::state::PlannerState::load_with_store(
        &project.state.repo_root,
        &project.state.planning_store,
    )
    .unwrap();
    let (id, body) = project.state.active_features.first().unwrap().clone();
    let feature_uid = crate::domain::ArtifactIdentity::from_markdown(&body)
        .unwrap()
        .unwrap()
        .uid;
    let mut root = Work::new(
        "task:recovered-request".into(),
        "Restore investigation context".into(),
        "Keep the original request".into(),
        String::new(),
    );
    root.task_intent_uid = Some(root.uid.clone());
    root.feature_id = Some(id.clone());
    root.feature_uid = Some(feature_uid.clone());
    let root_uid = root.uid.clone();
    project.planning_work.push(root);
    project.save_planning_work().unwrap();
    let root = project
        .planning_work
        .iter()
        .find(|work| work.uid == root_uid)
        .unwrap()
        .clone();
    project.queue.auto_plan = true;
    let mut item = crate::domain::OpenItem::new(
        "CLR-990".into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Question,
        "Architecture".into(),
        "Inspect the original feature".into(),
        String::new(),
    );
    item.authority = crate::domain::Authority::Agent;
    item.feature_id = Some(id.clone());
    item.feature_uid = Some(feature_uid.clone());
    item.task_intent_uid = Some(root.uid.clone());
    item.task_intent_cycle_uid = root.task_intent_cycle_uid.clone();
    let source_uid = item.uid.clone().unwrap();
    let key = item.id.clone();
    let mut source = root.clone();
    source.uid = source_uid.clone();
    source.key = item.conversation_key().into();
    source.feature_id = Some(id.clone());
    source.feature_uid = Some(feature_uid);
    project.state.items.push(item);
    let (changes, checks) = crate::artifacts::items_io::record_changes(
        &project.state.planning_store,
        &project.state.items,
        &project.state.resolved_items,
    )
    .unwrap();
    project
        .state
        .planning_store
        .transaction_with_record_revisions(&changes, &checks)
        .unwrap();
    let error =
        format!("Repair attempt: invalid feature reference {id}. Return a corrected envelope.");
    let mut triage = recovery::task(
        &source,
        &format!("investigation:{source_uid}"),
        recovery::FailureKind::Execution,
        error.clone(),
    );
    triage.status = WorkStatus::NeedsAttention;
    let r = triage.operational_recovery.as_mut().unwrap();
    r.autonomous_investigation = true;
    r.investigation_claims = 1;
    r.attempts = 1;
    r.repair_error = Some(error.clone());
    let triage_uid = triage.uid.clone();
    project.planning_work.push(triage);
    project.save_planning_work().unwrap();
    project.state.active_features.clear();
    manager::advance(project);
    assert!(manager::investigation_blocked(project, Some(&source_uid)));
    project.state = crate::core::state::PlannerState::load_with_store(
        &project.state.repo_root,
        &project.state.planning_store,
    )
    .unwrap();
    manager::advance(project);
    let triage = project
        .planning_work
        .iter_mut()
        .find(|work| work.uid == triage_uid)
        .unwrap();
    let r = triage.operational_recovery.as_mut().unwrap();
    assert!(r.repair_error.is_none());
    assert_eq!(r.investigation_claims, 1);
    assert!(r.next_attempt_ms > chrono::Utc::now().timestamp_millis());
    r.next_attempt_ms = 0;
    project.save_planning_work().unwrap();
    assert!(manager::claim_investigation(project, &key));
    manager::enqueue_investigation(project, &key, &source_uid, &error, Some(2));
    manager::advance(project);
    let r = project
        .planning_work
        .iter()
        .find(|work| work.uid == triage_uid)
        .unwrap()
        .operational_recovery
        .as_ref()
        .unwrap();
    assert_eq!(r.investigation_claims, 2);
    assert!(r.repair_error.is_some());
    assert!(manager::investigation_blocked(project, Some(&source_uid)));
}
