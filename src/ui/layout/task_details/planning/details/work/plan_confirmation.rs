//! Painter for the canonical "Review and approve plan" card.
//!
//! Primary question first: what does the person need to decide or do. The
//! typed decision actions come from `feature_approval` state (never assistant
//! prose); the immutable plan snapshot and technical provenance sit behind
//! progressive disclosure.

use crate::core::planning_work::{PlanConfirmationV1, WorkStatus};
use crate::ui::Surface;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
    planning: &[crate::core::planning_work::Work],
) -> bool {
    use crate::ui::theme;
    let Some(payload) = work.plan_confirmation.as_ref() else {
        return false;
    };
    let _ = planning;

    ui.heading(lead_heading(work.status));
    ui.label(theme::helper_text(lead_blurb(work, payload)));
    ui.label(crate::ui::layout::task_details::action::concise_detail(
        &work.detail,
    ));
    ui.add_space(theme::spacing::M);

    match work.status {
        WorkStatus::InReview => {
            paint_decide_area(ui, surface, work, payload);
            paint_snapshot_preview(ui, surface, work);
        }
        WorkStatus::InProgress => {
            ui.label(theme::section_heading("In progress"));
            ui.label(theme::helper_text(
                "Independent plan review is running. Its result lands on this card; nothing else is needed from you meanwhile.",
            ));
        }
        WorkStatus::NeedsAttention => {
            ui.label(theme::section_heading("What you can do"));
            ui.label(theme::helper_text(
                "Answer the questions below or ask Kool.ad/e to refine the plan. Approval is never assumed while anything is outstanding.",
            ));
            super::decision::free_text(ui, surface, &work.key);
        }
        WorkStatus::Done => {
            ui.label(theme::section_heading("Outcome"));
            ui.label(theme::helper_text(
                "This card reached its terminal state. The typed approval receipt, readiness assessment, and review verdict remain auditable on this card’s TaskIntent.",
            ));
        }
        WorkStatus::Todo => {}
    }

    ui.add_space(theme::spacing::S);
    ui.label(theme::metadata_text(format!(
        "{} · plan fingerprint {}",
        work.kind.label(),
        short(&payload.snapshot_content_hash)
    )));
    ui.collapsing("Technical provenance (advanced)", |ui| {
        for line in technical_lines(payload) {
            ui.label(line);
        }
    });
    true
}

fn short(hash: &str) -> String {
    hash.chars().take(12).collect()
}

fn lead_heading(status: WorkStatus) -> String {
    match status {
        WorkStatus::InReview => "A decision is ready when you are".into(),
        WorkStatus::NeedsAttention => "This plan needs input before approval".into(),
        WorkStatus::InProgress => "Kool.ad/e is finishing the independent review".into(),
        WorkStatus::Done => "Plan review complete".into(),
        WorkStatus::Todo => "Awaiting review preparation".into(),
    }
}

fn lead_blurb(
    work: &crate::core::planning_work::Work,
    payload: &PlanConfirmationV1,
) -> &'static str {
    match work.status {
        WorkStatus::InReview if payload.ready => {
            "The plan is complete and its snapshot is shown below. Approve to authorize independent review and continued work, or request changes — neither happens on your behalf."
        }
        WorkStatus::InReview => {
            "The plan changed since your last decision. Review the new version and decide again."
        }
        WorkStatus::NeedsAttention => {
            "Unresolved items are listed above. Nothing is approved until you confirm the plan yourself."
        }
        WorkStatus::InProgress => {
            "Your approval is recorded. Waiting on the independent plan review."
        }
        WorkStatus::Done => "See the outcome below.",
        WorkStatus::Todo => "Preparing the plan for review.",
    }
}

fn technical_lines(payload: &PlanConfirmationV1) -> Vec<String> {
    vec![
        format!("Feature binding: {}", payload.feature_uid),
        format!("Snapshot content hash: {}", payload.snapshot_content_hash),
        format!("Specification revision: {}", payload.snapshot_spec_revision),
        format!(
            "Published document: {}",
            if payload.pin_is_published { "yes" } else { "still an editable draft" }
        ),
        format!(
            "Readiness: {} ({})",
            payload.ready,
            match &payload.cause {
                None => "assessed".to_owned(),
                Some(crate::core::planning_work::PlanNotReadyCause::AssessmentNeeded) =>
                    "awaiting a readiness assessment".to_owned(),
                Some(crate::core::planning_work::PlanNotReadyCause::RequirementsOutstanding) =>
                    "outstanding requirements remain".to_owned(),
                Some(crate::core::planning_work::PlanNotReadyCause::MalformedDocument) =>
                    "the plan document could not be parsed".to_owned(),
            }
        ),
        "Receipts: user confirmation, readiness assessment, and review verdict are read from durable typed state — never inferred from chat text.".to_owned(),
    ]
}

