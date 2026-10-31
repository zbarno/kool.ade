//! Welcome front door.
//!
//! The welcome screen leads with two journeys operators actually
//! start from — (1) clone a code repository with its planning pair, or
//! (2) start a blank project — each opening a
//! two-step wizard: gather the few inputs, then REVIEW the exact side
//! effects on disk before anything is written. Saved managed workspaces
//! reopen from their recorded workspace association.
#![allow(clippy::too_many_arguments)]
use std::cell::Cell;

use super::KooladeApp;
mod state;
pub(super) use state::{WelcomeEntry, WelcomeWizard, WizField};

/// Discrete intents the primary welcome card can request from the caller.
pub(super) enum WizAction {
    Continue,
    Back,
    Commit,
    Close,
}

/// Action bus bridging egui's paint pass and the mutable handlers; cells
/// are drained AFTER the card paints.
pub(super) struct PrimaryBus<'a> {
    pub(super) door: &'a Cell<Option<WelcomeEntry>>,
    pub(super) action: &'a Cell<Option<WizAction>>,
    pub(super) browse: &'a Cell<Option<WizField>>,
}

impl<'a> PrimaryBus<'a> {
    /// Defers a folder-picker request: painted code cannot re-route the
    /// browser target mid-frame, so the pick is recorded and drained by the
    /// caller right after the card paints.
    pub(super) fn request_browse(&self, which: WizField) {
        self.browse.set(Some(which));
    }
}

impl KooladeApp {
    // ------------------------------------------------------------------
    // Draft lifecycle
    // ------------------------------------------------------------------

    /// Opens a fresh wizard draft. While a clone worker is in flight the
    /// welcome surface is committed to that job, so doors are inert.
    pub(super) fn wizard_open(&mut self, entry: WelcomeEntry) {
        if self.clone_job.is_some() {
            return;
        }
        self.wiz = Some(WelcomeWizard::new(entry));
    }

    /// Discards the active draft. A clone worker already launched keeps running.
    pub(super) fn wizard_close(&mut self) {
        self.wiz = None;
    }

    /// Step 1 → step 2 gate: collects the entry's required input or shows
    /// exactly what is still owed.
    pub(super) fn wizard_continue(&mut self) {
        let (entry, url, name, parent) = match self.wiz.as_ref() {
            Some(wiz) => (
                wiz.entry,
                wiz.url.trim().to_string(),
                wiz.effective_project_name(),
                wiz.parent.trim().to_string(),
            ),
            None => return,
        };

        match entry {
            WelcomeEntry::CloneRepository => {
                if url.is_empty() {
                    self.wiz_error("Paste the GitHub URL for the code repository.");
                    return;
                }
                let target = match crate::app::welcome::parse_github_url(&url) {
                    Ok(target) => target,
                    Err(detail) => {
                        self.wiz_error(format!("That URL doesn't look cloneable\n{detail}"));
                        return;
                    }
                };
                if format!("{}.packet", target.repo).len() > 100 {
                    self.wiz_error("The paired planning repository name would exceed GitHub's 100-character limit. Rename the code repository before cloning it.");
                    return;
                }
            }
            WelcomeEntry::BlankProject => {
                if name.is_empty() {
                    self.wiz_error("Give the project a name — the code folder is named for it.");
                    return;
                }
                if parent.is_empty() {
                    self.browse_wizard_field(WizField::Parent);
                    return;
                }
                // Derive the candidate EXACTLY the way the commit (and the
                // plan lines) do: the slugged code-folder name, never the
                // raw title — operators routinely keep spaces in titles.
                let candidate = format!(
                    "{}.packet",
                    crate::app::root::KooladeApp::project_title_to_folder_name(&name)
                );
                if let Some(problem) = naming_problem(&candidate) {
                    self.wiz_error(problem);
                    return;
                }
            }
        }
        if let Some(wiz) = self.wiz.as_mut() {
            wiz.step = 1;
            wiz.error = None;
        }
    }

