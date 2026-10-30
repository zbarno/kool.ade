use super::*;
use crate::core::managed_workspace::{
    ExistingProject, ExistingProjectKind, WorkspaceManager, WorkspaceManifest,
};

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

    pub(super) fn start_existing_project_inspection(&mut self, request: &ExistingProject) {
        if self.onboarding_inspection.is_some() {
            return;
        }
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

    pub(super) fn paint_existing_project(&mut self, ui: &mut egui::Ui, next: &mut Option<Step>) {
        if self.onboarding_creation.is_some() {
            ui.spinner();
            ui.label("Connecting and opening your project…");
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
        ui.heading("Open your code with its planning repository");
        presentation::problem(ui, self.conn_error.as_deref());
        let picker_busy = self.onboarding_inspection.is_some();
        if ui
            .add_enabled(!picker_busy, egui::Button::new("Choose project folder…"))
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
                    let sibling_planning_repo = folder
                        .parent()
                        .zip(folder.file_name())
                        .map(|(parent, name)| {
                            parent.join(format!("{}.packet", name.to_string_lossy()))
                        })
                        .filter(|path| {
                            WorkspaceManifest::is_managed_repository(path)
                                .is_ok_and(|managed| managed)
                        });
                    let workspace_id = sibling_planning_repo
                        .as_deref()
                        .and_then(|path| WorkspaceManifest::read(path).ok())
                        .map(|manifest| manifest.workspace_id)
                        .unwrap_or_else(uuid::Uuid::new_v4);
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
                        planning_repo: sibling_planning_repo.clone(),
                        title,
                        workspace_id,
                        repository_id: format!("app-{}", uuid::Uuid::new_v4().simple()),
                        create_workspace: sibling_planning_repo.is_none(),
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
            if ui.button("Back").clicked() {
                *next = Some(Step::Projects);
            }
            return;
        };
        if self.onboarding_project_kind.is_none()
            && self.onboarding_inspection.is_none()
            && self.conn_error.is_none()
        {
            self.start_existing_project_inspection(&request);
        }
        let checking = self.onboarding_inspection.is_some();
        ui.label(format!("Project: {}", request.title));
        ui.label(format!("App code: {}", request.folder.display()));
        if checking {
            ui.spinner();
            ui.label("Checking this folder without changing files…");
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        } else if self.onboarding_project_kind.is_none()
            && ui.button("Check folder again").clicked()
        {
            self.start_existing_project_inspection(&request);
        }
        ui.label("Choose the planning repository for this code folder.");
        ui.label("For example, pair kool.ade with its sibling kool.ade.packet.");
        let was_new = request.create_workspace;
        let mut changed = ui
            .radio_value(
                &mut request.create_workspace,
                true,
                "Create a new private planning workspace for this code",
            )
            .changed();
        changed |= ui
            .radio_value(
                &mut request.create_workspace,
                false,
                "Use an existing planning repository",
            )
            .changed();
        if !was_new && request.create_workspace {
            request.workspace_id = uuid::Uuid::new_v4();
            request.planning_repo = None;
        } else if was_new && !request.create_workspace {
            request.planning_repo = None;
        }
        if !request.create_workspace {
            let workspaces = self.onboarding_launcher.available.clone();
            if self.onboarding_launcher.checking() {
                ui.spinner();
                ui.label("Loading planning workspaces…");
            }
            egui::ComboBox::from_id_salt("attach_planning_workspace")
                .selected_text(
                    request
                        .planning_repo
                        .as_ref()
                        .map(|path| format!("Selected folder: {}", path.display()))
                        .or_else(|| {
                            workspaces
                                .iter()
                                .find(|item| item.id == request.workspace_id)
                                .map(|item| item.title.clone())
                        })
                        .unwrap_or_else(|| "Choose a planning repository".into()),
                )
                .show_ui(ui, |ui| {
                    for workspace in workspaces {
                        if ui
                            .selectable_value(
                                &mut request.workspace_id,
                                workspace.id,
                                workspace.title,
                            )
                            .clicked()
                        {
                            request.planning_repo = Some(workspace.planning_repo);
                            changed = true;
                        }
                    }
                });
            if ui.button("Choose planning repository folder…").clicked()
                && let Some(folder) = rfd::FileDialog::new()
                    .set_title("Choose the existing .packet planning repository")
                    .pick_folder()
            {
                match folder.canonicalize() {
                    Ok(folder) => {
                        request.planning_repo = Some(folder);
                        request.workspace_id = request
                            .planning_repo
                            .as_deref()
                            .and_then(|path| WorkspaceManifest::read(path).ok())
                            .map(|manifest| manifest.workspace_id)
                            .unwrap_or_else(uuid::Uuid::new_v4);
                        changed = true;
                    }
                    Err(_) => {
                        self.conn_error = Some(
                            "That planning folder cannot be read. Choose an accessible folder."
                                .into(),
                        );
                    }
                }
            }
        }
        if changed {
            self.onboarding.existing = Some(request.clone());
            self.onboarding.go(Step::ExistingProject);
        }
        let planning_path = self.onboarding_existing_planning_path(&request);
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
        }
        self.onboarding_recovery.paint(
            ui,
            crate::core::managed_workspace::RecoveryRequest::Existing(request.clone()),
        );
        if ui.button("Back").clicked() {
            *next = Some(Step::Projects);
        }
    }
}
