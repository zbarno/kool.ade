use crate::core::planning_work::{PlanConfirmationV1, Work};
use crate::ui::{Surface, feature_approval, theme};

pub(super) fn paint_review_unable(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    work: &Work,
    payload: &PlanConfirmationV1,
) -> bool {
    let Some(action) = matching_action(surface, work, payload) else {
        return false;
    };
    let Some(verdict) = action.review_verdict.as_ref().filter(|verdict| {
        verdict.outcome == Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable)
    }) else {
        return false;
    };

    feature_approval::paint_review(ui, verdict);
    ui.label(theme::helper_text(
        "Your approved plan is unchanged. Task creation stays paused until a review is complete.",
    ));
    if action.retry_review && action.user_confirmed && action.ready {
        if ui
            .add_enabled(
                !surface.conversation_busy(),
                egui::Button::new("Retry independent review").fill(theme::ACTION),
            )
            .clicked()
        {
            surface
                .dispatch(crate::ui::ApplicationCommand::ApproveFeatureForBoard { id: action.id });
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("koolade_task_details_close"), true);
            });
        }
        ui.label(theme::metadata_text(
            "This checks the same approved plan again. It does not change the plan or create tasks.",
        ));
    } else {
        ui.label(theme::helper_text(
            "A review retry is not available for this plan right now. The plan remains unchanged and task creation stays paused.",
        ));
    }
    true
}

pub(super) fn review_unable_action(
    surface: &dyn Surface,
    work: &Work,
    payload: &PlanConfirmationV1,
) -> bool {
    matching_action(surface, work, payload)
        .and_then(|action| action.review_verdict)
        .is_some_and(|verdict| {
            verdict.outcome == Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable)
        })
}

pub(super) fn matching_action(
    surface: &dyn Surface,
    work: &Work,
    payload: &PlanConfirmationV1,
) -> Option<feature_approval::Action> {
    let feature_id = work.feature_id.as_deref()?;
    surface.feature_actions(None).into_iter().find(|action| {
        matches_snapshot(action, feature_id, payload) && (!action.approved || action.retry_review)
    })
}

pub(super) fn matches_snapshot(
    action: &feature_approval::Action,
    feature_id: &str,
    payload: &PlanConfirmationV1,
) -> bool {
    action.plan_snapshot.as_ref().is_some_and(|snapshot| {
        snapshot_binding_matches(
            &action.id,
            &snapshot.feature_uid,
            &snapshot.content_hash,
            &snapshot.spec_revision,
            feature_id,
            payload,
        )
    })
}

fn snapshot_binding_matches(
    action_id: &str,
    action_feature_uid: &str,
    action_content_hash: &str,
    action_spec_revision: &str,
    feature_id: &str,
    payload: &PlanConfirmationV1,
) -> bool {
    action_id == feature_id
        && action_feature_uid == payload.feature_uid
        && action_content_hash == payload.snapshot_content_hash
        && action_spec_revision == payload.snapshot_spec_revision
}