    pub(super) fn wizard_back(&mut self) {
        if let Some(wiz) = self.wiz.as_mut() {
            wiz.step = 0;
            wiz.error = None;
        }
    }

    /// Single writer for wizard errors: bounces back to the inputs step so
    /// the operator corrects what the message points at.
    pub(super) fn wiz_error(&mut self, message: impl Into<String>) {
        if let Some(wiz) = self.wiz.as_mut() {
            wiz.error = Some(message.into());
            wiz.step = 0;
        }
    }

    /// Router for everything the primary card can ask for.
    pub(super) fn wizard_actuate(&mut self, action: WizAction) {
        match action {
            WizAction::Close => self.wizard_close(),
            WizAction::Continue => self.wizard_continue(),
            WizAction::Back => self.wizard_back(),
            WizAction::Commit => self.wizard_commit(),
        }
    }

    pub(super) fn browse_wizard_field(&mut self, which: WizField) {
        self.pick_wizard_folder(which);
    }

    /// Confirm-step authority. Nothing on the welcome surface writes to
    /// disk before this runs, and each branch spells out its effect.
    pub(super) fn wizard_commit(&mut self) {
        let (entry, project, url, parent) = match self.wiz.as_ref() {
            Some(wiz) => (
                wiz.entry,
                wiz.effective_project_name(),
                wiz.url.trim().to_string(),
                wiz.parent.trim().to_string(),
            ),
            None => return,
        };

        match entry {
            WelcomeEntry::BlankProject => {
                let code_name = Self::project_title_to_folder_name(&project);
                if parent.is_empty() {
                    self.wiz_error("Choose the folder where the project lives.");
                    return;
                }
                let mut spec =
                    crate::core::managed_workspace::NewProjectSpec::new(project, code_name.clone());
                spec.parent_dir = std::path::PathBuf::from(parent);
                match crate::app::welcome::attempt_create_project(&spec) {
                    Ok(project) => self.finish_connect(Ok(project)),
                    Err(error) => self.wiz_error(error.to_string()),
                }
            }
            WelcomeEntry::CloneRepository => {
                if self.clone_job.is_some() {
                    self.wiz_error("A clone is already in flight.");
                    return;
                }
                if url.is_empty() {
                    self.wiz_error("Paste the repository URL first.");
                    return;
                }
                self.begin_clone(&url, &project);
                if self.clone_job.is_some() {
                    // Stay on the review step while the worker clones and pairs.
                } else {
                    let reason = self.conn_error.clone();
                    self.wiz_error(match reason {
                        Some(detail) => format!("The clone couldn't start yet:\n{detail}"),
                        None => "Couldn't start the clone worker. Try again.".to_string(),
                    });
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Folder-picker bridges (routed from connection.rs)
    // ------------------------------------------------------------------

    pub(super) fn wiz_seed_for(&self, which: WizField) -> String {
        let Some(wiz) = self.wiz.as_ref() else {
            return String::new();
        };
        match which {
            WizField::Parent => wiz.parent.trim().to_string(),
        }
    }

    pub(super) fn wiz_apply_field(&mut self, which: WizField, path: String) {
        let Some(wiz) = self.wiz.as_mut() else {
            return;
        };
        match which {
            WizField::Parent => wiz.parent = path,
        }
    }

    // ------------------------------------------------------------------
    // Painting
    // ------------------------------------------------------------------
}

/// Input-boundary guard: refuse planning repository names that the
/// production `repository_name_allowed` rule would reject, BEFORE the
/// operator spends a review step (and, in the deferred track, an entire
/// clone) on a doomed name.
fn naming_problem(name: &str) -> Option<String> {
    let unacceptable =
        name.is_empty() || !crate::core::managed_workspace::repository_name_allowed(name);
    unacceptable.then(|| {
        format!(
            "\"{}\" is unusable as a planning repository name: it must start with a letter, fit in 64 characters, use letters, digits, dots, underscores, and dashes only, and not start with a dot. Adjust the planning repository name before continuing.",
            name
        )
    })
}

pub(super) mod painting;

pub(super) use painting::{paint_primary_card, paint_wizard_modal};