fn paint_decide_area(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    work: &crate::core::planning_work::Work,
    _payload: &PlanConfirmationV1,
) {
    use crate::ui::feature_approval;
    use crate::ui::theme;
    let Some(feature_id) = work.feature_id.clone() else {
        ui.label(theme::helper_text(
            "Waiting for the plan document to settle.",
        ));
        return;
    };
    let Some(action) = surface.feature_actions(None).into_iter().find(|action| {
        action.id == feature_id
            && action.plan_snapshot.as_ref().is_some_and(|snapshot| {
                feature_approval::belongs_to_root(work, &snapshot.feature_uid)
            })
            && (!action.approved || action.retry_review)
    }) else {
        // Not an anomaly to shout about: the action set refreshes on the
        // next reconciliation pass.
        ui.label(theme::helper_text(
            "The decision controls are regenerating from the stored plan.",
        ));
        return;
    };

    theme::card_frame().show(ui, |ui| {
        ui.label(
            egui::RichText::new(
                "Review the plan, then choose one. Your choice is recorded as a typed receipt on this card.",
            )
            .strong(),
        );
        feature_approval::paint_candidate(ui, &action);
        if let Some(verdict) = &action.review_verdict {
            feature_approval::paint_review(ui, verdict);
        }
        if !action.ready {
            ui.label(theme::helper_text(
                "The plan is not flagged ready yet; approving is disabled until readiness evidence exists.",
            ));
        } else if action
            .review_verdict
            .as_ref()
            .is_some_and(|verdict| !verdict.pass)
            && !action.retry_review
        {
            ui.label(theme::helper_text(
                "Independent review found issues. Address them and re-request review before approving.",
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
                        id: feature_id,
                        plan_id,
                    });
                }
                Some(feature_approval::ComparisonIntent::Discard) => {
                    surface.dispatch(crate::ui::ApplicationCommand::DiscardFeaturePlans {
                        id: feature_id,
                    });
                }
                None => {}
            }
        } else {
            let blocked_by_review = action
                .review_verdict
                .as_ref()
                .is_some_and(|verdict| !verdict.pass)
                && !action.retry_review;
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
                        surface.dispatch(crate::ui::ApplicationCommand::ApproveFeatureForBoard {
                            id: feature_id.clone(),
                        });
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(egui::Id::new("koolade_task_details_close"), true);
                        });
                    }
                    if !surface.conversation_busy()
                        && !blocked_by_review
                        && let Some(candidate) = action.plan_snapshot.as_ref()
                    {
                        let response = ui.add_enabled(
                            true,
                            egui::Button::new(egui::RichText::new("Request changes").strong())
                                .stroke(egui::Stroke::new(1.5, theme::TEXT_DIM))
                                .min_size(egui::vec2(230.0, 46.0)),
                        );
                        if response.clicked() {
                            surface.dispatch(crate::ui::ApplicationCommand::RequestFeatureChanges {
                                id: feature_id.clone(),
                                candidate_hash: candidate.content_hash.clone(),
                            });
                            ui.ctx().data_mut(|data| {
                                data.insert_temp(
                                    egui::Id::new("koolade_task_details_close"),
                                    true,
                                );
                            });
                        }
                    }
                } else {
                    let proceed = egui::Button::new(egui::RichText::new(action.label()).strong())
                        .fill(theme::ACTION);
                    let response = ui.add_enabled(
                        !surface.conversation_busy()
                            && action.ready
                            && (action.retry_review
                                || !action
                                    .review_verdict
                                    .as_ref()
                                    .is_some_and(|v| !v.pass)),
                        proceed.min_size(egui::vec2(230.0, 46.0)),
                    );
                    if response.clicked() {
                        surface.dispatch(crate::ui::ApplicationCommand::ApproveFeatureForBoard {
                            id: feature_id.clone(),
                        });
                        ui.ctx().data_mut(|data| {
                            data.insert_temp(egui::Id::new("koolade_task_details_close"), true);
                        });
                    }
                }
            });
            ui.label(theme::metadata_text(
                "Approving authorizes independent review of this exact plan version, then continued work. It never bypasses review.",
            ));
        }
    });
}

fn paint_snapshot_preview(
    ui: &mut egui::Ui,
    surface: &dyn Surface,
    work: &crate::core::planning_work::Work,
) {
    use crate::ui::theme;
    let Some(feature_id) = work.feature_id.clone() else {
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
                    "Approval binds to this exact document version. Editing the plan voids the approval until you re-approve.",
                ));
            }
            None => {
                ui.label(theme::helper_text(
                    "The plan document is unavailable right now; the snapshot binding is preserved for the next pass.",
                ));
            }
        }
    });
}
