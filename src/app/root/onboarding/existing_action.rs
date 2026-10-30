use super::*;
use crate::core::managed_workspace::{ExistingProject, ExistingProjectKind, WorkspaceManager};

impl KooladeApp {
    pub(super) fn paint_existing_project_action(&mut self, ui: &mut egui::Ui) {
        let Some(request) = self.onboarding.existing.clone() else {
            return;
        };
        let kind = self.onboarding_project_kind;
        let checking = self.onboarding_inspection.is_some();
        let planning_path = self.onboarding_existing_planning_path(&request);
        let retry_check = kind.is_none() && !checking;
        let cannot_persist_new_workspace =
            request.create_workspace && self.onboarding.error.is_some();
        let enabled = !checking
            && (retry_check || (planning_path.is_some() && !cannot_persist_new_workspace));
        let label = if retry_check {
            if self.conn_error.is_some() {
                "Try code folder check again"
            } else {
                "Check code folder"
            }
        } else if kind == Some(ExistingProjectKind::NeedsHistory) {
            "Set up Git and open project"
        } else {
            "Connect and open project"
        };
        ui.label(if checking {
            "Checking the code folder; opening will be available here when it is ready."
        } else if kind.is_none() {
            if self.conn_error.is_some() {
                "The folder check failed. Review Technical details above, then retry."
            } else {
                "Check the selected code folder to make the open action available."
            }
        } else if planning_path.is_none() {
            "Choose a planning repository above to continue."
        } else if cannot_persist_new_workspace {
            "Setup progress cannot be saved. Choose an existing planning repository or restore application-data access before creating one."
        } else {
            "Connect the selected code and planning folders, then open your project."
        });
        if self.onboarding.error.is_some() && !request.create_workspace {
            ui.label("Saved setup progress has an issue. This explicit open can still continue; if interrupted, you may need to select these folders again.");
        }
        let action_width = ui.available_width();
        if ui
            .add_enabled(
                enabled,
                presentation::primary_button(label).min_size(egui::vec2(action_width, 44.0)),
            )
            .clicked()
        {
            if retry_check {
                self.start_existing_project_inspection(&request);
            } else {
                let needs_init = kind == Some(ExistingProjectKind::NeedsHistory);
                let worker_request = request.clone();
                self.onboarding_creation = Some(std::thread::spawn(move || {
                    WorkspaceManager::from_state_root()
                        .attach_existing_project_with_planning_repo(
                            &worker_request,
                            worker_request.planning_repo.as_deref(),
                            needs_init,
                        )
                        .map(|workspace| workspace.id)
                        .map_err(|error| format!("{error:#}"))
                }));
            }
        }
        ui.add_space(10.0);
    }

    pub(super) fn onboarding_existing_planning_path(
        &self,
        request: &ExistingProject,
    ) -> Option<std::path::PathBuf> {
        if request.create_workspace {
            Some(WorkspaceManager::from_state_root().planning_repo_path(request.workspace_id))
        } else {
            request.planning_repo.clone().or_else(|| {
                self.onboarding_launcher
                    .available
                    .iter()
                    .find(|item| item.id == request.workspace_id)
                    .map(|item| item.planning_repo.clone())
            })
        }
    }
}
