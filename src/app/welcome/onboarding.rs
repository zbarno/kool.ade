//! Private navigation state, never an authority for setup or repair completion.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    #[default]
    Start,
    Manager,
    Readiness,
    Projects,
    NewProject,
    ReviewProject,
    ExistingProject,
    Advanced,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    FirstRun,
    Returning,
    Resume,
    Advanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryPath {
    Recent,
    ExistingProject,
    NewProject,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InterviewRef {
    pub workspace_id: uuid::Uuid,
    pub work_key: String,
}

#[derive(Serialize, Deserialize)]
pub struct Onboarding {
    version: u32,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub selected_entry_path: Option<EntryPath>,
    /// Role key only; HarnessSettings remains authoritative for provider/model.
    #[serde(default)]
    pub selected_manager_route_ref: Option<String>,
    #[serde(default)]
    pub active_setup_task_id: Option<String>,
    /// A task to revisit, never approval to resume a mutation.
    #[serde(default)]
    pub pending_operation_task_id: Option<String>,
    #[serde(default)]
    pub opening_workspace_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub pending_interview: Option<InterviewRef>,
    #[serde(default)]
    pub help_open: bool,
    #[serde(default)]
    pub existing: Option<crate::core::managed_workspace::ExistingProject>,
    #[serde(default)]
    pub previous_attachments: Vec<crate::core::managed_workspace::ExistingProject>,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub project_intent: String,
    #[serde(default)]
    pub project_parent: String,
    #[serde(default)]
    pub project_directory_name: String,
    #[serde(default)]
    pub creation: Option<crate::core::managed_workspace::ApplicationCreation>,
    #[serde(default)]
    pub previous_creations: Vec<crate::core::managed_workspace::ApplicationCreation>,
    pub step: Step,
    pub revision: u64,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    pub error: Option<String>,
}

impl Onboarding {
    pub fn load(path: PathBuf) -> Self {
        let fresh = || Self {
            project_name: String::new(),
            project_intent: String::new(),
            project_directory_name: String::new(),
            project_parent: std::env::var("HOME").unwrap_or_default(),
            existing: None,
            previous_attachments: Vec::new(),
            creation: None,
            previous_creations: Vec::new(),
            version: 1,
            mode: Mode::FirstRun,
            selected_entry_path: None,
            selected_manager_route_ref: None,
            active_setup_task_id: None,
            pending_operation_task_id: None,
            opening_workspace_id: None,
            pending_interview: None,
            help_open: false,
            step: Step::Start,
            revision: 0,
            updated_at: chrono::Utc::now(),
            path: path.clone(),
            error: None,
        };
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Self>(&bytes) {
                Ok(mut session) if session.version == 1 => {
                    session.path = path;
                    session.mode = match session.step {
                        Step::Advanced => Mode::Advanced,
                        Step::Projects => Mode::Returning,
                        _ => Mode::Resume,
                    };
                    session
                }
                _ => Self { error: Some("Saved setup progress could not be read. You can still use Advanced options; the saved file will be preserved.".into()), ..fresh() },
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut session = fresh();
                // Existing users keep their launcher; setup regressions remain
                // scoped checks rather than an imposed first-run interview.
                if let Some(home) = path.parent()
                    && crate::core::managed_workspace::WorkspaceManager::new(home)
                        .has_local_associations()
                {
                    session.step = Step::Projects;
                    session.mode = Mode::Returning;
                }
                session
            },
            Err(_) => Self { error: Some("Setup progress is unavailable. Check access to your application data folder.".into()), ..fresh() },
        }
    }

    pub fn go(&mut self, step: Step) {
        self.step = step;
        match step {
            Step::Advanced => self.mode = Mode::Advanced,
            Step::Projects => self.mode = Mode::Returning,
            Step::Manager => {
                self.selected_manager_route_ref =
                    Some(crate::persistence::harness_settings::MANAGER.into())
            }
            Step::NewProject => self.selected_entry_path = Some(EntryPath::NewProject),
            Step::ExistingProject => self.selected_entry_path = Some(EntryPath::ExistingProject),
            _ => {}
        }
        self.revision = self.revision.saturating_add(1);
        self.save();
    }

    pub fn save(&mut self) {
        self.updated_at = chrono::Utc::now();
        if self.error.is_none() {
            let result = serde_json::to_string_pretty(self)
                .map_err(anyhow::Error::from)
                .and_then(|text| crate::artifacts::atomic_write(&self.path, &text));
            if result.is_err() {
                self.error = Some("Could not save setup progress. Check access to your application data folder before closing.".into());
            }
        }
    }
}

#[cfg(test)]
mod tests;
