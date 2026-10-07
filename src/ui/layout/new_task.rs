use crate::{
    core::planning_work::WorkKind,
    ui::{ApplicationCommand, Surface, theme},
};
use egui::{RichText, Ui};
use routing::routing_editor;

mod routing;

#[derive(Clone, Default)]
struct Draft {
    open: bool,
    kind: WorkKind,
    description: String,
    source_branch: String,
    destination_branch: String,
    routing_overrides:
        std::collections::BTreeMap<String, crate::persistence::harness_settings::WorkRoute>,
}

pub(super) fn paint(ui: &mut Ui, surface: &mut dyn Surface) {
    let id = egui::Id::new("koolade_new_task_draft");
    let mut draft = ui
        .ctx()
        .data_mut(|data| data.get_temp::<Draft>(id).unwrap_or_default());
    let mut open = draft.open;
    let mut created = None;
    let mut dismissed = false;
    let mut submit = false;
    let branches = surface.repository_branches();
    let destination_branches = surface.repository_destination_branches();
    let harness_settings = surface.harness_settings();
    if draft.source_branch.is_empty() {
        draft.source_branch = surface.default_repository_branch().to_owned();
    }
    if draft.destination_branch.is_empty() {
        draft.destination_branch = surface.default_repository_branch().to_owned();
    }
    if open {
        let valid = !draft.description.trim().is_empty()
            && !draft.source_branch.is_empty()
            && !draft.destination_branch.is_empty()
            && branches.contains(&draft.source_branch)
            && destination_branches.contains(&draft.destination_branch)
            && !surface.conversation_busy();
        let closed = crate::ui::overlays::show_medium_modal(
            ui,
            true,
            "New Task",
            |ui| {
                ui.label(RichText::new("Task type").strong());
                for kind in [
                    WorkKind::Feature,
                    WorkKind::Bug,
                    WorkKind::NewProject,
                    WorkKind::DocumentationRefresh,
                    WorkKind::Question,
                ] {
                    ui.radio_value(&mut draft.kind, kind, new_task_kind_label(kind));
                }
                let selected_help = match draft.kind {
                    WorkKind::Feature => "Plan a capability or improve how the project works.",
                    WorkKind::Bug => "Investigate a problem and plan a fix.",
                    WorkKind::NewProject => {
                        "Define the purpose, scope, and architecture of a project."
                    }
                    WorkKind::DocumentationRefresh => {
                        "Survey the code and refresh project documentation."
                    }
                    WorkKind::Question => "Get an answer grounded in this project.",
                    WorkKind::TaskGeneration => {
                        "Generate implementation tasks from an approved specification."
                    }
                };
                ui.label(theme::helper_text(selected_help));
                ui.add_space(theme::spacing::S);
                ui.label(RichText::new("Goal and details").strong());
                ui.add(
                    egui::TextEdit::multiline(&mut draft.description)
                        .desired_rows(6)
                        .desired_width(ui.available_width())
                        .hint_text("Describe what you want to do…"),
                );
                ui.add_space(theme::spacing::S);
                branch_picker(ui, "Source Branch", &mut draft.source_branch, &branches);
                branch_picker(
                    ui,
                    "Destination Branch",
                    &mut draft.destination_branch,
                    &destination_branches,
                );
                routing_editor(ui, &harness_settings, &mut draft.routing_overrides);
            },
            |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(valid, egui::Button::new("Create Task").fill(theme::ACTION))
                        .clicked()
                    {
                        submit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        dismissed = true;
                    }
                });
            },
        );
        dismissed |= closed;
    }
    if submit {
        created = Some((
            draft.kind,
            draft.description.trim().to_owned(),
            draft.source_branch.clone(),
            draft.destination_branch.clone(),
            draft.routing_overrides.clone(),
        ));
    }
    if let Some((kind, description, source_branch, destination_branch, routing_overrides)) = created
    {
        surface.dispatch(ApplicationCommand::CreatePlanningTask {
            kind,
            description,
            parent_uid: None,
            source_branch: Some(source_branch),
            destination_branch: Some(destination_branch),
            routing_overrides,
        });
        draft.description.clear();
        draft.routing_overrides.clear();
        open = false;
    }
    if dismissed {
        open = false;
    }
    draft.open = open;
    ui.ctx().data_mut(|data| data.insert_temp(id, draft));
}

fn branch_picker(ui: &mut Ui, label: &str, selected: &mut String, branches: &[String]) {
    ui.label(theme::helper_text(label));
    let selected_text = if selected.is_empty() {
        "No branch available"
    } else {
        selected.as_str()
    };
    egui::ComboBox::from_id_salt(label)
        .selected_text(selected_text)
        .width(ui.available_width())
        .wrap_mode(egui::TextWrapMode::Truncate)
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
        .fill(theme::ACTION)
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
