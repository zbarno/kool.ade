use crate::app::session::Project;
use crate::core::state::PlannerState;
pub(super) fn project_from(root: &std::path::Path) -> Project {
    crate::core::state::test_support::seed_published_product_document(root).unwrap();
    let store = crate::app::session::test_support::planning_store_for(root);
    let state = PlannerState::load_with_store(root, &store).unwrap();
    Project {
        specification_trace_navigation: None,
        task_chats: Default::default(),
        activity: Default::default(),
        state,
        chat_slug: "test-slug".into(),
        chat: Vec::new(),
        draft: String::new(),
        queue: Default::default(),
        queue_lock: None,
        active_implementations: Default::default(),
        pr_refresh: None,
        reconciliation: Default::default(),
        investigation: None,
        investigation_attempted: Default::default(),
        investigation_cooldown_until: None,
        last_pr_refresh: None,
        implementation_states: Default::default(),
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
    }
}

pub(super) fn planning_store_for(
    root: &std::path::Path,
) -> crate::artifacts::planning_store::PlanningStore {
    crate::app::session::test_support::planning_store_for(root)
}

pub(super) fn remove_fixture(root: &std::path::Path) {
    crate::app::session::test_support::remove_planning_store_for(root);
    let _ = std::fs::remove_dir_all(root);
}

pub(super) fn isolate_global_git_config(
    root: &std::path::Path,
) -> crate::test_support::ProcessEnvironmentGuard {
    let config = root.join("empty-global-gitconfig");
    std::fs::write(&config, "").unwrap();
    let mut guard = crate::test_support::ProcessEnvironmentGuard::new();
    guard.set("GIT_CONFIG_GLOBAL", config);
    guard
}

pub(super) fn tempdir(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("koolade_dlg_{tag}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&p).unwrap();
    p
}
