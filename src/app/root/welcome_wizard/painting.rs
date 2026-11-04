//! Painting half of the welcome front door: the code clone door, the
//! two-step wizard panel, error labels, and the action row.
//! Lifecycle and commit authority live in the parent module; this half only
//! paints against the deferred [`PrimaryBus`] cells.
#![allow(clippy::too_many_arguments)]
use egui::{RichText, Ui, WidgetText};

use super::super::{KooladeApp, WelcomeEntry, WelcomeWizard};
use super::{PrimaryBus, WizAction};

/// The primary card is permanently the code-repository clone door. An ACTIVE
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
            "Clone a code repository",
            "Paste a GitHub repository URL to clone and open the code checkout.",
            "Clone repository",
            WelcomeEntry::CloneRepository,
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
            ui.label(crate::ui::theme::helper_text("Code repository GitHub URL"));
            ui.add_space(4.0);
            ui.add(
                egui::TextEdit::singleline(&mut wiz.url)
                    .desired_width(FIELD_W)
                    .hint_text("Example: https://github.com/zorgco/payroll-service"),
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
                action_row(
                    ui,
                    bus,
                    false,
                    WizAction::Commit,
                    "Clone and open repository",
                );
            }
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
/// reason, so a stray `Esc` mid-download cannot strand a partial clone.
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
    let bus = PrimaryBus {
        door: &door_req,
        action: &action_req,
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
    // Chrome dismissal is a draft discard, but a running clone owns the
    // modal until the code checkout is ready.
    if closed && !clone_busy {
        app.wizard_close();
    }
}
