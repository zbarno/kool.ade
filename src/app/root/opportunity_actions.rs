use super::{KooladeApp, Screen};
use crate::{
    core::opportunity::{OpportunityStatusV1, OpportunityStore},
    harness::prompt_catalog::{OpportunityChoice, OpportunityDecisionV1},
};

mod feature;
mod self_improvement;
mod settings;
mod snooze;
use feature::create_draft_feature;
use self_improvement::create_self_improvement_run;

impl KooladeApp {
    pub(super) fn confirm_self_improvement_installation(
        &mut self,
        run_uid: &str,
        expected_revision: u64,
    ) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.state.identity.source == crate::domain::IdentitySource::Guest {
            self.toasts
                .warning("Set a current user identity before confirming installation.");
            return;
        }
        let operator_id = project.state.effective_user().name.trim().to_owned();
        if operator_id.is_empty() {
            self.toasts
                .warning("Set a current user identity before confirming installation.");
            return;
        }
        match crate::core::self_improvement::SelfImprovementStore::new(
            &project.state.planning_store,
        )
        .record_installation_confirmation(run_uid, expected_revision, &operator_id)
        {
            Ok(_) => self
                .toasts
                .success("Installation confirmation recorded; capability readiness is pending."),
            Err(error) => self
                .toasts
                .warning(format!("Could not confirm installation: {error}")),
        }
    }

    pub(super) fn retry_opportunity_planning(&mut self, uid: &str, expected_revision: u64) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some() {
            self.toasts
                .warning("Finish the active planning turn before creating an opportunity goal.");
            return;
        }
        let store = OpportunityStore::new(&project.state.planning_store);
        let (proposal, actual_revision) = match store.load(uid) {
            Ok(value) => value,
            Err(error) => {
                self.toasts
                    .danger(format!("Could not load opportunity: {error}"));
                return;
            }
        };
        if actual_revision != expected_revision
            || proposal.status != OpportunityStatusV1::ApprovedToPlan
        {
            self.toasts.warning(
                "The pursued opportunity changed; refresh its current status before retrying.",
            );
            return;
        }
        match create_draft_feature(&project.state.planning_store, &proposal) {
            Ok((feature_id, feature_uid, markdown, revision)) => {
                match create_self_improvement_run(
                    &project.state.planning_store,
                    &store,
                    &proposal,
                    &feature_uid,
                ) {
                    Ok(Some(_)) => {
                        project.state.active_features.retain(|(id, _)| id != &feature_id);
                        project.state.active_features.push((feature_id.clone(), markdown.clone()));
                        project.state.active_feature = Some((feature_id.clone(), markdown));
                        project.state.baseline_planning_revision = project
                            .state
                            .planning_store
                            .revision()
                            .unwrap_or(revision);
                        self.toasts.success(format!("Draft feature {feature_id} and self-improvement run were added to planning; review and approve the specification before clone allocation."));
                    }
                    Ok(None) => match store.mark_planning(uid, &feature_uid, actual_revision) {
                    Ok(_) => {
                        project.state.active_features.retain(|(id, _)| id != &feature_id);
                        project.state.active_features.push((feature_id.clone(), markdown.clone()));
                        project.state.active_feature = Some((feature_id.clone(), markdown));
                        project.state.baseline_planning_revision = project
                            .state
                            .planning_store
                            .revision()
                            .unwrap_or(revision);
                        self.toasts.success(format!("Draft feature {feature_id} was added to planning; review and approve its specification before task generation."));
                    }
                    Err(error) => self.toasts.warning(format!("Draft feature {feature_id} exists, but opportunity status could not be advanced: {error}. Refresh before retrying.")),
                    },
                    Err(error) => self.toasts.warning(format!("Draft feature {feature_id} exists, but its self-improvement run could not be persisted: {error}. Refresh before retrying.")),
                }
            }
            Err(error) => self
                .toasts
                .warning(format!("Could not create the planning goal: {error}")),
        }
    }

    pub(super) fn decide_opportunity(
        &mut self,
        uid: &str,
        expected_revision: u64,
        choice: OpportunityChoice,
        explanation: Option<String>,
    ) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some() {
            self.toasts
                .warning("Finish the active planning turn before deciding on an opportunity.");
            return;
        }
        let store = OpportunityStore::new(&project.state.planning_store);
        let (proposal, actual_revision) = match store.load(uid) {
            Ok(value) => value,
            Err(error) => {
                self.toasts
                    .danger(format!("Could not load opportunity: {error}"));
                return;
            }
        };
        if actual_revision != expected_revision {
            self.toasts
                .warning("This opportunity changed in another session. Refresh before deciding.");
            return;
        }
        if project.state.identity.source == crate::domain::IdentitySource::Guest {
            self.toasts.warning(
                "Set a current user identity in workspace settings before recording an opportunity decision.",
            );
            return;
        }
        let actor_id = project.state.effective_user().name.trim().to_owned();
        if actor_id.is_empty() {
            self.toasts.warning(
                "Set a current user identity in workspace settings before recording an opportunity decision.",
            );
            return;
        }
        let occurred_at = chrono::Utc::now();
        let snooze_until = (choice == OpportunityChoice::NotNow).then(|| {
            (occurred_at
                + chrono::Duration::milliseconds(
                    crate::core::opportunity::OPPORTUNITY_SNOOZE_MS as i64,
                ))
            .to_rfc3339()
        });
        let decision = OpportunityDecisionV1 {
            uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            opportunity_uid: uid.to_owned(),
            revision_hash: match proposal.revision_hash() {
                Ok(hash) => hash,
                Err(error) => {
                    self.toasts
                        .danger(format!("Opportunity is invalid: {error}"));
                    return;
                }
            },
            user_id: actor_id.clone(),
            decision: choice.clone(),
            occurred_at: occurred_at.to_rfc3339(),
            snooze_until,
            explanation: explanation.clone(),
        };
        let recorded = match store.record_user_decision(expected_revision, &decision, &actor_id) {
            Ok(result) => result,
            Err(error) => {
                self.toasts
                    .warning(format!("Opportunity decision was not saved: {error}"));
                return;
            }
        };
        let (updated, decision_revision) = match recorded {
            crate::core::opportunity::OpportunityDecisionResult::Recorded {
                proposal,
                record_revision,
                ..
            }
            | crate::core::opportunity::OpportunityDecisionResult::AlreadyRecorded {
                proposal,
                record_revision,
                ..
            } => (proposal, record_revision),
        };
        let mut message = format!("Opportunity decision recorded as {:?}.", choice);
        if matches!(choice, OpportunityChoice::Pursue)
            && updated.status == OpportunityStatusV1::ApprovedToPlan
        {
            match create_draft_feature(&project.state.planning_store, &updated) {
                Ok((feature_id, feature_uid, markdown, revision)) => {
                    match create_self_improvement_run(
                        &project.state.planning_store,
                        &store,
                        &updated,
                        &feature_uid,
                    ) {
                        Ok(Some(_)) => {
                            project
                                .state
                                .active_features
                                .retain(|(id, _)| id != &feature_id);
                            project
                                .state
                                .active_features
                                .push((feature_id.clone(), markdown.clone()));
                            project.state.active_feature = Some((feature_id.clone(), markdown));
                            project.state.baseline_planning_revision =
                                project.state.planning_store.revision().unwrap_or(revision);
                            message = format!(
                                "Opportunity pursued. Draft feature {feature_id} and self-improvement run were added to planning; review and approve its specification before clone allocation."
                            );
                        }
                        Ok(None) => match store.mark_planning(
                            &updated.opportunity_uid,
                            &feature_uid,
                            decision_revision,
                        ) {
                            Ok((_, _)) => {
                                project
                                    .state
                                    .active_features
                                    .retain(|(id, _)| id != &feature_id);
                                project
                                    .state
                                    .active_features
                                    .push((feature_id.clone(), markdown.clone()));
                                project.state.active_feature = Some((feature_id.clone(), markdown));
                                project.state.baseline_planning_revision =
                                    project.state.planning_store.revision().unwrap_or(revision);
                                message = format!(
                                    "Opportunity pursued. Draft feature {feature_id} was added to planning; review and approve its specification before task generation."
                                );
                            }
                            Err(error) => {
                                message = format!(
                                    "The Pursue decision is saved and draft {feature_id} exists, but opportunity status could not be advanced: {error}. Refresh and retry planning adoption."
                                );
                            }
                        },
                        Err(error) => {
                            message = format!(
                                "The Pursue decision is saved and draft {feature_id} exists, but its self-improvement run could not be persisted: {error}. Refresh and retry planning adoption."
                            );
                        }
                    }
                }
                Err(error) => {
                    message = format!(
                        "The Pursue decision is saved, but a planning feature could not be created: {error}. The opportunity remains approved to plan."
                    );
                }
            }
        }
        if choice == OpportunityChoice::AskForMoreInformation {
            let question = explanation.unwrap_or_default();
            let prompt = format!(
                "Answer the user's question about this improvement proposal. This is a discussion only: do not approve the proposal, create an implementation feature or task, change repository state, or authorize execution or publication. Use the supplied proposal and evidence, state uncertainty, and explain practical options.\n\nProposal UID: {}\nTarget repository: {} ({})\nProblem or opportunity: {}\nUser value: {}\nExpected outcome: {}\nCurrent capability: {}\nWorkaround: {}\nRecommended approach: {}\nRisks and cost: {}\nEvidence references: {}\n\nUser question: {}",
                updated.opportunity_uid,
                updated.target.repository_uid,
                updated.target.canonical_remote,
                updated.problem_or_opportunity,
                updated.user_value,
                updated.expected_outcome,
                updated.current_capability_state,
                updated
                    .generic_workaround
                    .as_deref()
                    .unwrap_or("none recorded"),
                updated.recommended_approach,
                updated.cost_or_risk,
                updated
                    .evidence_refs
                    .iter()
                    .map(|evidence| evidence.evidence_uid.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                question
            );
            self.toasts
                .success("Question recorded. Man.ager is preparing an evidence-based reply.");
            self.start_turn_with_purpose(&prompt, crate::core::workflow::TurnPurpose::Question);
        } else {
            self.toasts.success(message);
        }
    }
}
