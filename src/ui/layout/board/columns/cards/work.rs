use super::super::super::super::task_cards;
use super::super::super::super::*;

pub(in crate::ui::layout::board::columns) fn work(
    ui: &mut egui::Ui,
    s: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
    planning: &[&crate::core::planning_work::Work],
    column: usize,
) {
    let kind = (work.kind == crate::core::planning_work::WorkKind::Question)
        .then_some(crate::domain::ItemKind::Question);
    let active = s.active_planning_work() == Some(work.key.as_str());
    task_cards::board_card(
        ui,
        &work.key,
        kind,
        active || work.status == crate::core::planning_work::WorkStatus::InProgress,
        column == 3,
        column == 4,
        |ui| {
            if column == 3 {
                super::super::attention::badge(ui, super::super::attention::Kind::WaitingOnUser);
                super::super::attention::user_action(ui, "Open the conversation to continue");
            }
            if ui
                .add(
                    egui::Button::new(RichText::new(&work.title).size(15.5).strong())
                        .frame(false)
                        .wrap(),
                )
                .clicked()
            {
                let mut tabs = ui
                    .ctx()
                    .data_mut(|data| data.get_temp::<ChatTabs>(egui::Id::new("koolade_chat_tabs")))
                    .unwrap_or_default();
                tabs.open(&work.key);
                ui.ctx()
                    .data_mut(|data| data.insert_temp(egui::Id::new("koolade_chat_tabs"), tabs));
            }
            ui.label(
                RichText::new(work.kind.label())
                    .size(11.0)
                    .color(theme::board_hue(kind)),
            );
            if active && matches!(work.kind, crate::core::planning_work::WorkKind::Bug) {
                ui.label(
                    RichText::new("Triage, then planning…")
                        .size(12.0)
                        .color(theme::BLUE_BRIGHT),
                );
            } else if active
                && matches!(
                    work.kind,
                    crate::core::planning_work::WorkKind::Feature
                        | crate::core::planning_work::WorkKind::NewProject
                )
            {
                ui.label(
                    RichText::new("Building context and planning…")
                        .size(12.0)
                        .color(theme::BLUE_BRIGHT),
                );
            }
            if active {
                ui.horizontal(|ui| {
                    theme::operation_indicator(ui);
                    ui.label(
                        RichText::new("Kool.ad/e is working on this task")
                            .size(12.0)
                            .strong()
                            .color(theme::BLUE_BRIGHT),
                    );
                });
                if let Some(activity) = s
                    .live_progress()
                    .and_then(|progress| progress.activity.as_deref())
                {
                    ui.label(RichText::new(activity).size(12.0).color(theme::TEXT_DIM));
                }
            }
            if let Some((_, specification)) = work.feature_id.as_deref().and_then(|id| {
                s.active_features()
                    .into_iter()
                    .find(|(feature_id, _)| *feature_id == id)
            }) {
                ui.collapsing("Open specification", |ui| {
                    crate::ui::markdown::paint(ui, specification, crate::ui::markdown::CHAT);
                });
            }
            if let Some(feature_id) = work.feature_id.as_deref()
                && let Some(action) = s
                    .feature_actions(None)
                    .into_iter()
                    .find(|action| action.id == feature_id && !action.approved)
            {
                if action
                    .plan_comparison
                    .as_ref()
                    .is_some_and(|comparison| comparison.selected_plan.is_none())
                {
                    match crate::ui::feature_approval::paint_comparison(ui, &action) {
                        Some(crate::ui::feature_approval::ComparisonIntent::Adopt(plan_id)) => {
                            s.dispatch(ApplicationCommand::ChooseFeaturePlan {
                                id: feature_id.to_owned(),
                                plan_id,
                            });
                        }
                        Some(crate::ui::feature_approval::ComparisonIntent::Discard) => {
                            s.dispatch(ApplicationCommand::DiscardFeaturePlans {
                                id: feature_id.to_owned(),
                            });
                        }
                        None => {}
                    }
                } else if ui
                    .add_enabled(!s.conversation_busy(), egui::Button::new(action.label()))
                    .clicked()
                {
                    s.dispatch(ApplicationCommand::ApproveFeatureForBoard {
                        id: feature_id.to_owned(),
                    });
                }
            } else if let Some(feature_id) = work.feature_id.as_deref()
                && s.feature_approved(feature_id)
            {
                ui.label(RichText::new("Specification approved").color(theme::SUCCESS));
            }
            if work.kind == crate::core::planning_work::WorkKind::TaskGeneration {
                ui.label("The approved specification is ready for task generation.");
                if ui
                    .add_enabled(!s.conversation_busy(), egui::Button::new("Generate tasks"))
                    .clicked()
                    && let Some(feature_id) = work.feature_id.clone()
                {
                    s.dispatch(ApplicationCommand::GenerateTasksForFeature {
                        work_key: work.key.clone(),
                        feature_id,
                    });
                }
            }
            if let Some(parent) = planning_parent_label(work, planning) {
                ui.label(RichText::new(parent).color(theme::WARNING));
            }
            ui.label(RichText::new(crate::core::context_build::clip(&work.detail, 180)).size(13.0));
            if let Some(offer) = &work.follow_up_task {
                ui.label(RichText::new(&offer.title).strong());
                if ui.button("Create related Feature task").clicked() {
                    s.dispatch(ApplicationCommand::CreatePlanningTask {
                        kind: crate::core::planning_work::WorkKind::Feature,
                        description: offer.description.clone(),
                        parent_uid: Some(work.uid.clone()),
                        source_branch: None,
                        destination_branch: None,
                        routing_overrides: Default::default(),
                    });
                }
            }
            if ui.small_button("Open conversation").clicked() {
                let mut tabs = ui
                    .ctx()
                    .data_mut(|data| data.get_temp::<ChatTabs>(egui::Id::new("koolade_chat_tabs")))
                    .unwrap_or_default();
                tabs.open(&work.key);
                ui.ctx()
                    .data_mut(|data| data.insert_temp(egui::Id::new("koolade_chat_tabs"), tabs));
            }
            if column == 4 && ui.small_button("Archive").clicked() {
                s.dispatch(ApplicationCommand::ArchiveTask {
                    ticket: work.key.clone(),
                });
            }
            let cancellable = column != 4
                || (work.kind == crate::core::planning_work::WorkKind::Feature
                    && work.feature_id.is_some());
            if cancellable && ui.small_button("Cancel").clicked() {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new("koolade_cancel_pending"), work.key.clone())
                });
            }
            ui.label(
                RichText::new(&work.key)
                    .size(11.0)
                    .color(theme::TEXT_MUTED)
                    .monospace(),
            );
        },
    );
}
