use crate::ui::{ApplicationCommand, Surface, planning_board::ViewModel, theme};
use egui::Ui;

mod evidence;
mod self_improvement;

pub(super) fn paint(
    ui: &mut Ui,
    surface: &mut dyn Surface,
    board: &ViewModel,
    proposal: &crate::core::opportunity::OpportunityV1,
    record_revision: u64,
    height: f32,
) {
    let key =
        crate::core::task_conversation::opportunity_conversation_key(&proposal.opportunity_uid);
    let focus_id = discussion_focus_id(&key);
    let focus_discussion = ui.ctx().data_mut(|data| {
        let focus = data.get_temp::<bool>(focus_id).unwrap_or(false);
        data.remove::<bool>(focus_id);
        focus
    });
    let decision = board.opportunity_decisions.get(&proposal.opportunity_uid);
    let run = board.self_improvement_runs.get(&proposal.opportunity_uid);
    if ui.available_width() >= 1000.0 {
        ui.columns(2, |panes| {
            egui::ScrollArea::vertical()
                .id_salt(("opportunity_conversation", &key))
                .max_height(height)
                .show(&mut panes[0], |ui| {
                    super::conversation::paint_with_focus(ui, surface, &key, focus_discussion)
                });
            egui::ScrollArea::vertical()
                .id_salt(("opportunity_details", &key))
                .max_height(height)
                .show(&mut panes[1], |ui| {
                    details(ui, surface, proposal, record_revision, decision, run, &key)
                });
        });
    } else {
        details(ui, surface, proposal, record_revision, decision, run, &key);
        ui.add_space(theme::spacing::L);
        ui.separator();
        super::conversation::paint_with_focus(ui, surface, &key, focus_discussion);
    }
}

fn details(
    ui: &mut Ui,
    surface: &mut dyn Surface,
    proposal: &crate::core::opportunity::OpportunityV1,
    record_revision: u64,
    decision: Option<&crate::core::opportunity::OpportunityDecisionV1>,
    run: Option<&crate::core::self_improvement::SelfImprovementRunV1>,
    conversation_key: &str,
) {
    use crate::core::opportunity::OpportunityStatusV1 as Status;

    ui.heading("Task details & state");
    ui.heading(&proposal.problem_or_opportunity);
    ui.label(theme::metadata_text(format!(
        "Improvement Proposal · {:?} · {}",
        proposal.origin,
        status_label(&proposal.status)
    )));
    ui.add_space(theme::spacing::M);
    ui.label(theme::section_heading("Grounded trigger"));
    ui.label(&proposal.problem_or_opportunity);
    ui.label(theme::helper_text(format!(
        "Detected from {:?} evidence",
        proposal.origin
    )));
    ui.label(theme::section_heading("Expected benefit"));
    ui.label(&proposal.user_value);
    ui.label(theme::section_heading("Expected outcome"));
    ui.label(&proposal.expected_outcome);

    ui.collapsing("Current state and workaround", |ui| {
        ui.label(&proposal.current_capability_state);
        if let Some(workaround) = &proposal.generic_workaround {
            ui.label(theme::section_heading("Current workaround"));
            ui.label(workaround);
        }
    });
    ui.collapsing("Scope, approach, and alternatives", |ui| {
        ui.label(format!("Estimated scope: {:?}", proposal.estimated_scope));
        ui.label(theme::section_heading("Recommended approach"));
        ui.label(&proposal.recommended_approach);
        if !proposal.possible_approaches.is_empty() {
            ui.label(theme::section_heading("Possible approaches"));
            for approach in &proposal.possible_approaches {
                ui.label(format!("• {approach}"));
            }
        }
        if !proposal.alternatives.is_empty() {
            ui.label(theme::section_heading("Alternatives"));
            for alternative in &proposal.alternatives {
                ui.label(format!("• {alternative}"));
            }
        }
        ui.label(theme::section_heading("Affected systems"));
        ui.label(proposal.affected_systems.join(", "));
    });
    ui.collapsing("Risks and impact", |ui| {
        ui.label(theme::section_heading("Expected impact"));
        ui.label(&proposal.impact);
        ui.label(theme::section_heading("Cost and risk"));
        ui.label(&proposal.cost_or_risk);
    });
    evidence::paint(ui, proposal);

    ui.label(theme::section_heading("Decision and next step"));
    if let Some(decision) = decision
        && let Some(explanation) = decision.explanation.as_deref()
    {
        ui.label(match decision.decision {
            crate::core::opportunity::OpportunityDecisionKindV1::AskForMoreInformation => {
                "Question sent to Man.ager"
            }
            _ => "Recorded decision note",
        });
        ui.label(explanation);
    }
    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let snooze_expired =
        decision.is_some_and(|decision| crate::core::opportunity::snooze_expired(decision, now_ms));
    match proposal.status {
        Status::Proposed | Status::AwaitingDecision | Status::InformationRequested => {
            let can_decide =
                !surface.conversation_busy() && !surface.task_chat_active(conversation_key);
            decision_actions(
                ui,
                surface,
                proposal,
                record_revision,
                conversation_key,
                can_decide,
            );
        }
        Status::Deferred if snooze_expired => {
            ui.label("The snooze ended. Choose whether to pursue, defer, decline, or discuss this proposal.");
            let can_decide =
                !surface.conversation_busy() && !surface.task_chat_active(conversation_key);
            decision_actions(
                ui,
                surface,
                proposal,
                record_revision,
                conversation_key,
                can_decide,
            );
        }
        Status::Deferred => {
            let snooze_until = decision
                .and_then(|decision| decision.snooze_until_ms)
                .and_then(|until| chrono::DateTime::from_timestamp_millis(until as i64));
            ui.label(snooze_until.map_or_else(
                || "This proposal is snoozed.".to_owned(),
                |until| format!("Snoozed until {}.", until.date_naive()),
            ));
        }
        Status::ApprovedToPlan => {
            if run.is_none()
                && ui
                    .add_enabled(
                        !surface.conversation_busy(),
                        egui::Button::new("Retry planning goal").fill(theme::ACTION),
                    )
                    .clicked()
            {
                surface.dispatch(ApplicationCommand::RetryOpportunityPlanning {
                    uid: proposal.opportunity_uid.clone(),
                    expected_revision: record_revision,
                });
            }
        }
        Status::Draft
        | Status::Superseded
        | Status::Planning
        | Status::Implemented
        | Status::Declined => {
            ui.label("No decision is pending for this proposal.");
        }
    }
    if let Some(run) = run {
        self_improvement::paint_progress(ui, surface, run);
    }
}

