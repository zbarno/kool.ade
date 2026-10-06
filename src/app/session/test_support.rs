use super::Project;
use crate::core::state::PlannerState;
use std::path::Path;

pub(crate) fn project_from(root: &Path) -> Project {
    Project {
        task_chats: Default::default(),
        activity: Default::default(),
        state: PlannerState::load(root).unwrap(),
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
