use super::*;
use crate::core::managed_workspace::{ExistingProject, ExistingProjectKind, WorkspaceManager};

impl KooladeApp {
    pub(in crate::app::root) fn apply_onboarding_inspection(
        &mut self,
        (id, folder, result): InspectionResult,
    ) {
        if !self
            .onboarding
            .existing
            .as_ref()
            .is_some_and(|request| request.repository_id == id && request.folder == folder)
        {
            return;
        }
        match result {
            Ok(kind) => self.onboarding_project_kind = Some(kind),
            Err(error) => self.conn_error = Some(error),
        }
    }

    pub(super) fn paint_existing_project(&mut self, ui: &mut egui::Ui, next: &mut Option<Step>) {
        if self.onboarding_creation.is_some() {
            ui.spinner();
            ui.label("Connecting your project and planning history…");
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            return;
        }
        if self
            .onboarding_inspection
            .as_ref()
            .is_some_and(|job| job.is_finished())
        {
            match self.onboarding_inspection.take().unwrap().join() {
                Ok(result) => self.apply_onboarding_inspection(result),
                Err(_) => {
                    self.conn_error = Some("Project inspection stopped. Please check again.".into())
                }
            }
        }
        ui.heading("Use a project on this computer");
        if let Some(error) = &self.conn_error {
            ui.label(error);
        }
        let checking = self.onboarding_inspection.is_some();
        if ui
            .add_enabled(!checking, egui::Button::new("Choose project folder…"))
            .clicked()
            && let Some(folder) = rfd::FileDialog::new().pick_folder()
        {
            match folder.canonicalize() {
                Ok(folder) => {
                    self.onboarding_project_kind = None;
                    self.conn_error = None;
                    let title = folder
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "My project".into());
                    if let Some(previous) = self.onboarding.existing.take()
                        && !self
                            .onboarding
                            .previous_attachments
                            .iter()
                            .any(|item| item.repository_id == previous.repository_id)
                    {
                        self.onboarding.previous_attachments.push(previous);
                    }
                    self.onboarding.existing = Some(ExistingProject {
                        folder,
                        title,
                        workspace_id: uuid::Uuid::new_v4(),
                        repository_id: format!("app-{}", uuid::Uuid::new_v4().simple()),
                        create_workspace: true,
                    });
                    self.onboarding.go(Step::ExistingProject);
                }
                Err(_) => {
                    self.conn_error =
                        Some("This folder cannot be read. Choose an accessible folder.".into())
                }
            }
        }
        let Some(mut request) = self.onboarding.existing.clone() else {
            if ui.link("Back").clicked() {
                *next = Some(Step::Projects);
            }
            return;
        };
        ui.label(format!("Project: {}", request.title));
        ui.label(format!("App code: {}", request.folder.display()));
        if checking {
            ui.spinner();
            ui.label("Checking this folder without changing files…");
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        } else if ui.button("Check folder").clicked() {
            self.onboarding_project_kind = None;
            self.conn_error = None;
            let folder = request.folder.clone();
            let repository_id = request.repository_id.clone();
            self.onboarding_inspection = Some(std::thread::spawn(move || {
                let result = WorkspaceManager::from_state_root()
                    .inspect_existing_project(&folder)
                    .map_err(|error| format!("{error:#}"));
                (repository_id, folder, result)
            }));
        }
        ui.label("Where should Kool.ad/e keep its plans and decisions?");
        ui.label("Planning history is kept separately from application code.");
        let was_new = request.create_workspace;
        let mut changed = ui
            .radio_value(
                &mut request.create_workspace,
                true,
                "Create a separate planning workspace for me",
            )
            .changed();
        changed |= ui
            .radio_value(
                &mut request.create_workspace,
                false,
                "Use an existing planning workspace",
            )
            .changed();
        if !was_new && request.create_workspace {
            request.workspace_id = uuid::Uuid::new_v4();
        }
        if !request.create_workspace {
            let workspaces = WorkspaceManager::from_state_root()
                .list_launcher()
                .map(|listing| listing.available)
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("attach_planning_workspace")
                .selected_text(
                    workspaces
                        .iter()
                        .find(|item| item.id == request.workspace_id)
                        .map(|item| item.title.as_str())
                        .unwrap_or("Choose a workspace"),
                )
                .show_ui(ui, |ui| {
                    for workspace in workspaces {
                        changed |= ui
                            .selectable_value(
                                &mut request.workspace_id,
                                workspace.id,
                                workspace.title,
                            )
                            .changed();
                    }
                });
        }
        if changed {
            self.onboarding.existing = Some(request.clone());
            self.onboarding.go(Step::ExistingProject);
        }
        let planning_path = if request.create_workspace {
            Some(WorkspaceManager::from_state_root().planning_repo_path(request.workspace_id))
        } else {
            WorkspaceManager::from_state_root()
                .list_launcher()
                .map(|listing| listing.available)
                .ok()
                .and_then(|items| {
                    items
                        .into_iter()
                        .find(|item| item.id == request.workspace_id)
                        .map(|item| item.planning_repo)
                })
        };
        if let Some(path) = &planning_path {
            ui.label(format!("Planning history: {}", path.display()));
        } else {
            ui.label("Choose an available planning workspace before connecting.");
        }
        if let Some(kind) = self.onboarding_project_kind {
            let needs_init = kind == ExistingProjectKind::NeedsHistory;
            ui.label(if needs_init {
                "This folder is not tracking changes yet. Approval initializes Git here and connects a separate planning workspace. Existing files will not be committed, moved or deleted."
            } else { "Your branch, history, staged files and working changes stay intact. Only planning records and the local connection are saved." });
            let label = if needs_init {
                "Approve history setup and connect"
            } else {
                "Approve connection"
            };
            if ui
                .add_enabled(
                    self.onboarding.error.is_none() && !checking && planning_path.is_some(),
                    egui::Button::new(label),
                )
                .clicked()
            {
                self.onboarding_creation = Some(std::thread::spawn(move || {
                    WorkspaceManager::from_state_root()
                        .attach_existing_project(&request, needs_init)
                        .map(|workspace| workspace.id)
                        .map_err(|error| format!("{error:#}"))
                }));
            }
        }
        if ui.link("Back").clicked() {
            *next = Some(Step::Projects);
        }
    }
}
