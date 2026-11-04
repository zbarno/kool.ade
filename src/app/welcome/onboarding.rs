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

#[derive(Serialize, Deserialize)]
pub struct Onboarding {
    version: u32,
    #[serde(default)]
    pub mode: Mode,
    /// Role key only; HarnessSettings remains authoritative for provider/model.
    #[serde(default)]
    pub selected_manager_route_ref: Option<String>,
    #[serde(default)]
    pub active_setup_task_id: Option<String>,
    /// A task to revisit, never approval to resume a mutation.
    #[serde(default)]
    pub pending_operation_task_id: Option<String>,
    #[serde(default)]
    pub help_open: bool,
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
            version: 1,
            mode: Mode::FirstRun,
            selected_manager_route_ref: None,
            active_setup_task_id: None,
            pending_operation_task_id: None,
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
                fresh()
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
