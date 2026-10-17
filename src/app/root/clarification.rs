use super::{KooladeApp, Screen};
use crate::domain::chatlog::{ChatMessage, ChatRole};

mod defer;
mod fence;
#[cfg(test)]
#[path = "clarification/tests.rs"]
mod tests;

impl KooladeApp {
    pub(super) fn start_clarification_planner(&mut self, feature_uid: &str, plan_revision: u64) {
        let harness = super::configured_harness_for(
            &mut self.task_harness,
            Some(crate::persistence::harness_settings::MANAGER),
        );
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let spec_revision = match super::planner_pass::spec_revision(project, feature_uid) {
            Ok(revision) => revision,
            Err(error) => {
                self.toasts.warning(format!(
                    "Planner readiness could not be bound to the current feature snapshot: {error}"
                ));
                return;
            }
        };
        let planner = super::planner_pass::ClarificationPlanner::start(
            project,
            feature_uid.to_owned(),
            plan_revision,
            harness,
        );
        project.activity.clarification_planner = Some(planner);
        project.activity.clarification_planner_identity =
            Some((feature_uid.to_owned(), plan_revision, spec_revision));
        project.live_progress = crate::harness::LiveProgress {
            activity: Some("Planner is reassessing clarification and readiness…".into()),
            ..Default::default()
        };
    }

    pub(super) fn answer_clarification(
        &mut self,
        uid: &str,
        expected_revision: u64,
        answer: &str,
        selected_option: Option<&str>,
    ) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some() {
            self.toasts
                .warning("Finish the active planning turn before answering this clarification.");
            return;
        }
        let store = crate::core::planning::QuestionStore::new(&project.state.planning_store);
        let (mut question, record_revision) = match store.load(uid) {
            Ok(question) => question,
            Err(error) => {
                self.toasts
                    .danger(format!("Could not load clarification: {error}"));
                return;
            }
        };
        if question.revision != expected_revision {
            self.toasts.warning(
                "This clarification changed in another session. Refresh it before answering.",
            );
            return;
        }
        let planning_revision =
            match fence::current_question_planning_revision(project, &store, &question) {
                Ok(revision) => revision,
                Err(error) => {
                    self.toasts.warning(format!(
                    "This clarification is stale or its current plan cannot be verified: {error}"
                ));
                    return;
                }
            };
        let answer = (!answer.trim().is_empty()).then_some(answer);
        let answered_by = project.state.effective_user().name;
        let answered_by = if answered_by.trim().is_empty() {
            "(guest)"
        } else {
            &answered_by
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let plan_revision = question.plan_revision;
        let declined = answer.is_none() && selected_option.is_none();
        let reconciliation = if declined {
            crate::core::planning::decline_question(
                &mut question,
                expected_revision,
                plan_revision,
                answered_by,
                now_ms,
            )
        } else {
            crate::core::planning::reconcile_answer(
                &mut question,
                expected_revision,
                plan_revision,
                answer,
                selected_option,
                answered_by,
                now_ms,
            )
        };
        if let Err(error) = reconciliation {
            self.toasts
                .warning(format!("Clarification could not be answered: {error}"));
            return;
        }
        let readiness = match store.save_question_and_reassess(
            &question,
            record_revision,
            &planning_revision,
        ) {
            Ok(readiness) => readiness,
            Err(error) => {
                self.toasts.warning(format!(
                    "Clarification changed before the answer was saved: {error}"
                ));
                return;
            }
        };
        if !declined {
            let next_plan_revision = match plan_revision.checked_add(1) {
                Some(revision) => revision,
                None => {
                    self.toasts.warning("The answer was saved, but the planning revision is exhausted; no follow-up Planner pass was started.");
                    return;
                }
            };
            project.activity.pending_clarification_planner =
                Some((question.feature_uid.clone(), next_plan_revision));
        }
        let readiness_message = match readiness {
            Some(crate::core::planning::ReadinessResult::Ready) => {
                "The recorded planning readiness checklist is complete.".to_owned()
            }
            Some(crate::core::planning::ReadinessResult::NeedsWork {
                blockers,
                unassessed,
                untraced_requirements,
                missing_acceptance_tests,
            }) => format!(
                "Readiness still needs work: {} blocking questions, {} unassessed areas, {} untraced requirements, missing acceptance tests: {}.",
                blockers.len(),
                unassessed.len(),
                untraced_requirements.len(),
                if missing_acceptance_tests { "yes" } else { "no" }
            ),
            Some(crate::core::planning::ReadinessResult::PlanningStalled { .. }) => {
                "Planning readiness is stalled; review its repeated no-progress evidence.".into()
            }
            Some(crate::core::planning::ReadinessResult::InvalidEvidence { reasons }) => {
                format!("Readiness evidence is invalid: {}", reasons.join("; "))
            }
            None => {
                "No readiness checklist is recorded for this plan; it remains unready until the planner records assessment evidence.".into()
            }
        };
        let mut message_text = answer.unwrap_or_default().trim().to_owned();
        if let Some(option_id) = selected_option
            && let Some(option) = question
                .options
                .iter()
                .find(|option| option.id == option_id)
        {
            if !message_text.is_empty() {
                message_text.push('\n');
            }
            message_text.push_str(&format!("Selected {}: {}", option.id, option.label));
        }
        if message_text.is_empty() {
            message_text = "I declined to choose an option.".into();
        }
        project.task_chats.ensure_loaded(&project.chat_slug);
        let key = format!("clarification:{uid}");
        if let Err(error) = project.task_chats.append(
            &project.chat_slug,
            &key,
            vec![
                ChatMessage::new(ChatRole::User, &message_text, Some(key.clone())),
                ChatMessage::new(ChatRole::System, &readiness_message, Some(key.clone())),
            ],
        ) {
            self.toasts.warning(format!(
                "Answer saved, but the clarification thread could not be updated: {error}"
            ));
        } else {
            if readiness_message
                .starts_with("The recorded planning readiness checklist is complete")
            {
                self.toasts
                    .success(format!("Clarification answer saved. {readiness_message}"));
            } else {
                self.toasts
                    .warning(format!("Clarification answer saved. {readiness_message}"));
            }
        }
        if !declined {
            let follow_up = format!(
                "Reconcile the user's recorded answer to clarification {uid} for feature {}.\n\nQuestion: {}\nAnswer: {}\n\nTreat the answer as explicit product intent. Update the feature decisions/specification with the answer, reassess material ambiguity and acceptance-test traceability, and raise only materially new follow-up questions. Do not change phase or approve the feature.",
                question.feature_uid, question.question, message_text
            );
            self.start_turn_with_purpose(&follow_up, crate::core::workflow::TurnPurpose::Interview);
            if let Screen::Connected(project) = &mut self.screen
                && project.active_turn.is_none()
            {
                project.activity.pending_clarification_planner = None;
            }
        }
    }
}
