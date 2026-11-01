//! Painting half of the welcome front door: the two journey doors, the
//! two-step wizard panel, browse fields, error labels, and the action row.
//! Lifecycle and commit authority live in the parent module; this half only
//! paints against the deferred [`PrimaryBus`] cells.
#![allow(clippy::too_many_arguments)]
use egui::{RichText, Ui, WidgetText};

use super::super::{KooladeApp, WelcomeEntry, WelcomeWizard, WizField};
use super::{PrimaryBus, WizAction};

/// The primary card is permanently the two journey doors. An ACTIVE
/// wizard no longer replaces this canvas — it floats above it as a modal
/// dialog ([`paint_wizard_modal`]), so the welcome screen stays visible
/// (and interaction-locked) beneath the dialog.
pub(crate) fn paint_primary_card(
    ui: &mut Ui,
    _app: &mut KooladeApp,
    bus: &PrimaryBus,
    _cloning: Option<(&str, &str)>,
) {
    paint_doors(ui, bus);
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
            "Clone a code repository and its planning repository",
            "Paste the code repository's GitHub URL. Kool.ad/e clones it into its managed folder, then opens owner/repository.packet or creates it with matching visibility.",
            "Clone and open project",
            WelcomeEntry::CloneRepository,
        );
        ui.add_space(10.0);
        door(
            ui,
            bus,
            "Start a blank project",
            "Nothing exists yet. Kool.ad/e creates the code repository where you choose and keeps planning in its managed workspace area.",
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
            let response = ui.add(egui::Button::new(action));
            if entry == WelcomeEntry::BlankProject {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("koolade_welcome_blank_project_button_rect"),
                        response.rect,
                    );
                });
            }
            if response.clicked() {
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
                WelcomeEntry::CloneRepository => {
                    ui.label(crate::ui::theme::helper_text("Code repository GitHub URL"));
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

            ui.label(crate::ui::theme::helper_text(match wiz.entry {
                WelcomeEntry::CloneRepository => "Name shown in Kool.ad/e (optional)",
                WelcomeEntry::BlankProject => "Project name",
            }));
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::singleline(&mut wiz.project_name)
                    .desired_width(FIELD_W)
                    .hint_text(match wiz.entry {
                        WelcomeEntry::CloneRepository => "Defaults to the repository name",
                        WelcomeEntry::BlankProject => "Shown on the board and planning repository",
                    }),
            );
            ui.add_space(12.0);

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
                action_row(ui, bus, false, WizAction::Commit, "Clone and open project");
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
/// Renders the active wizard as a modal dialog over the whole welcome
/// canvas. The shared modal machinery ([`crate::ui::overlays::show_modal`])
/// supplies the backdrop, input blocking, drag-to-move, close glyph, and
/// `Esc` handling; this adapter feeds in the two-step wizard panel and
/// mirrors the card-era deferred-cell bus. Chrome close (X, `Esc`, or
/// backdrop) retires the wizard — EXCEPT while a clone download is in
/// flight: the in-panel buttons are disabled for that phase for the same
/// reason, so a stray `Esc` mid-download cannot strand a partially paired
/// workspace.
pub(crate) fn paint_wizard_modal(ui: &mut Ui, app: &mut KooladeApp, cloning: Option<(&str, &str)>) {
    if app.wiz.is_none() {
        return;
    }
    let title = app
        .wiz
        .as_ref()
        .expect("checked above")
        .entry
        .title()
        .to_string();
    let door_req = std::cell::Cell::new(None::<WelcomeEntry>);
    let action_req = std::cell::Cell::new(None::<WizAction>);
    let browse_req = std::cell::Cell::new(None::<WizField>);
    let bus = PrimaryBus {
        door: &door_req,
        action: &action_req,
        browse: &browse_req,
    };
    let clone_busy = app.clone_job.is_some();
    let closed = crate::ui::overlays::show_modal(ui, !clone_busy, &title, 560.0, |ui| {
        if let Some(wiz) = app.wiz.as_mut() {
            paint_wizard_panel(ui, wiz, &bus, cloning);
        }
    });
    let _ = door_req.take();
    if let Some(action) = action_req.take() {
        app.wizard_actuate(action);
    }
    if let Some(which) = browse_req.take() {
        app.browse_wizard_field(which);
    }
    let click_position = ui.ctx().input(|input| input.pointer.interact_pos());
    let blank_project_button = ui.ctx().data(|data| {
        data.get_temp::<egui::Rect>(egui::Id::new("koolade_welcome_blank_project_button_rect"))
    });
    let background_door_clicked = click_position
        .zip(blank_project_button)
        .is_some_and(|(position, rect)| rect.contains(position));
    // Chrome dismissal is a draft discard, but a running clone owns the
    // modal until the code and planning repositories are paired.
    if closed && !clone_busy && !background_door_clicked {
        app.wizard_close();
    }
}
