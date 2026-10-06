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
    source_branch: String,
    destination_branch: String,
}

pub(super) fn paint(ui: &mut Ui, surface: &mut dyn Surface) {
    let id = egui::Id::new("koolade_new_task_draft");
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.get_temp::<Draft>(id).unwrap_or_default());
    let mut open = draft.open;
    let mut created = None;
    let mut dismissed = false;
    let branches = surface.repository_branches();
    let destination_branches = surface.repository_destination_branches();
    if draft.source_branch.is_empty() {
        draft.source_branch = surface.default_repository_branch().to_owned();
    }
    if draft.destination_branch.is_empty() {
        draft.destination_branch = surface.default_repository_branch().to_owned();
    }
    if open {
        let closed = crate::ui::overlays::show_modal(ui, true, "New Task", 480.0, |ui| {
            ui.label(RichText::new("What do you want Kool.ad/e to work on?").strong());
            ui.add_space(8.0);
            for (kind, explanation) in [
                (
                    WorkKind::Feature,
                    "Plan a new capability or improve how your project works.",
                ),
                (
                    WorkKind::Bug,
                    "Investigate something that is broken and plan a fix.",
                ),
                (
                    WorkKind::NewProject,
                    "Define a project's purpose, scope, and architecture. Use this to start documenting an existing codebase too.",
                ),
                (
                    WorkKind::DocumentationRefresh,
                    "Survey the code and update project documentation; queue questions and possible issues for review.",
                ),
                (
                    WorkKind::Question,
                    "Get an answer grounded in your project. This task answers questions without creating or updating specifications.",
                ),
            ] {
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(&mut draft.kind, kind, new_task_kind_label(kind));
                    ui.label(RichText::new(explanation).color(theme::TEXT_MUTED));
                });
                ui.add_space(2.0);
            }
            ui.separator();
            ui.label("Describe your goal and any details that will help.");
            ui.add(
                egui::TextEdit::multiline(&mut draft.description)
                    .desired_rows(4)
                    .desired_width(ui.available_width())
                    .hint_text("Describe what you want to do…"),
            );
            ui.horizontal(|ui| {
                branch_picker(ui, "Source Branch", &mut draft.source_branch, &branches);
            });
            ui.horizontal(|ui| {
                branch_picker(
                    ui,
                    "Destination Branch",
                    &mut draft.destination_branch,
                    &destination_branches,
                );
            });
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    dismissed = true;
                }
                if ui
                    .add_enabled(
                        !draft.description.trim().is_empty()
                            && !draft.source_branch.is_empty()
                            && !draft.destination_branch.is_empty()
                            && branches.contains(&draft.source_branch)
                            && destination_branches.contains(&draft.destination_branch)
                            && !surface.conversation_busy(),
                        egui::Button::new("Create Task").fill(theme::ACCENT_SOFT),
                    )
                    .clicked()
                {
                    created = Some((
                        draft.kind,
                        draft.description.trim().to_owned(),
                        draft.source_branch.clone(),
                        draft.destination_branch.clone(),
                    ));
                }
            });
        });
        dismissed |= closed;
    }
    if let Some((kind, description, source_branch, destination_branch)) = created {
        surface.dispatch(ApplicationCommand::CreatePlanningTask {
            kind,
            description,
            parent_uid: None,
            source_branch: Some(source_branch),
            destination_branch: Some(destination_branch),
        });
        draft.description.clear();
        open = false;
    }
    if dismissed {
        open = false;
    }
    draft.open = open;
    ui.ctx().data_mut(|data| data.insert_temp(id, draft));
}

fn branch_picker(ui: &mut Ui, label: &str, selected: &mut String, branches: &[String]) {
    egui::ComboBox::from_id_salt(label)
        .selected_text(if selected.is_empty() {
            format!("{label}: no branch available")
        } else {
            format!("{label}: {selected}")
        })
        .show_ui(ui, |ui| {
            for branch in branches {
                ui.selectable_value(selected, branch.clone(), branch);
            }
        });
}

fn new_task_kind_label(kind: WorkKind) -> &'static str {
    match kind {
        WorkKind::Question => "Question task",
        _ => kind.label(),
    }
}

pub(super) fn trigger(ui: &mut Ui) {
    let id = egui::Id::new("koolade_new_task_draft");
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.get_temp::<Draft>(id).unwrap_or_default());
    let hover_edge = ui.painter().add(egui::Shape::Noop);
    let response = ui.add(
        egui::Button::new(
            RichText::new("+ New Task")
                .size(14.0)
                .strong()
                .color(theme::TEXT),
        )
        .min_size(egui::vec2(112.0, 38.0))
        .stroke(egui::Stroke::new(1.5, theme::BLUE_BRIGHT))
        .fill(theme::BLUE)
        .corner_radius(7),
    );
    ui.painter().set(
        hover_edge,
        if response.hovered() {
            egui::Shape::rect_stroke(
                response.rect.expand(1.0),
                8,
                egui::Stroke::new(2.0, theme::BLUE_BRIGHT),
                egui::StrokeKind::Inside,
            )
        } else {
            egui::Shape::Noop
        },
    );
    if response
        .on_hover_text("Start a feature, bug fix, project, or question")
        .clicked()
    {
        draft.open = true;
    }
    ui.ctx().data_mut(|data| data.insert_temp(id, draft));
}
