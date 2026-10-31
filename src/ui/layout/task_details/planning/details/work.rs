use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
    planning: &[crate::core::planning_work::Work],
) {
    use crate::core::planning_work::WorkKind;
    ui.heading(&work.title);
    ui.label(super::super::super::action::concise_detail(&work.detail));
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("Next step"));
    if work.kind == WorkKind::TaskGeneration {
        ui.label("The approved specification is ready for task generation.");
        if ui
            .add_enabled(
                !surface.conversation_busy(),
                egui::Button::new("Generate tasks").fill(crate::ui::theme::ACTION),
            )
            .clicked()
            && let Some(feature_id) = work.feature_id.as_ref()
        {
            surface.dispatch(crate::ui::ApplicationCommand::GenerateTasksForFeature {
                work_key: work.key.clone(),
                feature_id: feature_id.clone(),
            });
        }
    } else if work.status == crate::core::planning_work::WorkStatus::InProgress {
        ui.label("Kool.ad/e is working on this task.");
    } else if matches!(
        work.status,
        crate::core::planning_work::WorkStatus::NeedsAttention
            | crate::core::planning_work::WorkStatus::InReview
    ) {
        super::decision::free_text(ui, surface, &work.key);
    }
    if let Some(parent) = crate::ui::layout::planning_parent_label(
        work,
        planning.iter().collect::<Vec<_>>().as_slice(),
    ) {
        ui.label(crate::ui::theme::helper_text(parent));
    }
    ui.collapsing("Why am I seeing this?", |ui| {
        ui.label(crate::ui::theme::metadata_text(format!(
            "{} · {:?}",
            work.kind.label(),
            work.status
        )));
        ui.label(&work.detail);
    });
    paint_approval(ui, surface, work);
    if let Some(offer) = &work.follow_up_task {
        ui.collapsing("Related task", |ui| {
            ui.label(&offer.description);
            if ui.button("Create related Feature task").clicked() {
                surface.dispatch(crate::ui::ApplicationCommand::CreatePlanningTask {
                    kind: WorkKind::Feature,
                    description: offer.description.clone(),
                    parent_uid: Some(work.uid.clone()),
                    source_branch: None,
                    destination_branch: None,
                    routing_overrides: Default::default(),
                });
            }
        });
    }
    paint_controls(ui, surface, work);
}

fn paint_approval(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
) {
    let Some(feature_id) = work.feature_id.as_deref() else {
        return;
    };
    let Some(action) = surface
        .feature_actions(None)
        .into_iter()
        .find(|action| action.id == feature_id && (!action.approved || action.retry_review))
    else {
        return;
    };
    egui::CollapsingHeader::new("Plan confirmation")
        .default_open(!action.user_confirmed || action.retry_review)
        .show(ui, |ui| {
            crate::ui::feature_approval::paint_candidate(ui, &action);
            if let Some(verdict) = &action.review_verdict {
                crate::ui::feature_approval::paint_review(ui, verdict);
            }
            let needs_choice = action
                .plan_comparison
                .as_ref()
                .is_some_and(|comparison| comparison.selected_plan.is_none());
            if needs_choice {
                match crate::ui::feature_approval::paint_comparison(ui, &action) {
                    Some(crate::ui::feature_approval::ComparisonIntent::Adopt(plan_id)) => {
                        surface.dispatch(crate::ui::ApplicationCommand::ChooseFeaturePlan {
                            id: feature_id.to_owned(),
                            plan_id,
                        });
                    }
                    Some(crate::ui::feature_approval::ComparisonIntent::Discard) => {
                        surface.dispatch(crate::ui::ApplicationCommand::DiscardFeaturePlans {
                            id: feature_id.to_owned(),
                        });
                    }
                    None => {}
                }
            } else {
                ui.horizontal_wrapped(|ui| {
                    if !action.user_confirmed
                        && ui
                            .add_enabled(
                                !surface.conversation_busy(),
                                egui::Button::new("Confirm this plan")
                                    .fill(crate::ui::theme::ACTION),
                            )
                            .clicked()
                    {
                        surface.dispatch(crate::ui::ApplicationCommand::ApproveFeatureForBoard {
                            id: feature_id.to_owned(),
                        });
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(egui::Id::new("koolade_task_details_close"), true);
                        });
                    }
                    if action.user_confirmed
                        && ui
                            .add_enabled(
                                !surface.conversation_busy() && action.ready,
                                egui::Button::new(action.label()).fill(crate::ui::theme::ACTION),
                            )
                            .clicked()
                    {
                        surface.dispatch(crate::ui::ApplicationCommand::ApproveFeatureForBoard {
                            id: feature_id.to_owned(),
                        });
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(egui::Id::new("koolade_task_details_close"), true);
                        });
                    }
                    if !action.user_confirmed
                        && !surface.conversation_busy()
                        && let Some(crate::ui::feature_approval::ApprovalIntent::RequestChanges(
                            id,
                            candidate_hash,
                        )) = crate::ui::feature_approval::paint_request_changes(ui, &action)
                    {
                        surface.dispatch(crate::ui::ApplicationCommand::RequestFeatureChanges {
                            id,
                            candidate_hash,
                        });
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(egui::Id::new("koolade_task_details_close"), true);
                        });
                    }
                });
            }
        });
}

fn paint_controls(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
) {
    ui.horizontal(|ui| {
        if work.status != crate::core::planning_work::WorkStatus::Done
            && ui.button("Cancel").clicked()
        {
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("koolade_cancel_pending"), work.key.clone())
            });
        }
        if work.status == crate::core::planning_work::WorkStatus::Done
            && ui.button("Archive").clicked()
        {
            surface.dispatch(crate::ui::ApplicationCommand::ArchiveTask {
                ticket: work.key.clone(),
            });
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("koolade_task_details_close"), true);
            });
        }
    });
    let cancel_id = egui::Id::new("koolade_cancel_pending");
    let confirming = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(cancel_id))
        .as_deref()
        == Some(work.key.as_str());
    if confirming {
        ui.group(|ui| {
            ui.label(crate::ui::theme::section_heading("Cancel this task?"));
            ui.label(crate::ui::theme::helper_text(
                "The task will leave the queue. Its files, conversation, and planning history will be kept.",
            ));
            ui.horizontal(|ui| {
                if ui.button("Keep working").clicked() {
                    ui.ctx().data_mut(|data| data.remove::<String>(cancel_id));
                }
                if ui
                    .button(egui::RichText::new("Confirm cancel").color(crate::ui::theme::DANGER))
                    .clicked()
                {
                    surface.dispatch(crate::ui::ApplicationCommand::CancelWork {
                        key: work.key.clone(),
                    });
                    ui.ctx().data_mut(|data| data.remove::<String>(cancel_id));
                }
            });
        });
    }
}
