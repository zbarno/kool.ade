use super::*;
mod decision;
mod work;

pub(super) enum PlanningDetails<'a> {
    Item(
        &'a crate::domain::item::OpenItem,
        &'a [crate::domain::item::OpenItem],
    ),
    Work(
        &'a crate::core::planning_work::Work,
        &'a [crate::core::planning_work::Work],
    ),
}

pub(super) fn paint_panes(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    key: &str,
    height: f32,
    details: PlanningDetails<'_>,
) {
    if ui.available_width() >= 1000.0 {
        ui.columns(2, |panes| {
            egui::ScrollArea::vertical()
                .id_salt(("planning_conversation", key))
                .max_height(height)
                .show(&mut panes[0], |ui| {
                    super::conversation::paint(ui, surface, key)
                });
            egui::ScrollArea::vertical()
                .id_salt(("planning_details", key))
                .max_height(height)
                .show(&mut panes[1], |ui| paint_details(ui, surface, details));
        });
    } else {
        paint_details(ui, surface, details);
        ui.add_space(crate::ui::theme::spacing::L);
        ui.separator();
        super::conversation::paint(ui, surface, key);
    }
}

fn paint_details(ui: &mut egui::Ui, surface: &mut dyn Surface, details: PlanningDetails<'_>) {
    match details {
        PlanningDetails::Item(item, items) => paint_item_details(ui, surface, item, items),
        PlanningDetails::Work(work, planning) => work::paint(ui, surface, work, planning),
    }
}

fn paint_item_details(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    item: &crate::domain::item::OpenItem,
    items: &[crate::domain::item::OpenItem],
) {
    ui.heading(&item.question);
    let column = crate::ui::layout::planning_column(item, items);
    ui.label(crate::ui::theme::metadata_text(format!(
        "{} · {} · {}",
        item.kind,
        item.category,
        crate::core::implementation::BOARD_COLUMNS[column]
    )));
    let authority = match item.authority {
        crate::domain::Authority::Human => "Waiting for your decision",
        crate::domain::Authority::Agent => "Kool.ad/e is investigating",
        crate::domain::Authority::Review => "Ready for your review",
    };
    ui.label(crate::ui::theme::helper_text(authority));
    ui.add_space(crate::ui::theme::spacing::M);
    decision::paint(ui, surface, item);
    ui.label(crate::ui::theme::section_heading("Next step"));
    if item.status == crate::domain::item::ItemStatus::Resolved {
        ui.label("This item is resolved.");
    } else if item.is_ownership_gap() {
        ui.label("Assign an owner to clear this item.");
        if ui.button("Manage stakeholders").clicked() {
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("koolade_workspace_settings_open"), true);
                data.insert_temp(
                    egui::Id::new("koolade_settings_open_page"),
                    "People & Stakeholders".to_owned(),
                );
            });
        }
    } else if item.authority == crate::domain::Authority::Agent {
        ui.label("Kool.ad/e is investigating this item.");
    } else if item.authority == crate::domain::Authority::Review
        && item.feature_id.is_some()
        && !item.recommendation.is_empty()
        && !crate::core::routing::has_open_prerequisite(items, item)
    {
        ui.label("Review the recommendation, then approve or reply in the conversation.");
        if ui
            .add_enabled(
                !surface.task_chat_active(item.conversation_key()),
                egui::Button::new("Approve provisional decision").fill(crate::ui::theme::ACTION),
            )
            .clicked()
        {
            surface.dispatch(crate::ui::ApplicationCommand::ApproveReviewItem {
                id: item.id.clone(),
            });
        }
    } else {
        ui.label("Continue this task in the conversation pane.");
    }
    ui.add_space(crate::ui::theme::spacing::M);
    if !item.reason.trim().is_empty() {
        ui.label(crate::ui::theme::section_heading("Context"));
        ui.label(&item.reason);
    }
    if !item.recommendation.is_empty() {
        ui.collapsing("Recommendation", |ui| ui.label(&item.recommendation));
    } else if let Some(recommendation) = item
        .decision_brief
        .as_ref()
        .and_then(|brief| brief.recommendation.as_ref())
    {
        ui.collapsing("Recommendation", |ui| {
            ui.label(&recommendation.rationale);
        });
    }
    if !item.evidence.is_empty() {
        ui.collapsing("Evidence", |ui| ui.label(&item.evidence));
    } else if let Some(evidence) = item
        .decision_brief
        .as_ref()
        .filter(|brief| !brief.evidence.is_empty())
    {
        ui.collapsing("Evidence", |ui| {
            for item in &evidence.evidence {
                ui.label(item);
            }
        });
    }
    if item.status == crate::domain::item::ItemStatus::Resolved && ui.button("Archive").clicked() {
        surface.dispatch(crate::ui::ApplicationCommand::ArchiveTask {
            ticket: item.id.clone(),
        });
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_task_details_close"), true);
        });
    }
    ui.collapsing("Activity", |ui| {
        crate::ui::task_activity::graph(
            ui,
            &surface.activity_samples(Some(item.conversation_key())),
            surface.activity_active(item.conversation_key()),
            48.0,
        );
        if let Some(progress) = surface.task_progress(&item.id) {
            if item.authority == crate::domain::Authority::Agent {
                ui.label(crate::ui::theme::section_heading("Agent investigation"));
            }
            if let Some(activity) = &progress.activity {
                ui.label(activity);
            }
            if !progress.thoughts.trim().is_empty() {
                ui.label(crate::ui::theme::section_heading("Worker notes"));
                ui.label(crate::ui::theme::helper_text(&progress.thoughts));
            }
            if !progress.response.trim().is_empty() {
                ui.label(crate::ui::theme::section_heading("Latest result"));
                crate::ui::markdown::paint(
                    ui,
                    &crate::core::context_build::clip(&progress.response, 4000),
                    crate::ui::markdown::CHAT,
                );
            }
        }
    });
}
