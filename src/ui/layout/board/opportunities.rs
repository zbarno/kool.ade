use crate::ui::{Surface, planning_board::ViewModel};

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &ViewModel,
    selection: &mut Option<String>,
) {
    paint_settings(ui, surface, board);
    if board.opportunities.is_empty() && board.opportunity_error.is_none() {
        return;
    }
    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let pending = board
        .opportunities
        .iter()
        .filter(|(proposal, _)| is_actionable(proposal, board, now_ms))
        .count();
    ui.collapsing(format!("Improvement proposals ({pending})"), |ui| {
        if let Some(error) = &board.opportunity_error {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("Could not load proposals: {error}"),
            );
        }
        ui.small("Open a proposal to review its evidence, discuss it, or record a decision.");
        for (proposal, _) in &board.opportunities {
            ui.push_id(&proposal.opportunity_uid, |ui| {
                ui.group(|ui| {
                    if ui
                        .selectable_label(
                            false,
                            egui::RichText::new(&proposal.problem_or_opportunity).strong(),
                        )
                        .clicked()
                    {
                        *selection = Some(
                            crate::core::task_conversation::opportunity_conversation_key(
                                &proposal.opportunity_uid,
                            ),
                        );
                    }
                    ui.small(format!(
                        "{} · {}",
                        status_label(proposal, board, now_ms),
                        proposal.user_value
                    ));
                });
            });
        }
    });
}

fn paint_settings(ui: &mut egui::Ui, surface: &mut dyn Surface, board: &ViewModel) {
    ui.collapsing("Opportunity discovery settings", |ui| {
        let mut settings = board.opportunity_discovery_settings.clone();
        let mut changed = false;
        let mut opted_in = settings.exploration
            == crate::core::opportunity::OpportunityExploreModeV1::OptedIn;
        if ui.checkbox(&mut opted_in, "Explore opportunities weekly").changed() {
            settings.exploration = if opted_in {
                crate::core::opportunity::OpportunityExploreModeV1::OptedIn
            } else {
                crate::core::opportunity::OpportunityExploreModeV1::Disabled
            };
            changed = true;
        }
        ui.add_enabled_ui(opted_in, |ui| {
            let mut limit = u32::from(settings.daily_proposal_limit);
            if ui
                .add(egui::Slider::new(&mut limit, 1..=10).text("Daily proposal limit"))
                .changed()
            {
                settings.daily_proposal_limit = limit as u8;
                changed = true;
            }
        });
        let mut active_limit = u32::from(settings.active_self_improvement_limit);
        if ui
            .add(egui::Slider::new(&mut active_limit, 1..=10).text("Active self-improvements"))
            .changed()
        {
            settings.active_self_improvement_limit = active_limit as u8;
            changed = true;
        }
        ui.small("Exploration runs only when this workspace is idle. Proposals remain advisory and require your separate decision.");
        if changed {
            surface.dispatch(crate::ui::ApplicationCommand::SetOpportunityDiscoverySettings {
                expected_revision: settings.revision,
                settings,
            });
        }
    });
}

fn status_label(
    proposal: &crate::core::opportunity::OpportunityV1,
    board: &ViewModel,
    now_ms: u64,
) -> String {
    use crate::core::opportunity::OpportunityStatusV1 as Status;
    if is_actionable(proposal, board, now_ms) {
        return "Needs your decision".into();
    }
    match proposal.status {
        Status::Draft => "Draft".into(),
        Status::Proposed | Status::AwaitingDecision => "Awaiting your decision".into(),
        Status::InformationRequested => "Discussion requested".into(),
        Status::ApprovedToPlan => "Approved to plan".into(),
        Status::Declined => "Declined".into(),
        Status::Deferred => board
            .opportunity_decisions
            .get(&proposal.opportunity_uid)
            .and_then(|decision| decision.snooze_until_ms)
            .and_then(|until| chrono::DateTime::from_timestamp_millis(until as i64))
            .map(|until| format!("Snoozed until {}", until.date_naive()))
            .unwrap_or_else(|| "Snoozed".into()),
        Status::Superseded => "Superseded".into(),
        Status::Planning => "Planning".into(),
        Status::Implemented => "Implemented".into(),
    }
}

fn is_actionable(
    proposal: &crate::core::opportunity::OpportunityV1,
    board: &ViewModel,
    now_ms: u64,
) -> bool {
    use crate::core::opportunity::OpportunityStatusV1 as Status;
    matches!(
        proposal.status,
        Status::Proposed | Status::AwaitingDecision | Status::InformationRequested
    ) || (proposal.status == Status::Deferred
        && board
            .opportunity_decisions
            .get(&proposal.opportunity_uid)
            .is_some_and(|decision| crate::core::opportunity::snooze_expired(decision, now_ms)))
}
