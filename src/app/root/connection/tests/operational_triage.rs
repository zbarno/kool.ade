use super::*;

/// Production boundary: app monitoring -> durable linked recovery task -> shared
/// reply authorization -> guarded apply receipt. Unit validators cannot prove
/// persistence, deduplication or original-task resumption. Temporary local files;
/// no provider, Docker or network, low setup cost.
#[test]
fn operational_triage_owns_one_task_and_verifies_original_reply_application() {
    let mut app = crate::app::root::board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("connected fixture");
    };
    let stable_uid = uuid::Uuid::new_v4().to_string();
    let task_key = format!("task:{stable_uid}");
    let key = task_key.as_str();
    let mut work = Work::new(
        key.into(),
        "Original request".into(),
        "Keep the approved scope".into(),
        "Internal failure".into(),
    );
    work.uid = stable_uid;
    work.kind = WorkKind::Bug;
    let uid = work.uid.clone();
    project.planning_work.push(work);
    project.save_planning_work().unwrap();
    project.bind_task_conversation_identities();
    let slug = project.chat_slug.clone();
    project.task_chats.ensure_loaded(&slug);
    let user = crate::domain::ChatMessage::new(
        crate::domain::ChatRole::User,
        "Keep my chosen option",
        None,
    );
    let reply = user.id.clone();
    project
        .task_chats
        .append_user_reply_and_claim(&slug, key, user)
        .unwrap();
    let mut failed = crate::domain::ChatMessage::new(
        crate::domain::ChatRole::System,
        "Planning stopped before application",
        None,
    );
    crate::persistence::task_chats::TaskChats::mark_task_reply_outcome(&mut failed);
    project.task_chats.append(&slug, key, vec![failed]).unwrap();
    crate::app::manager::operational_recovery::advance(project);
    crate::app::manager::operational_recovery::advance(project);
    let recovery = project
        .planning_work
        .iter()
        .filter(|w| w.operational_recovery.is_some())
        .collect::<Vec<_>>();
    assert_eq!(recovery.len(), 1);
    assert_eq!(recovery[0].parent_uid.as_deref(), Some(uid.as_str()));
    assert_eq!(
        recovery[0].status,
        crate::core::planning_work::WorkStatus::Todo
    );
    crate::app::manager::operational_recovery::prepare_reply_dispatch(project, key, &reply)
        .unwrap();
    let before_worker = project.state.planning_store.revision().unwrap();
    let claim = project
        .task_chats
        .claim_pending_task_reply(&slug, key, &reply)
        .unwrap()
        .unwrap();
    crate::app::manager::operational_recovery::advance(project);
    assert_eq!(
        project.state.planning_store.revision().unwrap(),
        before_worker,
        "monitoring must not invalidate the active worker snapshot"
    );
    let lock = crate::persistence::task_chats::TaskChats::lock_latest_user_message(
        &slug,
        key,
        &reply,
        Some(&claim),
    )
    .unwrap()
    .unwrap();
    lock.mark_applied().unwrap();
    drop(lock);
    project.task_chats.refresh_now(&slug);
    crate::app::manager::operational_recovery::advance(project);
    assert_eq!(
        project
            .planning_work
            .iter()
            .find(|w| w.operational_recovery.is_some())
            .unwrap()
            .status,
        crate::core::planning_work::WorkStatus::Done
    );
}

