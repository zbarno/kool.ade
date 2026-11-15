use super::*;

pub(super) fn paint_approval(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
) {
    let Some(feature_id) = work.feature_id.as_deref() else {
        return;
    };
    let Some(action) = surface.feature_actions(None).into_iter().find(|action| {
        action.id == feature_id
            && action.plan_snapshot.as_ref().is_some_and(|snapshot| {
                crate::ui::feature_approval::belongs_to_root(work, &snapshot.feature_uid)
            })
            && (!action.approved || action.retry_review)
    }) else {
        return;
    };
    egui::CollapsingHeader::new("Plan confirmation")
        .default_open(
            !action.user_confirmed
                || action.retry_review
                || action.review_verdict.as_ref().is_some_and(|verdict| !verdict.pass),
        )
        .show(ui, |ui| {
            crate::ui::feature_approval::paint_candidate(ui, &action);
            if let Some(verdict) = &action.review_verdict {
                crate::ui::feature_approval::paint_review(ui, verdict);
            }
            if !action.ready {
                ui.label("This plan cannot be confirmed yet. Resolve the readiness blockers in planning.");
            } else if action.review_verdict.as_ref().is_some_and(|verdict| {
                verdict.outcome
                    == Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable)
            }) {
                ui.label(if action.retry_review {
                    "The review could not finish. Retry checks the same approved plan and does not change it."
                } else {
                    "The review could not finish, and a retry is not available right now. The plan is unchanged."
                });
            } else if action.review_verdict.as_ref().is_some_and(|verdict| {
                verdict.outcome
                    == Some(crate::harness::prompt_catalog::ReviewOutcomeV1::BlockingFindings)
            }) && !action.retry_review
            {
                ui.label("The independent review found issues. Resolve them before asking for another review.");
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
            } else if action.compare_plans {
                if ui
                    .add_enabled(
                        !surface.conversation_busy(),
                        egui::Button::new("Compare plans").fill(crate::ui::theme::ACTION),
                    )
                    .clicked()
                {
                    surface.dispatch(crate::ui::ApplicationCommand::ApproveFeatureForBoard {
                        id: feature_id.to_owned(),
                    });
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
                                !surface.conversation_busy()
                                    && action.ready
                                    && (action.retry_review
                                        || !action.review_verdict.as_ref().is_some_and(|v| !v.pass)),
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
