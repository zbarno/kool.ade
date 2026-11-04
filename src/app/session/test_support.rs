use super::Project;
use crate::core::state::PlannerState;
use std::path::Path;

pub(crate) fn planning_store_for(root: &Path) -> crate::artifacts::planning_store::PlanningStore {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let project_id = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        canonical.to_string_lossy().as_bytes(),
    );
    let private_root = std::env::temp_dir().join(format!("koolade-session-store-{project_id}"));
    crate::artifacts::planning_store::PlanningStore::for_code_repository(
        project_id,
        private_root,
        root,
    )
}

pub(crate) fn remove_planning_store_for(root: &Path) {
    let store = planning_store_for(root);
    let _ = std::fs::remove_dir_all(store.root);
}

pub(crate) fn project_from(root: &Path) -> Project {
    crate::core::state::test_support::seed_published_product_document(root).unwrap();
    crate::test_support::set_test_origin_default(root);
    project_from_prepared(root)
}

pub(crate) fn project_from_prepared(root: &Path) -> Project {
    let store = planning_store_for(root);
    project_from_store(root, &store)
}

pub(crate) fn project_from_store(
    root: &Path,
    store: &crate::artifacts::planning_store::PlanningStore,
) -> Project {
    Project {
        specification_trace_navigation: None,
        task_chats: Default::default(),
        activity: Default::default(),
        state: PlannerState::load_with_store(root, store).unwrap(),
        chat_slug: "test-repository-names".into(),
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
    }
}
