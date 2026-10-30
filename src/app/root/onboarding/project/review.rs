use super::*;

impl KooladeApp {
    pub(super) fn paint_new_project_review(&mut self, ui: &mut egui::Ui, next: &mut Option<Step>) {
        if let Some(request) = self.onboarding.creation.clone() {
            ui.heading("Review your new project");
            if self.onboarding.project_directory_name.is_empty() {
                self.onboarding.project_directory_name = request
                    .code_directory
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
            }
            let folder_label = ui.label("App folder name");
            if presentation::single_line(
                ui,
                &mut self.onboarding.project_directory_name,
                folder_label.id,
            )
            .changed()
            {
                self.onboarding.save();
            }
            let current_name = request
                .code_directory
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            if self.onboarding.project_directory_name != current_name {
                ui.label("Changing the location creates a new request. Earlier files and the original request are kept for recovery.");
                if ui.button("Use this folder name").clicked() {
                    match location::edited(
                        &request.code_directory,
                        &self.onboarding.project_directory_name,
                    ) {
                        Ok(path) => {
                            let mut replacement = request.clone();
                            replacement.transaction_id = uuid::Uuid::new_v4();
                            replacement.workspace_id = uuid::Uuid::new_v4();
                            replacement.code_directory = path;
                            if !self
                                .onboarding
                                .previous_creations
                                .iter()
                                .any(|item| item.transaction_id == request.transaction_id)
                            {
                                self.onboarding.previous_creations.push(request);
                            }
                            self.onboarding.creation = Some(replacement);
                            self.onboarding.go(Step::ReviewProject);
                        }
                        Err(error) => self.conn_error = Some(error),
                    }
                    return;
                }
                ui.label("Apply the edited folder name before approving creation.");
                if ui.button("Back").clicked() {
                    *next = Some(Step::NewProject);
                }
                return;
            }
            self.onboarding_recovery.paint(
                ui,
                crate::core::managed_workspace::RecoveryRequest::New(request.clone()),
            );
            ui.label(format!("App code: {}", request.code_directory.display()));
            ui.label(format!(
                "Planning history: {}",
                WorkspaceManager::from_state_root()
                    .planning_repo_path(request.workspace_id)
                    .display()
            ));
            ui.label("Create two separate local repositories and save their connection. No package installation, remote publishing or app-file commit will run. If creation stops, files are preserved for retry.");
            if ui
                .add_enabled(
                    self.onboarding.error.is_none(),
                    presentation::primary_button("Approve and create project"),
                )
                .clicked()
            {
                self.conn_error = None;
                self.onboarding_creation = Some(std::thread::spawn(move || {
                    WorkspaceManager::from_state_root()
                        .create_application(&request)
                        .map(|workspace| workspace.id)
                        .map_err(|error| format!("{error:#}"))
                }));
            }
        }
        if ui.button("Back").clicked() {
            *next = Some(Step::NewProject);
        }
    }
}