/// Production boundary: investigation failure -> durable Manager task -> native
/// scheduler admission, without a user reply or provider. Local fixture storage,
/// low setup cost; validates restart-persistent retry budgets beyond unit policy.
#[test]
fn failed_agent_investigation_gets_one_owned_retry_and_a_persistent_blocker() {
    let mut app = crate::app::root::board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("connected fixture");
    };
    let key = "CLR-900";
    let mut intent = Work::new(
        String::new(),
        "Owning request".into(),
        "Inspect the synthetic behavior".into(),
        String::new(),
    );
    intent.key = format!("task:{}", intent.uid);
    let intent_uid = intent.uid.clone();
    project.planning_work.push(intent);
    project.save_planning_work().unwrap();
    let cycle_uid = project
        .planning_work
        .iter()
        .find(|work| work.uid == intent_uid)
        .unwrap()
        .task_intent_cycles
        .last()
        .unwrap()
        .uid
        .clone();
    let mut item = crate::domain::OpenItem::new(
        key.into(),
        crate::domain::Priority::Normal,
        crate::domain::ItemKind::Ambiguity,
        "Architecture".into(),
        "Inspect synthetic behavior".into(),
        "Repository evidence is needed".into(),
    );
    item.authority = crate::domain::Authority::Agent;
    item.conversation_id = Some("CLR-original-conversation".into());
    item.task_intent_uid = Some(intent_uid.clone());
    item.task_intent_cycle_uid = Some(cycle_uid.clone());
    let uid = item.uid.clone().unwrap();
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
    project.state = crate::core::state::PlannerState::load_with_store(
        &project.state.repo_root,
        &project.state.planning_store,
    )
    .unwrap();
    project.queue.auto_plan = true;
    project.investigation_attempted.insert(key.into());
    crate::app::manager::operational_recovery::enqueue_investigation(
        project,
        key,
        &uid,
        "Invalid response field type",
        None,
    );
    project.investigation_attempted.clear();
    assert!(
        crate::app::manager::operational_recovery::investigation_blocked(project, Some(&uid)),
        "restart must not bypass the durable cooldown"
    );
    let task = project
        .planning_work
        .iter_mut()
        .find(|w| w.operational_recovery.is_some())
        .unwrap();
    assert!(
        task.operational_recovery
            .as_ref()
            .unwrap()
            .autonomous_investigation
    );
    assert_eq!(task.task_intent_uid.as_deref(), Some(intent_uid.as_str()));
    assert_eq!(
        task.task_intent_cycle_uid.as_deref(),
        Some(cycle_uid.as_str())
    );
    task.operational_recovery.as_mut().unwrap().next_attempt_ms = 0;
    project.save_planning_work().unwrap();
    crate::app::manager::operational_recovery::advance(project);
    assert!(!project.investigation_attempted.contains(key));
    assert_eq!(
        project
            .planning_work
            .iter()
            .find(|w| w.operational_recovery.is_some())
            .unwrap()
            .operational_recovery
            .as_ref()
            .unwrap()
            .attempts,
        1
    );
    let _worker_lock = crate::core::investigation::try_claim(&project.state, key)
        .unwrap()
        .unwrap();
    assert!(crate::app::manager::operational_recovery::claim_investigation(project, key));
    let before_worker = project.state.planning_store.revision().unwrap();
    let wrong_uid = uuid::Uuid::new_v4().to_string();
    assert!(
        crate::app::manager::operational_recovery::investigation_callback_state(
            project,
            key,
            Some(&wrong_uid),
            Some(1)
        )
        .unwrap()
        .is_none()
    );
    assert!(
        crate::app::manager::operational_recovery::investigation_callback_state(
            project,
            key,
            Some(&uid),
            Some(0)
        )
        .unwrap()
        .is_none()
    );
    project.investigation = Some(crate::core::investigation::Controller::idle_fixture(key));
    crate::app::manager::operational_recovery::advance(project);
    assert_eq!(
        project.state.planning_store.revision().unwrap(),
        before_worker,
        "active investigation monitoring must not invalidate its snapshot"
    );
    project.investigation = None;
    crate::app::manager::operational_recovery::advance(project);
    assert_eq!(
        project.state.planning_store.revision().unwrap(),
        before_worker,
        "a worker in another window must not be treated as inactive"
    );
    crate::app::manager::operational_recovery::enqueue_investigation(
        project,
        key,
        &uid,
        "stale worker outcome",
        Some(0),
    );
    assert_eq!(
        project.state.planning_store.revision().unwrap(),
        before_worker,
        "a stale worker failure must not overwrite the current attempt"
    );
    crate::app::manager::operational_recovery::enqueue_investigation(
        project,
        key,
        &uid,
        "The provider still returns an invalid field",
        Some(1),
    );
    project.investigation_attempted.clear();
    project.planning_work =
        crate::core::planning_work::load(&project.state.planning_store).unwrap();
    assert!(crate::app::manager::operational_recovery::investigation_blocked(project, Some(&uid)));
    assert!(
        project
            .planning_work
            .iter()
            .find(|w| w.operational_recovery.is_some())
            .unwrap()
            .detail
            .contains("bounded investigation retry failed")
    );
}
