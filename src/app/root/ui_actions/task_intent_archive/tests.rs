use super::*;

#[test]
fn archive_refuses_a_root_with_active_owned_work() {
    let mut app = crate::app::root::board_tests::trace_navigation_test_app();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected");
    };
    let mut root = crate::core::planning_work::Work::new(
        String::new(),
        "Question: preserve history".into(),
        "Keep the answer auditable".into(),
        "Complete".into(),
    );
    root.kind = crate::core::planning_work::WorkKind::Question;
    root.status = crate::core::planning_work::WorkStatus::Done;
    root.key = format!("task:{}", root.uid);
    root.task_intent_uid = Some(root.uid.clone());
    let root_uid = root.uid.clone();
    let mut child = crate::core::planning_work::Work::new(
        String::new(),
        "Follow-up answer".into(),
        "A child is still active".into(),
        "In progress".into(),
    );
    child.kind = crate::core::planning_work::WorkKind::Question;
    child.status = crate::core::planning_work::WorkStatus::InProgress;
    child.parent_uid = Some(root_uid.clone());
    child.task_intent_uid = Some(root_uid.clone());
    project.planning_work = vec![root, child];
    project.save_planning_work().unwrap();
    let key = project.planning_work[0].key.clone();

    app.archive_task(&key);

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture must remain connected");
    };
    assert_eq!(project.planning_work[0].task_intent_archived_at_ms, None);
    assert!(!project.archived_tasks.contains(&key));
}

#[test]
fn cancelled_root_cannot_archive_while_owned_child_implementation_is_active() {
    let mut app = crate::app::root::board_tests::trace_navigation_test_app();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected");
    };
    let mut root = crate::core::planning_work::Work::new(
        String::new(),
        "Cancelled intent".into(),
        "The running child must still block archive".into(),
        "Cancelled".into(),
    );
    root.kind = crate::core::planning_work::WorkKind::Question;
    root.status = crate::core::planning_work::WorkStatus::Done;
    root.key = format!("task:{}", root.uid);
    root.task_intent_uid = Some(root.uid.clone());
    let root_uid = root.uid.clone();
    let mut child = crate::core::planning_work::Work::new(
        String::new(),
        "Active child".into(),
        "Still has an implementation worker".into(),
        "In progress".into(),
    );
    child.kind = crate::core::planning_work::WorkKind::Question;
    child.status = crate::core::planning_work::WorkStatus::InProgress;
    child.parent_uid = Some(root_uid.clone());
    child.task_intent_uid = Some(root_uid.clone());
    let child_key = child.key.clone();
    project.planning_work = vec![root, child];
    project
        .cancelled_work
        .insert(crate::persistence::cancelled_work::planning_id(&root_uid));
    project.active_implementations.insert(
        child_key,
        crate::core::implementation::Controller::idle_fixture(),
    );
    project.save_planning_work().unwrap();
    let root_key = project.planning_work[0].key.clone();

    app.archive_task(&root_key);

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture must remain connected");
    };
    assert_eq!(project.planning_work[0].task_intent_archived_at_ms, None);
    assert!(!project.archived_tasks.contains(&root_key));
}

#[test]
fn cancelled_root_cannot_archive_while_owned_child_waits_in_the_queue() {
    let mut app = crate::app::root::board_tests::trace_navigation_test_app();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected");
    };
    let mut root = crate::core::planning_work::Work::new(
        String::new(),
        "Cancelled intent".into(),
        "The queued child must still block archive".into(),
        "Cancelled".into(),
    );
    root.kind = crate::core::planning_work::WorkKind::Question;
    root.status = crate::core::planning_work::WorkStatus::Done;
    root.key = format!("task:{}", root.uid);
    root.task_intent_uid = Some(root.uid.clone());
    let root_uid = root.uid.clone();
    let mut child = crate::core::planning_work::Work::new(
        String::new(),
        "Queued child".into(),
        "Waiting for implementation capacity".into(),
        "In progress".into(),
    );
    child.kind = crate::core::planning_work::WorkKind::Question;
    child.status = crate::core::planning_work::WorkStatus::InProgress;
    child.parent_uid = Some(root_uid.clone());
    child.task_intent_uid = Some(root_uid.clone());
    project.planning_work = vec![root, child];
    project
        .cancelled_work
        .insert(crate::persistence::cancelled_work::planning_id(&root_uid));

    let mut identity = crate::domain::ArtifactIdentity::new("TASK-QUEUED", "Queued child");
    identity.parent_uid = Some(uuid::Uuid::new_v4().to_string());
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", Vec::new())
        .unwrap()
        .with_task_intent_uid(Some(&root_uid))
        .unwrap();
    let queued_path = "planning/tasks/queued-child.md";
    project
        .task_documents
        .push(crate::artifacts::task_docs::TaskDocument {
            path: queued_path.into(),
            title: "Queued child".into(),
            text: String::new(),
            identity: Some(identity),
            metadata: Some(metadata),
            task_state: None,
            metadata_error: None,
        });
    project
        .queue
        .waiting_for_capacity
        .insert(queued_path.into());
    project.save_planning_work().unwrap();
    let root_key = project.planning_work[0].key.clone();

    app.archive_task(&root_key);

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture must remain connected");
    };
    assert_eq!(project.planning_work[0].task_intent_archived_at_ms, None);
    assert!(!project.archived_tasks.contains(&root_key));
}

#[test]
fn failed_restore_keeps_task_intent_archived_after_a_stale_write() {
    let mut app = crate::app::root::board_tests::trace_navigation_test_app();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected");
    };
    let mut root = crate::core::planning_work::Work::new(
        String::new(),
        "Question: recover archive".into(),
        "Retain the archived state after failed restore".into(),
        "Complete".into(),
    );
    root.kind = crate::core::planning_work::WorkKind::Question;
    root.status = crate::core::planning_work::WorkStatus::Done;
    root.key = format!("task:{}", root.uid);
    root.task_intent_uid = Some(root.uid.clone());
    root.task_intent_archived_at_ms = Some(1_800_000_000_000);
    let root_uid = root.uid.clone();
    project.planning_work.push(root);
    project.save_planning_work().unwrap();
    let store = project.state.planning_store.clone();
    let revision = project.state.baseline_planning_revision.clone();
    let mut rival_work = project.planning_work.clone();
    rival_work
        .iter_mut()
        .find(|work| work.uid == root_uid)
        .unwrap()
        .detail = "Concurrent planning update".into();
    crate::core::planning_work::save_expected(&store, &rival_work, &revision).unwrap();

    app.restore_task_intent(&root_uid);

    let Screen::Connected(project) = &app.screen else {
        panic!("fixture must remain connected");
    };
    assert_eq!(
        project
            .planning_work
            .iter()
            .find(|work| work.uid == root_uid)
            .unwrap()
            .task_intent_archived_at_ms,
        Some(1_800_000_000_000)
    );
    let persisted = crate::core::planning_work::load(&store).unwrap();
    assert_eq!(
        persisted
            .iter()
            .find(|work| work.uid == root_uid)
            .unwrap()
            .task_intent_archived_at_ms,
        Some(1_800_000_000_000)
    );
}
