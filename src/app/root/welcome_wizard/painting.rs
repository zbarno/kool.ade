//! Painting half of the welcome front door: the three journey doors, the
//! two-step wizard panel, browse fields, error labels, and the action row.
//! Lifecycle and commit authority live in the parent module; this half only
//! paints against the deferred [`PrimaryBus`] cells.
#![allow(clippy::too_many_arguments)]
use egui::{RichText, Ui, WidgetText};

use super::super::{KooladeApp, WelcomeEntry, WelcomeWizard, WizField};
use super::{PrimaryBus, WizAction};

pub(crate) fn paint_primary_card(
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
