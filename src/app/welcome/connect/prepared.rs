//! Load connection data on a worker, then construct UI-owned runtime state on
//! the UI thread. No running controller crosses the thread boundary.
use crate::{
    app::{manager::WorkspaceActivity, session::Project},
    error::AppError,
};

pub struct PreparedConnection(Box<dyn FnOnce() -> Project + Send>);

impl PreparedConnection {
    pub fn finish(self) -> Project {
        (self.0)()
    }

    fn from_new_project(project: Project) -> Result<Self, AppError> {
        if project.active_turn.is_some()
            || !project.task_turns.is_empty()
            || !project.active_implementations.is_empty()
            || project.investigation.is_some()
            || project.pr_refresh.is_some()
        {
            return Err(AppError::Other(
                "Connection unexpectedly started a worker before handoff.".into(),
            ));
        }
        let Project {
            state,
            managed_workspace,
            planning_sync,
            planning_remote_url,
            chat_slug,
            chat,
            planning_work,
            queue,
            git,
            task_documents,
            archived_tasks,
            cancelled_work,
            implementation_states,
            activity,
            ..
        } = project;
        let WorkspaceActivity {
            tasks,
            pending,
            dirty_tickets,
            pending_planning_work,
            pending_dependency_reviews,
            ..
        } = activity;
        Ok(Self(Box::new(move || Project {
            state,
            managed_workspace,
            planning_sync,
            planning_remote_url,
            chat_slug,
            chat,
            planning_work,
            queue,
            git,
            task_documents,
            archived_tasks,
            cancelled_work,
            implementation_states,
            activity: WorkspaceActivity {
                tasks,
                pending,
                dirty_tickets,
                pending_planning_work,
                pending_dependency_reviews,
                ..Default::default()
            },
            specification_trace_navigation: None,
            task_chats: Default::default(),
            draft: String::new(),
            queue_lock: None,
            active_implementations: Default::default(),
            pr_refresh: None,
            reconciliation: Default::default(),
            investigation: None,
            investigation_attempted: Default::default(),
            investigation_cooldown_until: None,
            last_pr_refresh: None,
            active_turn: None,
            task_turns: Default::default(),
            task_live: Default::default(),
            active_planning_work: None,
            live_progress: Default::default(),
            next_question_id: None,
        })))
    }
}

pub fn prepare_open_workspace(id: uuid::Uuid) -> Result<PreparedConnection, AppError> {
    PreparedConnection::from_new_project(super::attempt_open_workspace(id)?)
}
