use super::*;
mod location;
mod review;
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
        presentation::problem(ui, self.conn_error.as_deref());
        match self.onboarding.step {
            Step::Projects => {
                ui.heading("Open or create a project");
                ui.label("Open code you already have and choose its planning repository, or start a new project.");
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
                        self.onboarding.project_directory_name = request
                            .code_directory
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        self.onboarding.creation = Some(request);
                        *next = Some(Step::ReviewProject);
                    }
                }
                if ui
                    .add(presentation::primary_button(
                        "Open an existing code project…",
                    ))
                    .clicked()
                {
                    *next = Some(Step::ExistingProject);
                }
                if ui.button("Create something new").clicked() {
                    *next = Some(Step::NewProject);
                }
                if let Some(id) = self.onboarding.opening_workspace_id
                    && ui.button("Resume opening my workspace").clicked()
                {
                    self.begin_onboarding_open(id, None);
                }
                if self.onboarding_launcher.checking() {
                    ui.spinner();
                    ui.label("Finding your workspaces…");
                }
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Planning workspaces on this computer").strong());
                ui.label("This list shows planning repositories. Use “Open an existing code project” to pair one with its code folder.");
                if welcome::paint_recent(
                    ui,
                    &self.onboarding_launcher.available,
                    &mut self.conn_workspace_id,
                    false,
                ) && let Some(id) = self.conn_workspace_id
                {
                    self.onboarding.selected_entry_path =
                        Some(welcome::onboarding::EntryPath::Recent);
                    self.begin_onboarding_open(id, None);
                }
                for (id, title, missing) in self.onboarding_launcher.unavailable.clone() {
                    ui.push_id(id, |ui| {
                        ui.label(title);
                        ui.label(if missing { "Its planning folder has moved or is missing." } else { "This workspace could not be opened. Review its settings in Advanced options." });
                        if missing && ui.button("Find its planning folder…").clicked()
                            && let Some(folder) = rfd::FileDialog::new().set_title("Reconnect this workspace").pick_folder() {
                            self.onboarding_creation = Some(std::thread::spawn(move || {
                                WorkspaceManager::from_state_root().reconnect(id, &folder)
                                    .map(|workspace| workspace.id).map_err(|error| format!("{error:#}"))
                            }));
                        }
                    });
                }
                if self.onboarding_launcher.history_unavailable {
                    ui.label("Recent ordering is unavailable. Your workspaces are shown by name.");
                }
                if self.onboarding_launcher.error.is_some() {
                    ui.label("Workspaces could not be listed. Retry or use Advanced options to open a known location.");
                }
                if ui.button("Refresh workspaces").clicked() {
                    self.onboarding_launcher.invalidate();
                }
                if ui.button("Review computer setup").clicked() {
                    *next = Some(Step::Readiness);
                }
            }
            Step::NewProject => {
                ui.heading("What do you want to make?");
                let name_label = ui.label("Project name");
                let mut changed =
                    presentation::single_line(ui, &mut self.onboarding.project_name, name_label.id)
                        .changed();
                let idea_label = ui.label("Describe your idea");
                changed |= ui
                    .text_edit_multiline(&mut self.onboarding.project_intent)
                    .labelled_by(idea_label.id)
                    .changed();
                let parent_label = ui.label("Parent folder");
                changed |= presentation::single_line(
                    ui,
                    &mut self.onboarding.project_parent,
                    parent_label.id,
                )
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
                    .add_enabled(ready, presentation::primary_button("Review locations"))
                    .clicked()
                {
                    let parent =
                        std::path::PathBuf::from(&self.onboarding.project_parent).canonicalize();
                    match parent {
                        Ok(parent) => {
                            match location::suggest(&parent, &self.onboarding.project_name) {
                                Ok(code_directory) => {
                                    if self.onboarding.creation.is_none() {
                                        self.onboarding.project_directory_name = code_directory
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .into_owned();
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
                if ui.button("Back").clicked() {
                    *next = Some(Step::Projects);
                }
            }
            Step::ReviewProject => self.paint_new_project_review(ui, next),
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
        self.onboarding_recovery.invalidate();
        match result {
            Ok(id) => {
                if self.onboarding.step == Step::ExistingProject
                    && self
                        .onboarding
                        .existing
                        .as_ref()
                        .is_some_and(|request| !request.create_workspace)
                {
                    self.begin_existing_onboarding_open(id);
                    return;
                }
                let interview = if self.onboarding.step == Step::ReviewProject {
                    self.onboarding.creation.as_ref().map(|request| {
                        (
                            request.intent.clone(),
                            format!("task:{}", request.transaction_id.hyphenated()),
                        )
                    })
                } else {
                    None
                };
                self.begin_onboarding_open(id, interview);
            }
            Err(error) => self.conn_error = Some(error),
        }
    }
}
