use crate::{
    core::planning_work::WorkKind,
    ui::{ApplicationCommand, Surface, theme},
};
use egui::{RichText, Ui};
use routing::routing_editor;

pub(super) mod git;
mod routing;

#[derive(Clone, Default)]
struct Draft {
    open: bool,
    kind: WorkKind,
    description: String,
    repository_git: std::collections::BTreeMap<String, git::RepositoryDraft>,
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
    let repositories = if open {
        surface.repository_git_options()
    } else {
        Vec::new()
    };
    for repository in &repositories {
        let selected = draft
            .repository_git
            .entry(repository.id.clone())
            .or_insert_with(|| git::RepositoryDraft {
                enabled: repository.id == "root",
                source_branch: repository.default_branch.clone(),
                destination_branch: repository.default_branch.clone(),
                publication_mode: crate::core::git_intent::PublicationModeV1::HoldForReview,
            });
        if !repository.source_branches.contains(&selected.source_branch) {
            selected.source_branch.clear();
        }
        if !repository
            .destination_branches
            .contains(&selected.destination_branch)
        {
            selected.destination_branch.clear();
        }
    }
    let harness_settings = surface.harness_settings();
    if open {
        let requires_git_intent = matches!(
            draft.kind,
            WorkKind::Feature
                | WorkKind::Bug
                | WorkKind::NewProject
                | WorkKind::DocumentationRefresh
        );
        let has_selected_repository = repositories
            .iter()
            .any(|repository| draft.repository_git[&repository.id].enabled);
        let selections_valid = repositories.iter().all(|repository| {
            let selection = &draft.repository_git[&repository.id];
            !selection.enabled
                || (repository.unavailable_reason.is_none()
                    && repository
                        .source_branches
                        .contains(&selection.source_branch)
                    && repository
                        .destination_branches
                        .contains(&selection.destination_branch))
        });
        let valid = !draft.description.trim().is_empty()
            && selections_valid
            && (!requires_git_intent || repositories.is_empty() || has_selected_repository);
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
                    WorkKind::ProjectPlanning => {
                        "Continue the shared project specification conversation."
                    }
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
                    WorkKind::PlanConfirmation => "Kool.ad/e maintains this card automatically.",
                    WorkKind::PlanReviewDisposition => {
                        "Resolve a Manager disposition for an independent plan-review finding."
                    }
                    WorkKind::CodeReviewDisposition => {
                        "Resolve a Manager disposition for an independent code-review finding."
                    }
                    WorkKind::QaFindingDisposition => {
                        "Propose a scoped repair for a persisted runtime QA finding."
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
                if repositories.is_empty() {
                    ui.label(theme::helper_text(
                        "No code repository checkout is available. Planning can continue; choose and approve repository Git intent before task generation.",
                    ));
                } else {
                    git::paint_repository_options(
                        ui,
                        &repositories,
                        &mut draft.repository_git,
                        &Default::default(),
                    );
                }
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
        let git_intents = git::selected_intents(&draft.repository_git);
        created = Some((
            draft.kind,
            draft.description.trim().to_owned(),
            git_intents,
            draft.routing_overrides.clone(),
        ));
    }
    if let Some((kind, description, git_intents, routing_overrides)) = created {
        surface.dispatch(ApplicationCommand::CreatePlanningTask {
            kind,
            description,
            parent_uid: None,
            git_intents,
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
