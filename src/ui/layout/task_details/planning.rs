use super::*;

mod clarification;
mod conversation;
mod details;
mod opportunity;

pub(super) fn paint_clarification(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    question: &crate::core::planning::QuestionV1,
    height: f32,
) {
    clarification::paint(ui, surface, question, height);
}

pub(super) fn paint_item(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    item: &crate::domain::item::OpenItem,
    height: f32,
) {
    let key = item.conversation_key();
    details::paint_panes(
        ui,
        surface,
        key,
        height,
        details::PlanningDetails::Item(item, &board.planning_items),
    );
}

pub(super) fn paint_work(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    work: &crate::core::planning_work::Work,
    height: f32,
) {
    let key = work.key.as_str();
    details::paint_panes(
        ui,
        surface,
        key,
        height,
        details::PlanningDetails::Work(work, &board.planning_work),
    );
}

pub(super) fn paint_opportunity(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    proposal: &crate::core::opportunity::OpportunityV1,
    record_revision: u64,
    height: f32,
) {
    opportunity::paint(ui, surface, board, proposal, record_revision, height);
}

pub(super) fn paint_machine_setup(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    task: &crate::persistence::setup_tasks::LocalTask,
) {
    ui.heading(&task.title);
    ui.label(crate::ui::theme::metadata_text(format!(
        "Machine setup · {:?}",
        task.state
    )));
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading(
        "Why this needs attention",
    ));
    ui.label(&task.explanation);
    if !task.prerequisites.is_empty() {
        ui.label(crate::ui::theme::section_heading("Waiting for"));
        for item in &task.prerequisites {
            ui.label(item);
        }
    }
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("What to do next"));
    ui.label(&task.next_step);
    ui.label(crate::ui::theme::helper_text(
        "Kool.ad/e stores setup state on this machine, not in the project Git repository.         Automatic repairs and dedicated task conversations are being implemented separately.",
    ));
    if ui.button("Open coding tools").clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_open_coding_settings"), true);
        });
    }
    if ui
        .add_enabled(
            !board.setup_checking,
            egui::Button::new(if board.setup_checking {
                "Checking setup…"
            } else {
                "Check machine again"
            }),
        )
        .clicked()
    {
        surface.dispatch(crate::ui::ApplicationCommand::RetrySetupCheck);
    }
}

pub(super) fn paint_setup_issue(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    issue: &crate::app::setup_attention::SetupIssue,
) {
    ui.heading(issue.title);
    ui.label(crate::ui::theme::metadata_text("Setup · Needs your input"));
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("What needs attention"));
    ui.label(&issue.issue);
    ui.collapsing("Why this matters", |ui| ui.label(issue.why));
    ui.label(crate::ui::theme::section_heading("Recommended next step"));
    ui.label(issue.recommendation);
    ui.label(crate::ui::theme::helper_text(issue.impact));
    ui.label(issue.next_action);
    if ui.button("Open workspace settings").clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_workspace_settings_open"), true);
        });
    }
    let label = if board.setup_checking {
        "Checking setup…"
    } else {
        "Retry setup check"
    };
    if ui
        .add_enabled(
            !board.setup_checking,
            egui::Button::new(label).fill(crate::ui::theme::ACTION),
        )
        .clicked()
    {
        surface.dispatch(crate::ui::ApplicationCommand::RetrySetupCheck);
    }
}

