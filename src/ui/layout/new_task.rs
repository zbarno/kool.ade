use crate::{
    core::planning_work::WorkKind,
    ui::{ApplicationCommand, Surface, theme},
};
use egui::{RichText, Ui};

#[derive(Clone, Default)]
struct Draft {
    open: bool,
    kind: WorkKind,
    description: String,
}

pub(super) fn paint(ui: &mut Ui, surface: &mut dyn Surface) {
    let id = egui::Id::new("packet_new_task_draft");
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.get_temp::<Draft>(id).unwrap_or_default());
    ui.horizontal(|ui| {
        if ui
            .add(egui::Button::new(RichText::new("+ New Task").strong()).fill(theme::ACCENT_SOFT))
            .clicked()
        {
            draft.open = true;
        }
        ui.label(
            RichText::new("Start a feature, bug fix, project, or question.")
                .small()
                .weak(),
        );
    });

    let mut open = draft.open;
    let mut created = None;
    let mut dismissed = false;
    if open {
        egui::Window::new("New Task")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ui.ctx(), |ui| {
                ui.label(RichText::new("What do you want Packet to work on?").strong());
                ui.horizontal_wrapped(|ui| {
                    for kind in [
                        WorkKind::Feature,
                        WorkKind::Bug,
                        WorkKind::NewProject,
                        WorkKind::Question,
                    ] {
                        ui.selectable_value(&mut draft.kind, kind, kind.label());
                    }
                });
                ui.add(
                    egui::TextEdit::multiline(&mut draft.description)
                        .desired_rows(4)
                        .desired_width(460.0)
                        .hint_text("Describe what you want to do…"),
                );
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        dismissed = true;
                    }
                    if ui
                        .add_enabled(
                            !draft.description.trim().is_empty() && !surface.conversation_busy(),
                            egui::Button::new("Create Task").fill(theme::ACCENT_SOFT),
                        )
                        .clicked()
                    {
                        created = Some((draft.kind, draft.description.trim().to_owned()));
                    }
                });
            });
    }
    if dismissed {
        open = false;
    }
    if let Some((kind, description)) = created {
        surface.dispatch(ApplicationCommand::CreatePlanningTask {
            kind,
            description,
            parent_uid: None,
        });
        draft.description.clear();
        open = false;
    }
    draft.open = open;
    ui.ctx().data_mut(|data| data.insert_temp(id, draft));
}
