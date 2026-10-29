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

use egui::{RichText, Ui, WidgetText};

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
        let (entry, code_path, url, name, parent) = match self.wiz.as_ref() {
            Some(wiz) => (
                wiz.entry,
                wiz.code_path.trim().to_string(),
                wiz.url.trim().to_string(),
                wiz.effective_project_name(),
                wiz.parent.trim().to_string(),
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
            }
            WelcomeEntry::CloneRepository => {
                if url.is_empty() {
                    self.wiz_error("Paste the repository URL (GitHub, Glab, HTTPS or SSH).");
                    return;
                }
                if let Err(detail) = crate::app::welcome::parse_github_url(&url) {
                    self.wiz_error(format!("That URL doesn't look cloneable\n{detail}"));
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
                    self.wiz_error("Couldn't start the clone worker. Try again.");
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
pub(super) fn paint_primary_card(
    ui: &mut Ui,
    app: &mut KooladeApp,
    bus: &PrimaryBus,
    cloning: Option<(&str, &str)>,
) {
    if let Some(wiz) = app.wiz.as_mut() {
        paint_wizard_panel(ui, wiz, bus, cloning);
    } else {
        paint_doors(ui, bus);
    }
}

fn paint_doors(ui: &mut Ui, bus: &PrimaryBus) {
    ui.vertical_centered(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new("What are we working on?").strong().size(20.0));
        ui.add_space(4.0);
        ui.label(crate::ui::theme::helper_text(
            "Pick a starting point — each walks you through in two short steps.",
        ));
        ui.add_space(14.0);
        door(
            ui,
            bus,
            "Use code that's already on this computer",
            "Its repository lives on this machine. Kool.ad/e adds the planning repository beside it and opens the board.",
            "Set up planning for my code",
            WelcomeEntry::ExistingCode,
        );
        ui.add_space(10.0);
        door(
            ui,
            bus,
            "Clone a code repository",
            "Hand Kool.ad/e the URL. It clones the code into ~/kool-ade-workspaces, adds the planning pair, and opens the board.",
            "Clone and set up planning",
            WelcomeEntry::CloneRepository,
        );
        ui.add_space(10.0);
        door(
            ui,
            bus,
            "Start a blank project",
            "Nothing exists yet. Create the code and planning repositories from scratch in a folder of your choice.",
            "Create a brand-new project",
            WelcomeEntry::BlankProject,
        );
        ui.add_space(10.0);
    });
}

fn door(
    ui: &mut Ui,
    bus: &PrimaryBus,
    title: &str,
    blurb: &str,
    action: &str,
    entry: WelcomeEntry,
) {
    let width = ui.available_width().min(430.0);
    egui::Frame::new()
        .fill(ui.visuals().panel_fill)
        .corner_radius(10.0)
        .stroke(egui::Stroke::new(1.0, crate::ui::theme::BORDER_STRONG))
        .inner_margin(14.0)
        .show(ui, |ui| {
            ui.set_max_width(width);
            ui.label(RichText::new(title).strong().size(14.0));
            ui.label(crate::ui::theme::helper_text(blurb));
            ui.add_space(4.0);
            if ui.add(egui::Button::new(action)).clicked() {
                bus.door.replace(Some(entry));
            }
        });
}

fn paint_wizard_panel(
    ui: &mut Ui,
    wiz: &mut WelcomeWizard,
    bus: &PrimaryBus,
    cloning: Option<(&str, &str)>,
) {
    let step_no = wiz.step.min(1) + 1;
    let busy = wiz.entry == WelcomeEntry::CloneRepository && cloning.is_some();
    const FIELD_W: f32 = 340.0;

    // NOTE: deliberate plain (non-centered) layout. `vertical_centered`
    // corrupts allocation around `TextEdit` widgets on this egui revision
    // (rows jumped hundreds of pixels), so the panel is top-down/left;
    // the card itself is already centered in its column.
    ui.scope(|ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(wiz.entry.title()).strong().size(18.0));
        ui.label(crate::ui::theme::helper_text(format!(
            "Step {step_no} of 2 — {}",
            if wiz.step == 0 { "details" } else { "review" }
        )));
        ui.add_space(12.0);

        if wiz.step == 0 {
            match wiz.entry {
                WelcomeEntry::ExistingCode => paint_browse_field(
                    ui,
                    bus,
                    WizField::CodePath,
                    &mut wiz.code_path,
                    "Code folder (the folder containing .git)",
                    "Choose code folder…",
                ),
                WelcomeEntry::CloneRepository => {
                    ui.label(crate::ui::theme::helper_text(
                        "Repository URL (GitHub / Glab / HTTPS / SSH)",
                    ));
                    ui.add_space(4.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut wiz.url)
                            .desired_width(FIELD_W)
                            .hint_text("Example: https://github.com/zorgco/payroll-service"),
                    );
                }
                WelcomeEntry::BlankProject => paint_browse_field(
                    ui,
                    bus,
                    WizField::Parent,
                    &mut wiz.parent,
                    "Folder where the project lives",
                    "Choose folder…",
                ),
            }
            ui.add_space(12.0);

            ui.label(crate::ui::theme::helper_text("Project name"));
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::singleline(&mut wiz.project_name)
                    .desired_width(FIELD_W)
                    .hint_text("Shown on the board and stamped on the planning repository"),
            );
            ui.add_space(12.0);

            ui.label(crate::ui::theme::helper_text("Planning repository name"));
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::singleline(&mut wiz.packet_name)
                    .desired_width(FIELD_W)
                    .hint_text("Leave empty for the suggested <name>.packet"),
            );

            if let Some(error) = &wiz.error {
                error_label(ui, error);
            }
            ui.add_space(12.0);
            action_row(ui, bus, busy, WizAction::Continue, "Continue");
        } else {
            ui.label(RichText::new("Here's exactly what will happen:").strong());
            ui.add_space(6.0);
            for line in wiz.disk_plan_lines() {
                ui.label(WidgetText::from(RichText::new(format!("•  {line}")).weak()));
                ui.add_space(2.0);
            }
            ui.add_space(12.0);
            ui.label(crate::ui::theme::helper_text("Planning repository name"));
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::singleline(&mut wiz.packet_name)
                    .desired_width(FIELD_W)
                    .hint_text("Leave empty for the suggested <name>.packet"),
            );
            if let Some((display, repo)) = cloning {
                ui.add_space(8.0);
                ui.label(crate::ui::theme::helper_text(format!(
                    "Downloading {display} ({repo}) — this can take a little while…"
                )));
            }
            if let Some(error) = &wiz.error {
                error_label(ui, error);
            }
            ui.add_space(12.0);
            if busy {
                ui.label(crate::ui::theme::helper_text("Finishing up…"));
            } else {
                action_row(ui, bus, false, WizAction::Commit, "Set up and connect");
            }
        }
    });
}