pub(super) fn paint_feature_approval(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    action: &crate::ui::feature_approval::Action,
) {
    ui.heading(&action.id);
    ui.label(crate::ui::theme::metadata_text(
        "Specification · Needs approval",
    ));
    ui.add_space(crate::ui::theme::spacing::M);
    crate::ui::feature_approval::paint_candidate(ui, action);
    if action
        .plan_comparison
        .as_ref()
        .is_some_and(|comparison| comparison.selected_plan.is_none())
    {
        match crate::ui::feature_approval::paint_comparison(ui, action) {
            Some(crate::ui::feature_approval::ComparisonIntent::Adopt(plan_id)) => surface
                .dispatch(crate::ui::ApplicationCommand::ChooseFeaturePlan {
                    id: action.id.clone(),
                    plan_id,
                }),
            Some(crate::ui::feature_approval::ComparisonIntent::Discard) => {
                surface.dispatch(crate::ui::ApplicationCommand::DiscardFeaturePlans {
                    id: action.id.clone(),
                })
            }
            None => {}
        }
    } else {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !surface.conversation_busy(),
                    egui::Button::new(action.label()).fill(crate::ui::theme::ACTION),
                )
                .clicked()
            {
                surface.dispatch(crate::ui::ApplicationCommand::ApproveFeatureForBoard {
                    id: action.id.clone(),
                });
                ui.ctx().data_mut(|data| {
                    data.insert_temp(egui::Id::new("koolade_task_details_close"), true);
                });
            }
            if !surface.conversation_busy()
                && let Some(crate::ui::feature_approval::ApprovalIntent::RequestChanges(
                    id,
                    candidate_hash,
                )) = crate::ui::feature_approval::paint_request_changes(ui, action)
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
}

pub(super) fn paint_plan_review(
    ui: &mut egui::Ui,
    review: &crate::ui::task_detail::LifecyclePlanReviewView,
) {
    let snapshot = &review.snapshot;
    ui.heading("Independent Plan Review");
    ui.label(crate::ui::theme::metadata_text(format!(
        "Feature {} · plan revision {}",
        review.feature_uid, snapshot.spec_revision
    )));
    ui.label(crate::ui::theme::section_heading("Confirmed plan"));
    ui.label(&snapshot.intent);
    ui.label(egui::RichText::new("Expected outcome").strong());
    ui.label(&snapshot.outcome);
    ui.columns(2, |columns| {
        columns[0].label(egui::RichText::new("Current behavior").strong());
        columns[0].label(&snapshot.current_state);
        columns[1].label(egui::RichText::new("Proposed behavior").strong());
        columns[1].label(&snapshot.target_behavior);
    });
    plan_list(ui, "Included scope", &snapshot.scope);
    plan_list(ui, "Explicit exclusions", &snapshot.non_goals);
    plan_list(ui, "Acceptance criteria", &snapshot.acceptance_criteria);
    plan_list(ui, "Risks", &snapshot.risks);
    plan_list(
        ui,
        "Test strategy",
        std::slice::from_ref(&snapshot.test_strategy),
    );
    ui.label(crate::ui::theme::helper_text(format!(
        "Confirmed plan hash · {}",
        snapshot.content_hash
    )));
    ui.label(crate::ui::theme::section_heading("Review evidence"));
    ui.label(format!(
        "User confirmation · {}",
        review
            .confirmation
            .as_ref()
            .map_or("not recorded", |receipt| receipt.subject_sha256.as_str())
    ));
    if let Some(receipt) = &review.review {
        ui.label(format!(
            "Independent review receipt · {}",
            receipt.receipt_uid
        ));
    } else {
        ui.label(crate::ui::theme::helper_text(
            "The confirmed plan is awaiting an independent reviewer.",
        ));
    }
    if let Some(verdict) = &review.verdict {
        let (label, color) = if verdict.pass {
            ("Review passed", crate::ui::theme::SUCCESS)
        } else {
            ("Review findings", crate::ui::theme::WARNING)
        };
        ui.colored_label(color, label);
        for coverage in &verdict.coverage {
            ui.label(format!(
                "{} · {:?} · {}",
                coverage.requirement,
                coverage.status,
                coverage.evidence.join(", ")
            ));
        }
        for finding in &verdict.findings {
            ui.group(|ui| {
                ui.label(format!(
                    "{} · {:?} · {:?}",
                    finding.id, finding.severity, finding.kind
                ));
                ui.label(&finding.requirement);
                ui.label(&finding.proposed_correction);
                ui.label(crate::ui::theme::helper_text(format!(
                    "{} · evidence: {}",
                    finding.location,
                    finding.evidence.join(", ")
                )));
            });
        }
        plan_list(ui, "Not covered yet", &verdict.untested);
    }
}

fn plan_list(ui: &mut egui::Ui, title: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    ui.collapsing(title, |ui| {
        for value in values {
            ui.label(format!("• {value}"));
        }
    });
}

#[cfg(test)]
#[path = "planning/plan_review_tests.rs"]
mod plan_review_tests;
