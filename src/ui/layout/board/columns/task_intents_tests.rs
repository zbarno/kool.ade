use super::*;

fn intent_fixture() -> (
    crate::ui::planning_board::ViewModel,
    crate::core::planning_work::Work,
    crate::core::planning_work::Work,
    crate::artifacts::task_docs::TaskDocument,
) {
    let mut root = crate::core::planning_work::Work::new(
        "task:intent-root".into(),
        "Feature: durable intent".into(),
        "Keep the user's goal through delivery".into(),
        "Planning".into(),
    );
    root.kind = crate::core::planning_work::WorkKind::Feature;
    root.task_intent_uid = Some(root.uid.clone());
    let mut child = crate::core::planning_work::Work::new(
        "task:planning-child".into(),
        "Question: recovery behavior".into(),
        "Clarify restart behavior".into(),
        "Needs an answer".into(),
    );
    child.kind = crate::core::planning_work::WorkKind::Question;
    child.parent_uid = Some(root.uid.clone());
    child.task_intent_uid = Some(root.uid.clone());

    let batch_uid = uuid::Uuid::new_v4().to_string();
    let mut identity = crate::domain::ArtifactIdentity::new("TASK-001", "Implement recovery");
    identity.parent_uid = Some(batch_uid.clone());
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", vec![])
        .unwrap()
        .with_task_intent_uid(Some(&root.uid))
        .unwrap();
    let task = crate::artifacts::task_docs::TaskDocument {
        path: "planning/tasks/fixture/001-recovery.md".into(),
        title: "Implement recovery".into(),
        text: "# Implement recovery".into(),
        identity: Some(identity),
        metadata: Some(metadata),
        task_state: None,
        metadata_error: None,
    };
    let board = crate::ui::planning_board::ViewModel {
        planning_work: vec![root.clone(), child.clone()],
        task_documents: vec![task.clone()],
        ..Default::default()
    };
    (board, root, child, task)
}

#[test]
fn work_and_executable_tasks_group_under_one_root_across_different_lanes() {
    let (board, root, child, task) = intent_fixture();
    let planning_lane = collect_groups(&board, &[&child], &[], &[]);
    let execution_lane = collect_groups(&board, &[], &[&task], &[]);

    assert_eq!(planning_lane.len(), 1);
    assert_eq!(execution_lane.len(), 1);
    assert_eq!(planning_lane[&root.uid].root.unwrap().uid, root.uid);
    assert_eq!(planning_lane[&root.uid].planning.len(), 1);
    assert!(planning_lane[&root.uid].tasks.is_empty());
    assert_eq!(execution_lane[&root.uid].root.unwrap().uid, root.uid);
    assert!(execution_lane[&root.uid].planning.is_empty());
    assert_eq!(execution_lane[&root.uid].tasks.len(), 1);
}

#[test]
fn children_with_missing_or_ambiguous_roots_are_not_grouped() {
    let (mut board, _, child, task) = intent_fixture();
    board.planning_work.clear();
    assert!(collect_groups(&board, &[&child], &[&task], &[]).is_empty());

    let (mut board, root, child, task) = intent_fixture();
    board.planning_work.push(root);
    assert!(collect_groups(&board, &[&child], &[&task], &[]).is_empty());
}

#[test]
fn root_lane_uses_group_header_once_children_exist_and_keeps_empty_root_visible() {
    let (board, root, child, _) = intent_fixture();
    let mut groups = collect_groups(&board, &[&root], &[], &[]);
    add_root_lane_groups(&board, &[&root], &mut groups, &[]);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[&root.uid].root.unwrap().uid, root.uid);
    assert_eq!(groups[&root.uid].planning.len(), 1);
    assert_eq!(child_count(&board, &root.uid), 2);

    let empty_board = crate::ui::planning_board::ViewModel {
        planning_work: vec![root.clone()],
        ..Default::default()
    };
    let mut empty_groups = collect_groups(&empty_board, &[&root], &[], &[]);
    add_root_lane_groups(&empty_board, &[&root], &mut empty_groups, &[]);
    assert_eq!(empty_groups[&root.uid].planning.len(), 1);
    assert_eq!(child_count(&empty_board, &root.uid), 0);

    let child_lane = collect_groups(&board, &[&child], &[], &[]);
    assert_eq!(child_lane[&root.uid].planning.len(), 1);
}

#[test]
fn scoped_decision_items_join_the_root_group() {
    let (mut board, root, _, _) = intent_fixture();
    let mut item = crate::domain::OpenItem::new(
        "CLR-001".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "Product".into(),
        "Should the migration remain reversible?".into(),
        "The approved plan leaves this decision open.".into(),
    );
    item.task_intent_uid = Some(root.uid.clone());
    board.planning_items.push(item.clone());
    let groups = collect_groups(&board, &[], &[], &[&item]);
    assert_eq!(groups[&root.uid].items.len(), 1);
    assert_eq!(child_count(&board, &root.uid), 3);
}

#[test]
fn prior_cycle_decisions_do_not_block_the_current_intent_lifecycle() {
    let (mut board, root, _, _) = intent_fixture();
    let root_uid = root.uid.clone();
    let board_root = board
        .planning_work
        .iter_mut()
        .find(|work| work.uid == root_uid)
        .unwrap();
    board_root.kind = crate::core::planning_work::WorkKind::Question;
    board_root.status = crate::core::planning_work::WorkStatus::Done;
    let cycle_uid = crate::core::planning_work::start_follow_up_cycle(
        &mut board.planning_work,
        &root_uid,
        "Resolve a new follow-up decision",
    )
    .unwrap();
    let cycle_work = board
        .planning_work
        .iter_mut()
        .find(|work| {
            work.uid != root_uid
                && work.task_intent_cycle_uid.as_deref() == Some(cycle_uid.as_str())
        })
        .unwrap();
    cycle_work.status = crate::core::planning_work::WorkStatus::Done;
    let mut old_decision = crate::domain::OpenItem::new(
        "CLR-OLD".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "Product".into(),
        "Old cycle decision remains open".into(),
        "Prior work context".into(),
    );
    old_decision.task_intent_uid = Some(root_uid.clone());
    old_decision.task_intent_cycle_uid = Some(root_uid.clone());
    board.planning_items.push(old_decision);

    let root = board
        .planning_work
        .iter()
        .find(|work| work.uid == root_uid)
        .unwrap();
    assert_eq!(
        lifecycle(root, &board),
        crate::core::task_intent::TaskIntentLifecycle::Complete
    );
}

#[test]
fn archive_is_disabled_until_owned_work_reaches_a_terminal_outcome() {
    let (mut board, mut root, mut child, _) = intent_fixture();
    root.kind = crate::core::planning_work::WorkKind::Question;
    root.status = crate::core::planning_work::WorkStatus::Done;
    child.task_intent_cycle_uid = Some(root.uid.clone());
    child.status = crate::core::planning_work::WorkStatus::InProgress;
    board.planning_work = vec![root.clone(), child.clone()];

    assert!(!board.can_archive_task_intent(&root));
    child.status = crate::core::planning_work::WorkStatus::Done;
    board.planning_work = vec![root.clone(), child];
    assert!(board.can_archive_task_intent(&root));

    let mut unresolved = crate::domain::OpenItem::new(
        "CLR-404".into(),
        crate::domain::Priority::Blocking,
        crate::domain::ItemKind::Question,
        "Product".into(),
        "A decision remains open".into(),
        "It blocks the request.".into(),
    );
    unresolved.task_intent_uid = Some(root.uid.clone());
    board.planning_items.push(unresolved);
    assert!(!board.can_archive_task_intent(&root));
}
