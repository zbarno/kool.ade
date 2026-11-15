//! Painter for the canonical "Review and approve plan" card.
//!
//! Primary question first: what does the person need to decide or do. The
//! typed decision actions come from `feature_approval` state (never assistant
//! prose); the immutable plan snapshot and technical provenance sit behind
//! progressive disclosure.

mod decision;

use crate::core::planning_work::{PlanConfirmationV1, WorkStatus};
use crate::ui::Surface;

pub(super) fn review_unable_action_for(
    surface: &dyn Surface,
    work: &crate::core::planning_work::Work,
) -> bool {
    work.plan_confirmation
        .as_ref()
        .is_some_and(|payload| decision::review_unable_action(surface, work, payload))
}

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

    let review_unable = work.status == WorkStatus::NeedsAttention
        && decision::review_unable_action(surface, work, payload);
    ui.heading(if review_unable {
        "Plan review is paused".into()
    } else {
        lead_heading(work.status)
    });
    ui.label(theme::helper_text(if review_unable {
        "Your approved plan is unchanged. Task creation is paused until a complete review is recorded."
    } else {
        lead_blurb(work, payload)
    }));
    if !review_unable {
        ui.label(crate::ui::layout::task_details::action::concise_detail(
            &work.detail,
        ));
    }
    ui.add_space(theme::spacing::M);

    match work.status {
        WorkStatus::InReview => {
            decision::paint_decide_area(ui, surface, work, payload);
            decision::paint_snapshot_preview(ui, surface, work);
        }
        WorkStatus::InProgress => {
            ui.label(theme::section_heading("In progress"));
            ui.label(theme::helper_text(
                "Independent plan review is running. Its result lands on this card; nothing else is needed from you meanwhile.",
            ));
        }
        WorkStatus::NeedsAttention => {
            if !decision::paint_review_unable(ui, surface, work, payload) {
                ui.label(theme::section_heading("What you can do"));
                ui.label(theme::helper_text(
                    "Answer the questions below or ask Kool.ad/e to refine the plan. Approval is never assumed while anything is outstanding.",
                ));
                super::decision::free_text(ui, surface, &work.key);
            }
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
