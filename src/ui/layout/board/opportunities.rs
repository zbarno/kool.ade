use crate::{
    harness::prompt_catalog::OpportunityChoice,
    ui::{ApplicationCommand, Surface, planning_board::ViewModel},
};

pub(super) fn paint(ui: &mut egui::Ui, surface: &mut dyn Surface, board: &ViewModel) {
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
        ui.small("Exploration runs only when this workspace is idle. Proposals remain advisory and require your separate decision.");
        if changed {
            surface.dispatch(ApplicationCommand::SetOpportunityDiscoverySettings {
                expected_revision: settings.revision,
                settings,
            });
        }
    });
    let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let pending = board
        .opportunities
        .iter()
        .filter(|(proposal, _)| is_actionable(proposal, board, now_ms))
        .collect::<Vec<_>>();
    let pursued = board
        .opportunities
        .iter()
        .filter(|(proposal, _)| {
            matches!(
                &proposal.status,
                crate::core::opportunity::OpportunityStatusV1::ApprovedToPlan
                    | crate::core::opportunity::OpportunityStatusV1::Planning
            )
        })
        .collect::<Vec<_>>();
    let resolved = board
        .opportunities
        .iter()
        .filter(|(proposal, _)| {
            !is_actionable(proposal, board, now_ms)
                && !matches!(
                    &proposal.status,
                    crate::core::opportunity::OpportunityStatusV1::ApprovedToPlan
                        | crate::core::opportunity::OpportunityStatusV1::Planning
                )
        })
        .collect::<Vec<_>>();
    if pending.is_empty()
        && pursued.is_empty()
        && resolved.is_empty()
        && board.opportunity_error.is_none()
    {
        return;
    }
    ui.collapsing(format!("Improvement proposals ({})", pending.len()), |ui| {
        if let Some(error) = &board.opportunity_error {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("Could not load proposals: {error}"),
            );
        }
        for (proposal, revision) in pending {
            ui.group(|ui| {
                ui.strong(&proposal.problem_or_opportunity);
                if proposal.status == crate::core::opportunity::OpportunityStatusV1::Deferred {
                    ui.small(
                        "The snooze ended. Choose whether to pursue, snooze again, or decline.",
                    );
                }
                ui.label(format!("Why it matters: {}", proposal.user_value));
                ui.label(format!("Expected outcome: {}", proposal.expected_outcome));
                ui.label(format!(
                    "Current support: {}",
                    proposal.current_capability_state
                ));
                if let Some(workaround) = &proposal.generic_workaround {
                    ui.label(format!("Current workaround: {workaround}"));
                }
                ui.label(format!(
                    "Proposed scope: {:?} · impact: {}",
                    proposal.estimated_scope, proposal.impact
                ));
                ui.label(format!("Approach: {}", proposal.recommended_approach));
                ui.label(format!("Risks and cost: {}", proposal.cost_or_risk));
                if !proposal.alternatives.is_empty() {
                    ui.small(format!(
                        "Alternatives: {}",
                        proposal.alternatives.join("; ")
                    ));
                }
                ui.small(format!(
                    "Target: {} · source {} · evidence: {}",
                    proposal.target.canonical_remote,
                    proposal.target.source_sha,
                    proposal
                        .evidence_refs
                        .iter()
                        .map(|item| item.evidence_uid.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                if proposal.status
                    == crate::core::opportunity::OpportunityStatusV1::InformationRequested
                    && let Some(question) = board
                        .opportunity_decisions
                        .get(&proposal.opportunity_uid)
                        .and_then(|decision| decision.explanation.as_deref())
                {
                    ui.label(format!("Question sent to Man.ager: {question}"));
                }
                ui.horizontal_wrapped(|ui| {
                    for (label, choice) in [
                        ("Pursue", OpportunityChoice::Pursue),
                        ("Not now · 7 days", OpportunityChoice::NotNow),
                        ("Decline", OpportunityChoice::Decline),
                    ] {
                        if ui.button(label).clicked() {
                            surface.dispatch(ApplicationCommand::DecideOpportunity {
                                uid: proposal.opportunity_uid.clone(),
                                expected_revision: *revision,
                                choice,
                                explanation: None,
                            });
                        }
                    }
                    ui.menu_button("Snooze", |ui| {
                        for days in [1, 7, 30] {
                            if ui.button(format!("{days} days")).clicked() {
                                surface.dispatch(ApplicationCommand::SnoozeOpportunity {
                                    uid: proposal.opportunity_uid.clone(),
                                    expected_revision: *revision,
                                    days,
                                });
                                ui.close();
                            }
                        }
                    });
                    let question_id = ui
                        .make_persistent_id(("opportunity-discussion", &proposal.opportunity_uid));
                    if ui.button("Discuss").clicked() {
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(question_id, true));
                    }
                });
                let question_id =
                    ui.make_persistent_id(("opportunity-discussion", &proposal.opportunity_uid));
                if ui
                    .ctx()
                    .data(|data| data.get_temp::<bool>(question_id).unwrap_or(false))
                {
                    let input_id =
                        ui.make_persistent_id(("opportunity-question", &proposal.opportunity_uid));
                    let mut question = ui
                        .ctx()
                        .data(|data| data.get_temp::<String>(input_id).unwrap_or_default());
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut question)
                                .hint_text("What should Man.ager clarify?"),
                        );
                        let enabled = !question.trim().is_empty();
                        if ui
                            .add_enabled(enabled, egui::Button::new("Send question"))
                            .clicked()
                        {
                            surface.dispatch(ApplicationCommand::DecideOpportunity {
                                uid: proposal.opportunity_uid.clone(),
                                expected_revision: *revision,
                                choice: OpportunityChoice::AskForMoreInformation,
                                explanation: Some(question.trim().to_owned()),
                            });
                            ui.ctx().data_mut(|data| {
                                data.remove::<bool>(question_id);
                                data.remove::<String>(input_id);
                            });
                        }
                    });
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(input_id, question));
                }
            });
        }
        for (proposal, revision) in pursued {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{:?}: {}",
                    proposal.status, proposal.problem_or_opportunity
                ));
                if let Some(run) = board.self_improvement_runs.get(&proposal.opportunity_uid) {
                    ui.label(format!("Self-improvement: {:?}", run.phase));
                    if let Some(url) = &run.pr_url {
                        ui.hyperlink_to("Pull request", url);
                    }
                    if run.phase
                        == crate::core::self_improvement::SelfImprovementPhaseV1::MergedNotActivated
                    {
                        ui.colored_label(
                            egui::Color32::YELLOW,
                            "Merged; awaiting authorized activation and readiness probe",
                        );
                    } else if run.phase
                        == crate::core::self_improvement::SelfImprovementPhaseV1::Activated
                    {
                        ui.label(format!(
                            "Activated version: {}",
                            run.activated_version.as_deref().unwrap_or("unknown")
                        ));
                    }
                } else if proposal.status
                    == crate::core::opportunity::OpportunityStatusV1::ApprovedToPlan
                    && ui.button("Create planning goal").clicked()
                {
                    surface.dispatch(ApplicationCommand::RetryOpportunityPlanning {
                        uid: proposal.opportunity_uid.clone(),
                        expected_revision: *revision,
                    });
                }
            });
        }
        for (proposal, _) in resolved {
            ui.horizontal(|ui| {
                let snooze = board
                    .opportunity_decisions
                    .get(&proposal.opportunity_uid)
                    .and_then(|decision| decision.snooze_until_ms);
                let label = snooze
                    .filter(|_| {
                        proposal.status == crate::core::opportunity::OpportunityStatusV1::Deferred
                    })
                    .map_or_else(
                        || format!("{:?}: {}", proposal.status, proposal.problem_or_opportunity),
                        |until| {
                            format!(
                                "Snoozed until {}: {}",
                                chrono::DateTime::from_timestamp_millis(until as i64)
                                    .map(|time| time.to_rfc3339())
                                    .unwrap_or_else(|| "unknown time".into()),
                                proposal.problem_or_opportunity
                            )
                        },
                    );
                ui.label(label);
            });
        }
    });
}

fn is_actionable(
    proposal: &crate::core::opportunity::OpportunityV1,
    board: &ViewModel,
    now_ms: u64,
) -> bool {
    matches!(
        proposal.status,
        crate::core::opportunity::OpportunityStatusV1::Proposed
            | crate::core::opportunity::OpportunityStatusV1::AwaitingDecision
            | crate::core::opportunity::OpportunityStatusV1::InformationRequested
    ) || (proposal.status == crate::core::opportunity::OpportunityStatusV1::Deferred
        && board
            .opportunity_decisions
            .get(&proposal.opportunity_uid)
            .is_some_and(|decision| crate::core::opportunity::snooze_expired(decision, now_ms)))
}
