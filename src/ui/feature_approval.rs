//! Actions come from application state, never from parsing an assistant's prose.

#[derive(Clone, Debug)]
pub struct Action {
    pub id: String,
    pub specification: String,
    pub approved: bool,
    pub user_confirmed: bool,
    pub ready: bool,
    pub plan_snapshot: Option<crate::core::lifecycle::PlanSnapshotV1>,
    pub review_verdict: Option<crate::harness::prompt_catalog::ReviewVerdictV1>,
    pub prepare_tasks: bool,
    pub retry_review: bool,
    pub compare_plans: bool,
    pub plan_comparison: Option<crate::domain::PlanComparison>,
}

pub fn paint_candidate(ui: &mut egui::Ui, action: &Action) {
    confirmation::paint(ui, action, "Plan confirmation");
}

pub fn paint_review(ui: &mut egui::Ui, verdict: &crate::harness::prompt_catalog::ReviewVerdictV1) {
    confirmation::paint_review(ui, verdict);
}
impl Action {
    /// A confirmation card belongs to one immutable candidate, so a revised
    /// plan gets a new board identity even when its feature ID is unchanged.
    pub fn board_key(&self) -> String {
        self.plan_snapshot.as_ref().map_or_else(
            || format!("plan-confirmation:{}:unavailable", self.id),
            |snapshot| {
                format!(
                    "plan-confirmation:{}:{}",
                    snapshot.feature_uid, snapshot.content_hash
                )
            },
        )
    }
}

#[path = "feature_approval/confirmation.rs"]
mod confirmation;

pub enum ComparisonIntent {
    Adopt(String),
    Discard,
}

pub enum ApprovalIntent {
    Confirm(String, bool),
    RequestChanges(String, String),
}

pub fn paint_comparison(ui: &mut egui::Ui, action: &Action) -> Option<ComparisonIntent> {
    let comparison = action.plan_comparison.as_ref()?;
    ui.heading(format!("Compare plans for {}", action.id));
    ui.label("Kool.ad/e drafted both approaches from the feature and project evidence. The recommendation is guidance; you choose.");
    let mut intent = None;
    let mut paint_plan = |ui: &mut egui::Ui, plan: &crate::domain::PlanAlternative| {
        ui.push_id(&plan.id, |ui| {
            super::theme::card_frame().show(ui, |ui| {
                ui.heading(format!("Plan {}", plan.id));
                ui.label(&plan.objective);
                ui.collapsing("Phases", |ui| {
                    for (index, phase) in plan.phases.iter().enumerate() {
                        ui.label(format!("{}. {}", index + 1, phase.name));
                        for subtask in &phase.subtasks {
                            ui.label(format!("  • {subtask}"));
                        }
                    }
                });
                detail_list(ui, "Files touched", &plan.files_touched);
                detail_list(ui, "State changes", &plan.state_changes);
                detail_list(ui, "Failure modes", &plan.failure_modes);
                ui.label(format!("Effort: {}", plan.effort_band));
                detail_list(ui, "Known risks", &plan.known_risks);
                ui.label(format!("Reversibility: {}", plan.reversibility));
                if comparison.selected_plan.as_deref() == Some(&plan.id) {
                    ui.label(egui::RichText::new("Selected").color(super::theme::SUCCESS));
                } else if comparison.selected_plan.is_none()
                    && ui.button(format!("Adopt Plan {}", plan.id)).clicked()
                {
                    intent = Some(ComparisonIntent::Adopt(plan.id.clone()));
                }
            });
        });
    };
    if ui.available_width() >= 700.0 {
        ui.columns(comparison.alternatives.len().max(1), |columns| {
            for (ui, plan) in columns.iter_mut().zip(&comparison.alternatives) {
                paint_plan(ui, plan);
            }
        });
    } else {
        for plan in &comparison.alternatives {
            paint_plan(ui, plan);
            ui.add_space(super::theme::spacing::M);
        }
    }
    super::theme::card_frame().show(ui, |ui| {
        ui.label(
            egui::RichText::new(format!(
                "Kool.ad/e recommends Plan {}",
                comparison.recommendation.plan_id
            ))
            .strong(),
        );
        ui.label(&comparison.recommendation.rationale);
        for evidence in &comparison.recommendation.evidence {
            ui.label(format!("Evidence: {evidence}"));
        }
        ui.label("The recommendation is advisory; the operator chooses the plan.");
    });
    if comparison.selected_plan.is_none() {
        ui.label("If you defer, the feature remains unapproved and no implementation tasks are generated.");
        if ui.button("Discard and re-compare").clicked() {
            intent = Some(ComparisonIntent::Discard);
        }
    } else {
        ui.label("The adopted plan is frozen into the feature and its decision record.");
    }
    intent
}