pub(super) fn paint_decide_area(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    work: &Work,
    payload: &PlanConfirmationV1,
) {
    let Some(feature_id) = work.feature_id.clone() else {
        ui.label(theme::helper_text(
            "Waiting for the plan document to settle.",
        ));
        return;
    };
    let Some(action) = matching_action(surface, work, payload) else {
        ui.label(theme::helper_text(
            "The plan controls are refreshing. Your saved plan and decision are safe; check back in a moment.",
        ));
        ui.label(theme::metadata_text(format!(
            "Plan version {} · revision {}",
            super::short(&payload.snapshot_content_hash),
            payload.snapshot_spec_revision
        )));
        return;
    };

    theme::card_frame().show(ui, |ui| {
        ui.label(
            egui::RichText::new(
                "Review the plan, then choose one. Your choice is recorded on this card.",
            )
            .strong(),
        );
        feature_approval::paint_candidate(ui, &action);
        if let Some(verdict) = &action.review_verdict {
            feature_approval::paint_review(ui, verdict);
        }
        if !action.ready {
            ui.label(theme::helper_text(
                "The plan is not marked complete yet. Approval is disabled until its open items are resolved.",
            ));
        } else if blocking_findings(&action) && !action.retry_review {
            ui.label(theme::helper_text(
                "The independent review found issues in the plan. Resolve them before asking for another review.",
            ));
        }

        let needs_choice = action
            .plan_comparison
            .as_ref()
            .is_some_and(|comparison| comparison.selected_plan.is_none());
        if needs_choice {
            match feature_approval::paint_comparison(ui, &action) {
                Some(feature_approval::ComparisonIntent::Adopt(plan_id)) => {
                    surface.dispatch(crate::ui::ApplicationCommand::ChooseFeaturePlan {
                        id: feature_id.clone(),
                        plan_id,
                    });
                }
                Some(feature_approval::ComparisonIntent::Discard) => {
                    surface.dispatch(crate::ui::ApplicationCommand::DiscardFeaturePlans {
                        id: feature_id.clone(),
                    });
                }
                None => {}
            }
            ui.separator();
            ui.label(theme::helper_text(
                "If neither plan fits, describe what you need below. Your reply is never treated as approval.",
            ));
            super::super::super::decision::free_text(ui, surface, &work.key);
        } else {
            let blocked_by_review = blocking_findings(&action) && !action.retry_review;
            ui.add_space(theme::spacing::S);
            ui.horizontal_wrapped(|ui| {
                if !action.user_confirmed {
                    let approve = egui::Button::new(
                        egui::RichText::new("Approve plan & continue").strong(),
                    )
                    .fill(theme::ACTION);
                    let response = ui.add_enabled(
                        !surface.conversation_busy() && action.ready && !blocked_by_review,
                        approve.min_size(egui::vec2(230.0, 46.0)),
                    );
                    if response.clicked() {
                        confirm(surface, ui, feature_id.clone());
                    }
                    if !surface.conversation_busy()
                        && !blocked_by_review
                        && let Some(candidate) = action.plan_snapshot.as_ref()
                    {
                        let response = ui.add_enabled(
                            true,
                            egui::Button::new("Request changes")
                                .stroke(egui::Stroke::new(1.5, theme::TEXT_DIM))
                                .min_size(egui::vec2(230.0, 46.0)),
                        );
                        if response.clicked() {
                            surface.dispatch(crate::ui::ApplicationCommand::RequestFeatureChanges {
                                id: feature_id.clone(),
                                candidate_hash: candidate.content_hash.clone(),
                            });
                            close_details(ui);
                        }
                    }
                } else {
                    let proceed = egui::Button::new(egui::RichText::new(action.label()).strong())
                        .fill(theme::ACTION);
                    let response = ui.add_enabled(
                        !surface.conversation_busy()
                            && action.ready
                            && (action.retry_review || !blocking_findings(&action)),
                        proceed.min_size(egui::vec2(230.0, 46.0)),
                    );
                    if response.clicked() {
                        confirm(surface, ui, feature_id.clone());
                    }
                }
            });
            ui.label(theme::metadata_text(
                "Approval authorizes an independent review of this exact plan version. Work continues only after that review passes.",
            ));
        }
    });
}

fn blocking_findings(action: &feature_approval::Action) -> bool {
    action.review_verdict.as_ref().is_some_and(|verdict| {
        verdict.outcome == Some(crate::harness::prompt_catalog::ReviewOutcomeV1::BlockingFindings)
    })
}

fn confirm(surface: &mut dyn Surface, ui: &mut egui::Ui, feature_id: String) {
    surface.dispatch(crate::ui::ApplicationCommand::ApproveFeatureForBoard { id: feature_id });
    close_details(ui);
}

fn close_details(ui: &mut egui::Ui) {
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new("koolade_task_details_close"), true);
    });
}

pub(super) fn paint_snapshot_preview(ui: &mut egui::Ui, surface: &dyn Surface, work: &Work) {
    let Some(feature_id) = work.feature_id.as_deref() else {
        return;
    };
    ui.collapsing("Review the exact plan this card binds to", |ui| {
        match surface
            .active_features()
            .into_iter()
            .find(|(id, _)| *id == feature_id)
        {
            Some((_, body)) => {
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        crate::ui::markdown::paint(ui, body, crate::ui::markdown::CHAT);
                    });
                ui.label(theme::helper_text(
                    "Approval applies to this exact plan version. Editing the plan means you will review and approve the new version again.",
                ));
            }
            None => {
                ui.label(theme::helper_text(
                    "The plan is temporarily unavailable. Its saved version is still here and will be shown again when it reconnects.",
                ));
            }
        }
    });
}

#[cfg(test)]
#[path = "decision/tests.rs"]
mod tests;
