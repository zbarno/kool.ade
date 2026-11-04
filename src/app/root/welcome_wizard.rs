//! Welcome front door.
//!
//! The welcome screen offers a reviewed GitHub code-repository clone.
#![allow(clippy::too_many_arguments)]
use std::cell::Cell;

use super::KooladeApp;
mod state;
pub(super) use state::{WelcomeEntry, WelcomeWizard};

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
        let url = match self.wiz.as_ref() {
            Some(wiz) => wiz.url.trim().to_string(),
            None => return,
        };
        if url.is_empty() {
            self.wiz_error("Paste the GitHub URL for the code repository.");
            return;
        }
        if let Err(detail) = crate::app::welcome::parse_github_url(&url) {
            self.wiz_error(format!("That URL doesn't look cloneable\n{detail}"));
            return;
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

    /// Confirm-step authority. Nothing on the welcome surface writes to
    /// disk before this runs, and each branch spells out its effect.
    pub(super) fn wizard_commit(&mut self) {
        let url = match self.wiz.as_ref() {
            Some(wiz) => wiz.url.trim().to_string(),
            None => return,
        };
        if self.clone_job.is_some() {
            self.wiz_error("A clone is already in flight.");
            return;
        }
        self.begin_clone(&url);
        if self.clone_job.is_none() {
            let reason = self.conn_error.clone();
            self.wiz_error(match reason {
                Some(detail) => format!("The clone couldn't start yet:\n{detail}"),
                None => "Couldn't start the clone worker. Try again.".to_string(),
            });
        }
    }

    // ------------------------------------------------------------------
    // ------------------------------------------------------------------
    // Painting
    // ------------------------------------------------------------------
}

pub(super) mod painting;

pub(super) use painting::{paint_primary_card, paint_wizard_modal};