fn detail_list(ui: &mut egui::Ui, title: &str, entries: &[String]) {
    ui.collapsing(title, |ui| {
        for entry in entries {
            ui.label(format!("• {entry}"));
        }
    });
}

impl Action {
    pub fn label(&self) -> String {
        if self.retry_review {
            return format!("Start or retry independent plan review for {}", self.id);
        }
        if self.compare_plans {
            return format!("Compare plans for {}", self.id);
        }
        if self
            .plan_comparison
            .as_ref()
            .is_some_and(|comparison| comparison.selected_plan.is_none())
        {
            return format!("Choose a plan for {} below", self.id);
        }
        match (self.approved, self.prepare_tasks) {
            (true, _) => format!("Prepare tasks for {}", self.id),
            (false, true) => format!("Confirm {} and prepare tasks", self.id),
            (false, false) => format!("Confirm plan for {}", self.id),
        }
    }
}

pub fn paint_request_changes(ui: &mut egui::Ui, action: &Action) -> Option<ApprovalIntent> {
    let candidate = action.plan_snapshot.as_ref()?;
    ui.button("Request changes")
        .clicked()
        .then(|| ApprovalIntent::RequestChanges(action.id.clone(), candidate.content_hash.clone()))
}

pub fn paint(ui: &mut egui::Ui, actions: &[Action], busy: bool) -> Option<ApprovalIntent> {
    let mut selected = None;
    for action in actions {
        ui.push_id((&action.id, "feature_approval"), |ui| {
            super::theme::card_frame().show(ui, |ui| {
                if !action.user_confirmed {
                    confirmation::paint(ui, action, "Plan confirmation");
                    if !action.ready {
                        ui.label("Readiness evidence is missing or stale. Reassess the plan before confirming it.");
                    }
                } else if action.retry_review {
                    confirmation::paint(ui, action, "Confirmed plan snapshot");
                    ui.label("Your confirmation is saved. Task generation remains blocked until an independent review passes.");
                } else if !action.ready {
                    ui.label("Readiness evidence is missing, stale, or blocked. Reassess the plan before preparing tasks.");
                }
                if let Some(verdict) = &action.review_verdict {
                    confirmation::paint_review(ui, verdict);
                }
                ui.collapsing(format!("Review {} specification", action.id), |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .show(ui, |ui| {
                            super::markdown::paint(
                                ui,
                                &action.specification,
                                super::markdown::CHAT,
                            );
                        });
                });
                let needs_choice = action
                    .plan_comparison
                    .as_ref()
                    .is_some_and(|comparison| comparison.selected_plan.is_none());
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            !busy && !needs_choice && action.ready,
                            egui::Button::new(action.label()),
                        )
                        .clicked()
                    {
                        selected = Some(ApprovalIntent::Confirm(
                            action.id.clone(),
                            action.compare_plans,
                        ));
                    }
                    if !busy
                        && !needs_choice
                        && !action.user_confirmed
                        && let Some(intent) = paint_request_changes(ui, action)
                    {
                        selected = Some(intent);
                    }
                });
                if action.prepare_tasks {
                    ui.label("Refreshes the task plan if needed, then generates task stories.");
                }
            });
        });
    }
    selected
}

#[cfg(test)]
#[path = "feature_approval/tests.rs"]
mod tests;
