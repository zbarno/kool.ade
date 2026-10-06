use crate::app::session::Project;
use crate::core::state::PlannerState;
pub(super) fn project_from(root: &std::path::Path) -> Project {
    let state = PlannerState::load(root).unwrap();
    Project {
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

pub(super) fn tempdir(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("koolade_dlg_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
