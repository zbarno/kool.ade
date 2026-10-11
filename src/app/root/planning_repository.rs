use super::{KooladeApp, Screen};
use crate::{
    core::{managed_workspace::WorkspaceManager, planning_sync::PlanningSyncStatus},
    ui::PlanningRepositoryCommand,
};

impl KooladeApp {
    pub(super) fn handle_planning_repository_command(
        &mut self,
        command: PlanningRepositoryCommand,
    ) {
        match command {
            PlanningRepositoryCommand::SyncNow => self.start_planning_sync(None),
            PlanningRepositoryCommand::ResolveConflict {
                operation_id,
                path,
                choice,
                manual_value,
            } => self.start_planning_sync(Some((operation_id, path, choice, manual_value))),
            PlanningRepositoryCommand::CancelPendingOperation { operation_id } => {
                self.cancel_pending_planning_operation(operation_id)
            }
            PlanningRepositoryCommand::MapCodeCheckout { repository_id } => {
                self.map_workspace_checkout(&repository_id)
            }
            PlanningRepositoryCommand::ForgetCodeCheckout { repository_id } => {
                self.forget_workspace_checkout(&repository_id)
            }
            PlanningRepositoryCommand::ExportPending => self.export_pending_planning(),
        }
    }

    fn start_planning_sync(
        &mut self,
        resolution: Option<(
            uuid::Uuid,
            String,
            crate::core::planning_sync::PlanningConflictChoice,
            Option<String>,
        )>,
    ) {
        if self.planning_sync_job.is_some() {
            return;
        }
        let Screen::Connected(project) = &self.screen else {
            return;
        };
        let Some(workspace) = project.managed_workspace.as_ref() else {
            self.toasts
                .warning("Connect a managed planning workspace before syncing.");
            return;
        };
        if project.active_turn.is_some() || !project.active_implementations.is_empty() {
            self.toasts
                .warning("Wait for active planning and implementation writes before syncing.");
            return;
        }
        let id = workspace.id;
        let manager = WorkspaceManager::from_state_root();
        self.planning_sync_job = Some(std::thread::spawn(move || {
            let result = match resolution {
                Some((operation_id, path, choice, manual_value)) => manager
                    .resolve_planning_conflict(
                        id,
                        operation_id,
                        &path,
                        choice,
                        manual_value.as_deref(),
                    )
                    .map_err(|error| error.to_string()),
                None => manager.sync_planning(id).map_err(|error| error.to_string()),
            };
            (id, result)
        }));
    }

