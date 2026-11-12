/// Production boundary: a completed lifecycle PR and a malformed-order
/// Manager stream coexist in the private app store across restart. The normal
/// activity tick must preserve the Manager action and must not create liveness
/// work for a TaskIntent whose authoritative PR record is merged. Projection
/// unit tests cannot prove both persisted streams survive app recovery. Uses
/// synthetic local records only; no provider, Docker, or network; moderate
/// existing lifecycle fixture setup.
pub(super) fn verify_merged_task_intent_survives_reordered_manager(
    app: &mut crate::app::root::KooladeApp,
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    task_uid: &str,
    work_uid: &str,
) {
    use crate::{
        app::root::{Screen, board_tests},
        core::{
            lifecycle::{PullRequestPreparationStatus as PrStatus, TaskPhase},
            planning_work::{Work, WorkStatus},
        },
        domain::{ChatMessage, ChatRole, ManagerEventCheckpointV1, ManagerRunEventV1},
    };

    let task = lifecycle.load_task(task_uid).unwrap();
    let feature_uid = task.feature_uid.clone();
    assert_eq!(task.phase, TaskPhase::Done);
    assert_eq!(
        lifecycle.load_feature(&feature_uid).unwrap().phase,
        crate::core::lifecycle::FeaturePhase::Executing,
        "the merged PR record, rather than feature completion, is authoritative"
    );
    assert_eq!(task.pr_work_item_uid.as_deref(), Some(work_uid));
    assert_eq!(
        lifecycle.load_pr_preparation(work_uid).unwrap().status,
        PrStatus::Merged
    );

    let checkpoint = ManagerEventCheckpointV1::from_messages(
        &["Investigate the completed TaskIntent projection".into()],
        None,
        None,
        None,
    )
    .unwrap();
    let run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let event = |sequence, outcome| ManagerRunEventV1 {
        run_id: run_id.clone(),
        sequence,
        outcome,
        checkpoint: (sequence == 1).then(|| checkpoint.clone()),
        checkpoint_cursor: checkpoint.cursor,
        checkpoint_sha256: checkpoint.sha256.clone(),
        retry_after_unix_ms: None,
    };
    let mut queued = ChatMessage::new(ChatRole::System, "Manager queued", None);
    queued.manager_run_event = Some(event(1, crate::domain::ManagerRunOutcomeV1::Queued));
    let mut completed = ChatMessage::new(ChatRole::System, "Manager completed", None);
    completed.manager_run_event = Some(event(3, crate::domain::ManagerRunOutcomeV1::Completed));
    let mut started = ChatMessage::new(ChatRole::System, "Manager started", None);
    started.manager_run_event = Some(event(2, crate::domain::ManagerRunOutcomeV1::Started));

    let manager_root = Work::new(
        "task:reordered-manager-completed-root".into(),
        "Continue Manager supervision".into(),
        "Keep the saved supervision checkpoint actionable.".into(),
        "Manager outcomes were persisted out of order.".into(),
    );
    let manager_cycle = manager_root.uid.clone();
    let mut manager_root = manager_root;
    manager_root.task_intent_uid = Some(manager_root.uid.clone());
    manager_root.task_intent_cycles = vec![crate::core::planning_work::TaskIntentCycleV1 {
        uid: manager_cycle.clone(),
        sequence: 1,
        goal: manager_root.request.clone(),
        work_uid: manager_root.uid.clone(),
        started_at_ms: None,
    }];
    manager_root.task_intent_cycle_uid = Some(manager_cycle.clone());
    let mut blocker = Work::new(
        "task:reordered-manager-completed-blocker".into(),
        "Review the interrupted Manager event".into(),
        "Resolve the preserved supervision checkpoint.".into(),
        "Manager outcomes are out of order.".into(),
    );
    blocker.status = WorkStatus::NeedsAttention;
    blocker.parent_uid = Some(manager_root.uid.clone());
    blocker.task_intent_uid = Some(manager_root.uid.clone());
    blocker.task_intent_cycle_uid = Some(manager_cycle);
    let blocker_uid = blocker.uid.clone();

    let merged_root = Work::new(
        "task:reordered-manager-merged-root".into(),
        "Verify merged lifecycle survives Manager replay".into(),
        "Preserve the durable merged PR outcome.".into(),
        String::new(),
    );
    let mut merged_root = merged_root;
    merged_root.status = WorkStatus::Done;
    merged_root.task_intent_uid = Some(merged_root.uid.clone());
    merged_root.feature_uid = Some(feature_uid.clone());
    let merged_root_uid = merged_root.uid.clone();

    let slug = match &mut app.screen {
        Screen::Connected(project) => {
            project.state.baseline_planning_revision =
                project.state.planning_store.revision().unwrap();
            project.planning_work = vec![manager_root, blocker, merged_root];
            project.save_planning_work().unwrap();
            let slug = project.chat_slug.clone();
            crate::persistence::manager_activity::append(
                &slug,
                &[queued.clone(), completed.clone(), started.clone()],
            )
            .unwrap();
            project.activity.manager_history = crate::persistence::manager_activity::load(&slug);
            slug
        }
        _ => panic!("completed lifecycle fixture remains connected"),
    };

    app.advance_activity();
    assert_replay_blocker_and_merged_root(
        app,
        lifecycle,
        &slug,
        &blocker_uid,
        &merged_root_uid,
        &feature_uid,
        task_uid,
        work_uid,
        &checkpoint,
    );

    let publication_path = match &app.screen {
        Screen::Connected(project) => project
            .state
            .planning_store
            .root
            .join("state/lifecycle_pr_work_items")
            .join(format!("{work_uid}.json")),
        _ => panic!("completed lifecycle fixture remains connected"),
    };
    let publication_backup = publication_path.with_extension("reordered-manager-backup");
    std::fs::rename(&publication_path, &publication_backup).unwrap();
    app.advance_activity();
    let Screen::Connected(project) = &app.screen else {
        panic!("fixture remains connected while publication state is unreadable")
    };
    assert!(project.activity.pending.iter().any(|event| {
        event.starts_with("TaskIntent liveness could not inspect lifecycle publication state")
    }));
    assert!(project.planning_work.iter().all(|work| {
        !work
            .key
            .starts_with(&format!("task:manager-liveness:{merged_root_uid}:"))
    }));
    std::fs::rename(publication_backup, publication_path).unwrap();

    let mut restarted = board_tests::restart_fixture(std::mem::take(app));
    if let Screen::Connected(project) = &mut restarted.screen {
        project.activity.manager_history = crate::persistence::manager_activity::load(&slug);
        project.planning_work = crate::core::planning_work::load(&project.state.planning_store)
            .expect("TaskIntent roots reload from the app store");
    }
    restarted.advance_activity();
    assert_replay_blocker_and_merged_root(
        &mut restarted,
        lifecycle,
        &slug,
        &blocker_uid,
        &merged_root_uid,
        &feature_uid,
        task_uid,
        work_uid,
        &checkpoint,
    );
    *app = restarted;
}