fn paint_browse_field(
    ui: &mut Ui,
    bus: &PrimaryBus,
    which: WizField,
    value: &mut String,
    caption: &str,
    button: &str,
) {
    ui.label(crate::ui::theme::helper_text(caption));
    ui.add_space(4.0);
    ui.add(egui::TextEdit::singleline(value).desired_width(340.0));
    ui.add_space(8.0);
    ui.vertical_centered(|ui| {
        if value.is_empty() {
            ui.label(crate::ui::theme::helper_text("(not chosen yet)"));
        }
        if ui.button(button).clicked() {
            bus.request_browse(which);
        }
    });
}

fn error_label(ui: &mut Ui, error: &str) {
    ui.add_space(6.0);
    ui.label(WidgetText::from(
        RichText::new(error).color(crate::ui::theme::DANGER),
    ));
}

fn action_row(ui: &mut Ui, bus: &PrimaryBus, busy: bool, primary: WizAction, primary_label: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if ui.add_enabled(!busy, egui::Button::new("Back")).clicked() {
            bus.action.replace(Some(WizAction::Back));
        }
        if ui
            .add_enabled(!busy, egui::Button::new(primary_label))
            .clicked()
        {
            bus.action.replace(Some(primary));
        }
        if ui.add_enabled(!busy, egui::Button::new("Cancel")).clicked() {
            bus.action.replace(Some(WizAction::Close));
        }
    });
}