    pub(super) fn poll_planning_sync(&mut self, ctx: &egui::Context) {
        let Some(job) = self.planning_sync_job.as_ref() else {
            return;
        };
        if !job.is_finished() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
            return;
        }
        let Some(job) = self.planning_sync_job.take() else {
            return;
        };
        let (id, result) = match job.join() {
            Ok(result) => result,
            Err(_) => {
                self.toasts
                    .warning("Planning sync stopped unexpectedly; local work is preserved.");
                return;
            }
        };
        let status = match result {
            Ok(status) => status,
            Err(error) => {
                let manager = WorkspaceManager::from_state_root();
                let mut status = manager
                    .planning_sync_status(id)
                    .unwrap_or_else(|_| PlanningSyncStatus::local());
                status.detail = Some(error.clone());
                self.toasts
                    .warning(format!("Planning sync needs attention: {error}"));
                status
            }
        };
        let refresh = if let Screen::Connected(project) = &mut self.screen {
            if project.state.planning_store.project_id == id {
                project.planning_sync = status.clone();
                refresh_project_from_planning_store(project, id)
            } else {
                Ok(())
            }
        } else {
            Ok(())
        };
        if let Err(error) = refresh {
            self.toasts
                .warning(format!("Planning state refreshed with an error: {error:#}"));
        }
        if let Screen::Connected(project) = &self.screen {
            let (cached_user, synth) = Self::derive_caches(project);
            self.cached_user = cached_user;
            self.synth = synth;
        }
        match status.state {
            crate::core::planning_sync::PlanningSyncState::Synced => {
                self.toasts.success("Planning repository sync confirmed");
            }
            crate::core::planning_sync::PlanningSyncState::Conflicted => {
                self.toasts
                    .warning("Planning conflicts need review in Workspace settings");
            }
            _ => {}
        }
    }

    pub(super) fn refresh_planning_sync_status(&mut self) {
        if self.planning_sync_job.is_some() {
            return;
        }
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.planning_sync.mode != crate::core::planning_sync::PlanningSyncMode::Shared {
            return;
        }
        let Some(workspace) = project.managed_workspace.as_ref() else {
            project.planning_sync.state = crate::core::planning_sync::PlanningSyncState::Pending;
            project.planning_sync.detail =
                Some("Shared synchronization status is unavailable; sync to verify.".into());
            return;
        };
        match crate::core::planning_sync::sync_status_at(
            &workspace.planning_repo,
            &crate::persistence::state_root(),
        ) {
            Ok(status) => project.planning_sync = status,
            Err(error) => {
                if project.planning_sync.state
                    == crate::core::planning_sync::PlanningSyncState::Synced
                {
                    project.planning_sync.state =
                        crate::core::planning_sync::PlanningSyncState::Pending;
                }
                project.planning_sync.detail =
                    Some(format!("Synchronization status could not be read: {error}"));
            }
        }
    }

    fn cancel_pending_planning_operation(&mut self, operation_id: uuid::Uuid) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if self.planning_sync_job.is_some()
            || project.active_turn.is_some()
            || !project.active_implementations.is_empty()
        {
            self.toasts
                .warning("Wait for active planning and implementation writes before cancelling.");
            return;
        }
        let Some(workspace) = &project.managed_workspace else {
            return;
        };
        let manager = WorkspaceManager::from_state_root();
        match manager.cancel_pending_operation(workspace.id, operation_id) {
            Ok(status) => {
                project.planning_sync = status;
                if let Err(error) = refresh_project_from_planning_store(project, workspace.id) {
                    self.toasts.warning(format!(
                        "Pending operation was cancelled, but the board could not be refreshed: {error:#}"
                    ));
                }
                let (cached_user, synth) = Self::derive_caches(project);
                self.cached_user = cached_user;
                self.synth = synth;
                self.toasts
                    .success("Uncommitted planning operation cancelled");
            }
            Err(error) => self
                .toasts
                .warning(format!("Planning operation was preserved: {error:#}")),
        }
    }

    fn export_pending_planning(&mut self) {
        let Screen::Connected(project) = &self.screen else {
            return;
        };
        let Some(workspace) = &project.managed_workspace else {
            return;
        };
        let id = workspace.id;
        let suggested = format!("unsynced-planning-{id}.json");
        let Some(destination) = rfd::FileDialog::new().set_file_name(&suggested).save_file() else {
            return;
        };
        match WorkspaceManager::from_state_root().export_pending_planning(id, &destination) {
            Ok(_) => self.toasts.success(format!(
                "Unsynced changes exported to {}",
                destination.display()
            )),
            Err(error) => self
                .toasts
                .warning(format!("Unsynced changes were not exported: {error:#}")),
        }
    }

    fn map_workspace_checkout(&mut self, repository_id: &str) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Choose this code repository checkout")
            .pick_folder()
        else {
            return;
        };
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let Some(workspace) = &project.managed_workspace else {
            return;
        };
        match WorkspaceManager::from_state_root().map_code_checkout(
            workspace.id,
            repository_id,
            &path,
        ) {
            Ok(workspace) => {
                project.managed_workspace = Some(workspace);
                self.toasts
                    .success(format!("Checkout mapped for {repository_id}"));
            }
            Err(error) => self
                .toasts
                .warning(format!("Checkout was not mapped: {error:#}")),
        }
    }

    fn forget_workspace_checkout(&mut self, repository_id: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let Some(workspace) = &project.managed_workspace else {
            return;
        };
        match WorkspaceManager::from_state_root().disconnect_checkout(workspace.id, repository_id) {
            Ok(workspace) => {
                project.managed_workspace = Some(workspace);
                self.toasts.info(format!(
                    "Forgot the local checkout mapping for {repository_id}"
                ));
            }
            Err(error) => self
                .toasts
                .warning(format!("Checkout mapping was preserved: {error:#}")),
        }
    }
}

fn refresh_project_from_planning_store(
    project: &mut crate::app::session::Project,
    workspace_id: uuid::Uuid,
) -> anyhow::Result<()> {
    let manager = WorkspaceManager::from_state_root();
    let store = project.state.planning_store.clone();
    let mut state =
        crate::core::state::PlannerState::load_with_store(&project.state.repo_root, &store)?;
    let (planning_work, revision) =
        crate::core::planning_work::load_expected(&store, &state.baseline_planning_revision)?;
    state.baseline_planning_revision = revision;
    let workspace = manager.open(workspace_id)?;
    project.state = state;
    project.planning_work = planning_work;
    project.task_documents = crate::artifacts::task_docs::load_board(
        &project.state.planning_store,
        &project.state.workflow,
    );
    project.managed_workspace = Some(workspace.clone());
    project.planning_remote_url = manager.planning_remote_url(workspace_id)?;
    project.next_question_id = None;
    Ok(())
}
