//! Welcome front door.
//!
//! The welcome screen leads with the three journeys operators actually
//! start from — (1) my code already lives on this computer, (2) clone a
//! code repository first, (3) start a blank project — each opening a
//! two-step wizard: gather the few inputs, then REVIEW the exact side
//! effects on disk before anything is written. Everything historical
//! (classic connect, joins, workspace administration) remains reachable
//! under "More connection options".
#![allow(clippy::too_many_arguments)]
use std::cell::Cell;

use super::{KooladeApp, WelcomeEntry, WelcomeWizard, WizField};

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

    /// Discards the active draft. A clone this wizard launched keeps
    /// RUNNING; its pairing intent is dropped so the legacy completion
    /// path governs it.
    pub(super) fn wizard_close(&mut self) {
        self.wiz = None;
        self.pending_clone_pairing = None;
    }

    /// Step 1 → step 2 gate: collects the entry's required input or shows
    /// exactly what is still owed.
    pub(super) fn wizard_continue(&mut self) {
        let (entry, code_path, url, name, parent, plan) = match self.wiz.as_ref() {
            Some(wiz) => (
                wiz.entry,
                wiz.code_path.trim().to_string(),
                wiz.url.trim().to_string(),
                wiz.effective_project_name(),
                wiz.parent.trim().to_string(),
                wiz.packet_name.trim().to_string(),
            ),
            None => return,
        };

        match entry {
            WelcomeEntry::ExistingCode => {
                if code_path.is_empty() {
                    self.browse_wizard_field(WizField::CodePath);
                    return;
                }
                if !std::path::Path::new(&code_path).is_dir() {
                    self.wiz_error(format!("That folder was not found: {code_path}"));
                    return;
                }
                let picked = std::path::Path::new(&code_path);
                let is_root =
                    crate::core::managed_workspace::repository_root(picked).is_ok_and(|root| {
                        if root == picked {
                            return true;
                        }
                        // Tolerate trailing-separator and symlink-form
                        // differences in the typed/picked path.
                        std::fs::canonicalize(root)
                            .ok()
                            .zip(std::fs::canonicalize(picked).ok())
                            .is_some_and(|(resolved_root, resolved_pick)| {
                                resolved_root == resolved_pick
                            })
                    });
                if !is_root {
                    self.wiz_error(format!(
                        "{code_path} is not the root of a Git repository — choose the folder that contains the .git"
                    ));
                    return;
                }
            }
            WelcomeEntry::CloneRepository => {
                if url.is_empty() {
                    self.wiz_error("Paste the repository URL (GitHub, Glab, HTTPS or SSH).");
                    return;
                }
                let segment = match crate::app::welcome::parse_github_url(&url) {
                    Ok(target) => target.repo,
                    Err(detail) => {
                        self.wiz_error(format!("That URL doesn't look cloneable\n{detail}"));
                        return;
                    }
                };
                let candidate = if plan.is_empty() {
                    format!("{segment}.packet")
                } else {
                    plan
                };
                if let Some(problem) = naming_problem(&candidate) {
                    self.wiz_error(problem);
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
                let candidate = if plan.is_empty() {
                    format!("{name}.packet")
                } else {
                    plan
                };
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
        self.conn_browse_target = super::BrowseTarget::Wizard(which);
        self.pick_folder_for_active_target();
    }

    /// Confirm-step authority. Nothing on the welcome surface writes to
    /// disk before this runs, and each branch spells out its effect.
    pub(super) fn wizard_commit(&mut self) {
        let (entry, project, packet, code_path, url, parent) = match self.wiz.as_ref() {
            Some(wiz) => (
                wiz.entry,
                wiz.effective_project_name(),
                wiz.packet_name.trim().to_string(),
                wiz.code_path.trim().to_string(),
                wiz.url.trim().to_string(),
                wiz.parent.trim().to_string(),
            ),
            None => return,
        };

        match entry {
            WelcomeEntry::ExistingCode => {
                let code_dir = std::path::Path::new(code_path.as_str());
                let code_name = match code_dir.file_name().and_then(|n| n.to_str()) {
                    Some(name) if !name.is_empty() => name.to_string(),
                    _ => {
                        self.wiz_error(
                            "Choose the folder that IS the code repository, not its parent.",
                        );
                        return;
                    }
                };
                let planning = if packet.is_empty() {
                    format!("{code_name}.packet")
                } else {
                    packet
                };
                let mut spec = crate::core::managed_workspace::SiblingPairSpec::new(
                    project,
                    code_name.clone(),
                );
                spec.code_already_exists = true;
                spec.planning_repo_name = planning;
                spec.parent_dir = code_dir
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_else(|| std::path::PathBuf::from("/"));
                match crate::app::welcome::attempt_create_project(&spec) {
                    Ok(project) => self.finish_connect(Ok(project)),
                    Err(error) => self.wiz_error(error.to_string()),
                }
            }
            WelcomeEntry::BlankProject => {
                let code_name = Self::project_title_to_folder_name(&project);
                if parent.is_empty() {
                    self.wiz_error("Choose the folder where the project lives.");
                    return;
                }
                let planning = if packet.is_empty() {
                    format!("{code_name}.packet")
                } else {
                    packet
                };
                let mut spec = crate::core::managed_workspace::SiblingPairSpec::new(
                    project,
                    code_name.clone(),
                );
                spec.planning_repo_name = planning;
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
                self.begin_clone(&url);
                if self.clone_job.is_some() {
                    // The worker-completion hook consumes this intent and
                    // pairs the planning sibling BEFORE connecting.
                    self.pending_clone_pairing = Some(super::PendingClonePairing {
                        project_name: project,
                        packet_name: packet,
                    });
                    // Stay on the review step: it shows live progress and
                    // locks the button while the download runs.
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
            WizField::CodePath => wiz.code_path.trim().to_string(),
            WizField::Parent => wiz.parent.trim().to_string(),
        }
    }

    pub(super) fn wiz_apply_field(&mut self, which: WizField, path: String) {
        let Some(wiz) = self.wiz.as_mut() else {
            return;
        };
        match which {
            WizField::CodePath => {
                wiz.code_path = path;
                // Derive a title ONCE so operators don't re-type the obvious.
                if wiz.project_name.trim().is_empty() {
                    wiz.project_name = std::path::Path::new(&wiz.code_path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                }
            }
            WizField::Parent => wiz.parent = path,
        }
    }

    // ------------------------------------------------------------------
    // Painting
    // ------------------------------------------------------------------
}

/// Paints the primary welcome card contents: the ACTIVE wizard when one is
/// open, else the three-journey landing. Actions travel back through the
/// `bus`; text fields edit the draft in place.
/// Input-boundary guard: refuse planning repository names that the
/// production `repository_name_allowed` rule would reject, BEFORE the
/// operator spends a review step (and, in the deferred track, an entire
/// clone) on a doomed name.
fn naming_problem(name: &str) -> Option<String> {
    let unacceptable =
        name.is_empty() || !crate::core::managed_workspace::repository_name_allowed(name);
    unacceptable.then(|| {"\"{name}\" is unusable as a planning repository name: letters, digits, dots, underscores, and dashes only, and no leading dot. Adjust the planning repository name before continuing.".to_string()})
}

pub(super) mod painting;

pub(super) use painting::{paint_primary_card, paint_wizard_modal};