fn decision_actions(
    ui: &mut Ui,
    surface: &mut dyn Surface,
    proposal: &crate::core::opportunity::OpportunityV1,
    record_revision: u64,
    conversation_key: &str,
    can_decide: bool,
) {
    ui.label("Discussion is advisory. Pursue starts the existing planning and approval lifecycle.");
    ui.horizontal_wrapped(|ui| {
        for (label, choice) in [
            (
                "Pursue",
                crate::harness::prompt_catalog::OpportunityChoice::Pursue,
            ),
            (
                "Not now · 7 days",
                crate::harness::prompt_catalog::OpportunityChoice::NotNow,
            ),
            (
                "Decline",
                crate::harness::prompt_catalog::OpportunityChoice::Decline,
            ),
        ] {
            if ui
                .add_enabled(can_decide, egui::Button::new(label))
                .clicked()
            {
                surface.dispatch(ApplicationCommand::DecideOpportunity {
                    uid: proposal.opportunity_uid.clone(),
                    expected_revision: record_revision,
                    choice,
                    explanation: None,
                });
            }
        }
        if ui
            .add_enabled(can_decide, egui::Button::new("Discuss"))
            .clicked()
        {
            ui.ctx().data_mut(|data| {
                data.insert_temp(discussion_focus_id(conversation_key), true);
            });
        }
        ui.add_enabled_ui(can_decide, |ui| {
            ui.menu_button("Snooze", |ui| {
                for days in [1, 7, 30] {
                    if ui.button(format!("{days} days")).clicked() {
                        surface.dispatch(ApplicationCommand::SnoozeOpportunity {
                            uid: proposal.opportunity_uid.clone(),
                            expected_revision: record_revision,
                            days,
                        });
                        ui.close();
                    }
                }
            });
        });
    });
}

fn discussion_focus_id(conversation_key: &str) -> egui::Id {
    egui::Id::new(("opportunity_discussion_focus", conversation_key))
}

fn status_label(status: &crate::core::opportunity::OpportunityStatusV1) -> &'static str {
    use crate::core::opportunity::OpportunityStatusV1 as Status;
    match status {
        Status::Draft => "Draft",
        Status::Proposed | Status::AwaitingDecision => "Needs your decision",
        Status::InformationRequested => "Discussion requested",
        Status::ApprovedToPlan => "Approved to plan",
        Status::Declined => "Declined",
        Status::Deferred => "Snoozed",
        Status::Superseded => "Superseded",
        Status::Planning => "Planning",
        Status::Implemented => "Implemented",
    }
}