#[allow(clippy::too_many_arguments)]
fn assert_replay_blocker_and_merged_root(
    app: &mut crate::app::root::KooladeApp,
    lifecycle: &crate::core::lifecycle::LifecycleStore<'_>,
    slug: &str,
    blocker_uid: &str,
    merged_root_uid: &str,
    feature_uid: &str,
    task_uid: &str,
    work_uid: &str,
    checkpoint: &crate::domain::ManagerEventCheckpointV1,
) {
    use crate::{
        app::root::Screen,
        core::{lifecycle::PullRequestPreparationStatus as PrStatus, planning_work::WorkStatus},
    };

    let Screen::Connected(project) = &app.screen else {
        panic!("restarted fixture remains connected")
    };
    assert!(
        project
            .activity
            .manager_history_error
            .as_deref()
            .is_some_and(|error| error.contains("sequence 3"))
    );
    assert_eq!(
        project
            .planning_work
            .iter()
            .find(|work| work.uid == blocker_uid)
            .expect("the same Manager blocker survives replay")
            .status,
        WorkStatus::NeedsAttention
    );
    assert!(
        project.planning_work.iter().all(|work| {
            !work
                .key
                .starts_with(&format!("task:manager-liveness:{merged_root_uid}:"))
        }),
        "a durable merged PR must not produce a liveness recovery task"
    );
    assert_eq!(
        lifecycle.replay_feature(feature_uid).unwrap(),
        lifecycle.load_feature(feature_uid).unwrap()
    );
    assert_eq!(
        lifecycle.load_task(task_uid).unwrap().phase,
        crate::core::lifecycle::TaskPhase::Done
    );
    assert_eq!(
        lifecycle.load_pr_preparation(work_uid).unwrap().status,
        PrStatus::Merged
    );
    assert!(
        crate::persistence::manager_activity::load(slug)
            .iter()
            .any(
                |message| message.manager_run_event.as_ref().is_some_and(|event| {
                    event.sequence == 1 && event.checkpoint.as_ref() == Some(checkpoint)
                })
            )
    );
}
