use super::*;
mod location;
use crate::core::managed_workspace::{ApplicationCreation, WorkspaceManager};

impl KooladeApp {
    pub(super) fn paint_project_entry(&mut self, ui: &mut egui::Ui, next: &mut Option<Step>) {
        if self.onboarding_creation.is_some() {
            ui.spinner();
            ui.label("Creating your project and its separate planning history…");
            ui.label("Closing the app may leave partial work. Reopen and review the same request to resume; existing files will not be deleted.");
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            return;
        }
        if let Some(error) = &self.conn_error {
            ui.label(error);
        }
        match self.onboarding.step {
            Step::Projects => {
                ui.heading("What would you like to do?");
                for request in self.onboarding.previous_attachments.clone() {
                    if ui
                        .button(format!("Resume connecting: {}", request.title))
                        .clicked()
                    {
                        self.onboarding.existing = Some(request);
                        self.onboarding_project_kind = None;
                        *next = Some(Step::ExistingProject);
                    }
                }
                for request in self.onboarding.previous_creations.clone() {
                    if ui
                        .button(format!("Resume creation: {}", request.title))
                        .clicked()
                    {
                        self.onboarding.project_name = request.title.clone();
                        self.onboarding.project_intent = request.intent.clone();
                        self.onboarding.project_parent = request
                            .code_directory
                            .parent()
                            .map(|path| path.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.onboarding.creation = Some(request);
                        *next = Some(Step::ReviewProject);
                    }
                }
                if ui.button("Create something new").clicked() {
                    *next = Some(Step::NewProject);
                }
                if ui.button("Use an existing project").clicked() {
                    *next = Some(Step::ExistingProject);
                }
                let manager = WorkspaceManager::from_state_root();
                match manager.list_launcher() {
                    Ok(launcher) => {
                        let mut workspaces = launcher.available;
                        let _ = manager.sort_by_recent_open(&mut workspaces);
                        if welcome::paint_recent(
                            ui,
                            &workspaces,
                            &mut self.conn_workspace_id,
                            false,
                        ) {
                            self.submit_open_local_workspace();
                        }
                        for id in launcher.unavailable {
                            ui.push_id(id, |ui| {
                                let (title, missing) = manager.unavailable_workspace_label(id);
                                ui.label(title);
                                ui.label(if missing { "Its planning folder has moved or is missing." } else { "This workspace could not be opened. Check its permissions and repository settings in Advanced options." });
                                if missing && ui.button("Find its planning folder…").clicked()
                                    && let Some(folder) = rfd::FileDialog::new()
                                        .set_title("Reconnect this workspace")
                                        .pick_folder()
                                {
                                    self.onboarding_creation =
                                        Some(std::thread::spawn(move || {
                                            WorkspaceManager::from_state_root()
                                                .reconnect(id, &folder)
                                                .map(|workspace| workspace.id)
                                                .map_err(|error| format!("{error:#}"))
                                        }));
                                }
                            });
                        }
                    }
                    Err(_) => {
                        ui.label("A workspace could not be found. Use Advanced options to locate its planning folder.");
                    }
                }
                if ui.link("Review computer setup").clicked() {
                    *next = Some(Step::Readiness);
                }
            }
            Step::NewProject => {
                ui.heading("What do you want to make?");
                ui.label("Project name");
                let mut changed = ui
                    .text_edit_singleline(&mut self.onboarding.project_name)
                    .changed();
                ui.label("Describe your idea");
                changed |= ui
                    .text_edit_multiline(&mut self.onboarding.project_intent)
                    .changed();
                ui.label("Parent folder");
                changed |= ui
                    .text_edit_singleline(&mut self.onboarding.project_parent)
                    .changed();
                if ui.button("Choose folder…").clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder()
                {
                    self.onboarding.project_parent = folder.to_string_lossy().into_owned();
                    changed = true;
                }
                if changed {
                    if let Some(previous) = self.onboarding.creation.take()
                        && !self
                            .onboarding
                            .previous_creations
                            .iter()
                            .any(|item| item.transaction_id == previous.transaction_id)
                    {
                        self.onboarding.previous_creations.push(previous);
                    }
                    self.onboarding.go(Step::NewProject);
                }
                let ready = !self.onboarding.project_name.trim().is_empty()
                    && !self.onboarding.project_intent.trim().is_empty();
                if ui
                    .add_enabled(ready, egui::Button::new("Review locations"))
                    .clicked()
                {
                    let parent =
                        std::path::PathBuf::from(&self.onboarding.project_parent).canonicalize();
                    match parent {
                        Ok(parent) => {
                            match location::suggest(&parent, &self.onboarding.project_name) {
                                Ok(code_directory) => {
                                    if self.onboarding.creation.is_none() {
                                        self.onboarding.creation = Some(ApplicationCreation {
                                            transaction_id: uuid::Uuid::new_v4(),
                                            workspace_id: uuid::Uuid::new_v4(),
                                            title: self.onboarding.project_name.trim().into(),
                                            intent: self.onboarding.project_intent.trim().into(),
                                            code_directory,
                                        });
                                    }
                                    *next = Some(Step::ReviewProject);
                                }
                                Err(error) => self.conn_error = Some(error),
                            }
                        }
                        Err(_) => {
                            self.conn_error =
                                Some("Choose an existing parent folder you can write to.".into())
                        }
                    }
                }
                if ui.link("Back").clicked() {
                    *next = Some(Step::Projects);
                }
            }
            Step::ReviewProject => {
                if let Some(request) = self.onboarding.creation.clone() {
                    ui.heading("Review your new project");
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
                            egui::Button::new("Approve and create project"),
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
                if ui.link("Back").clicked() {
                    *next = Some(Step::NewProject);
                }
            }
            _ => {}
        }
    }

    pub(super) fn poll_onboarding_creation(&mut self) {
        if !self
            .onboarding_creation
            .as_ref()
            .is_some_and(|job| job.is_finished())
        {
            return;
        }
        let result = self
            .onboarding_creation
            .take()
            .unwrap()
            .join()
            .unwrap_or_else(|_| {
                Err("Project creation stopped. Review the request and retry.".into())
            });
        match result {
            Ok(id) => {
                self.finish_connect(welcome::attempt_open_workspace(id));
                if matches!(self.screen, Screen::Connected(_)) {
                    if self.onboarding.step == Step::ReviewProject
                        && let Some(request) = &self.onboarding.creation
                    {
                        let intent = request.intent.clone();
                        let key = format!("task:{}", request.transaction_id.hyphenated());
                        self.start_turn_for_work(
                            &intent,
                            crate::core::workflow::TurnPurpose::Interview,
                            None,
                            Some(key),
                        );
                    }
                    self.onboarding.go(Step::Projects);
                }
            }
            Err(error) => self.conn_error = Some(error),
        }
    }
}
