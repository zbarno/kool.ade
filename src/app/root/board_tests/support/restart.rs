use crate::app::{
    root::{KooladeApp, Screen},
    session::Project,
};

/// Rebuild the application session around a fresh PlannerState loaded from the
/// same workspace. Test temp roots move to the new process fixture so they
/// remain alive until the restarted app is dropped.
pub(crate) fn restart_fixture(mut app: KooladeApp) -> KooladeApp {
    let test_temp_roots = std::mem::take(&mut app._test_temp_roots);
    let setup_task_state_path = app.setup_task_state_path.clone();
    let (repo_root, planning_store, chat_slug) = match &app.screen {
        Screen::Connected(project) => (
            project.state.repo_root.clone(),
            project.state.planning_store.clone(),
            project.chat_slug.clone(),
        ),
        _ => panic!("fixture should be connected before restart"),
    };
    drop(app);

    KooladeApp {
        _test_temp_roots: test_temp_roots,
        setup_task_state_path,
        screen: Screen::Connected(Box::new(Project {
            specification_trace_navigation: None,
            task_chats: Default::default(),
            activity: Default::default(),
            state: crate::core::state::PlannerState::load_with_store(&repo_root, &planning_store)
                .unwrap(),
            managed_workspace: None,
            planning_sync: crate::core::planning_sync::PlanningSyncStatus::local(),
            planning_remote_url: None,
            chat_slug,
            chat: Vec::new(),
            draft: String::new(),
            queue: Default::default(),
            queue_lock: None,
            active_implementations: Default::default(),
            implementation_states: Default::default(),
            pr_refresh: None,
            reconciliation: Default::default(),
            investigation: None,
            investigation_attempted: Default::default(),
            investigation_cooldown_until: None,
            last_pr_refresh: None,
            active_turn: None,
            task_turns: Default::default(),
            task_turn_user_message_ids: Default::default(),
            task_turn_reply_claims: Default::default(),
            task_live: Default::default(),
            planning_work: Default::default(),
            active_planning_work: None,
            live_progress: Default::default(),
            next_question_id: None,
            git: Default::default(),
            task_documents: Vec::new(),
            archived_tasks: Default::default(),
            cancelled_work: Default::default(),
        })),
        ..KooladeApp::default()
    }
}
