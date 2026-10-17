use super::{KooladeApp, Screen};

impl KooladeApp {
    pub(in crate::app::root) fn defer_clarification(
        &mut self,
        uid: &str,
        expected_revision: u64,
        details: crate::core::planning::DeferredAssumption,
    ) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some() {
            self.toasts
                .warning("Finish the active planning turn before deferring this clarification.");
            return;
        }
        let store = crate::core::planning::QuestionStore::new(&project.state.planning_store);
        let (mut question, record_revision) = match store.load(uid) {
            Ok(question) => question,
            Err(error) => {
                self.toasts.danger(format!(
                    "Could not load clarification for deferral: {error}"
                ));
                return;
            }
        };
        let planning_revision =
            match super::fence::current_question_planning_revision(project, &store, &question) {
                Ok(revision) => revision,
                Err(error) => {
                    self.toasts.warning(format!(
                        "Clarification is stale or its current plan cannot be verified: {error}"
                    ));
                    return;
                }
            };
        if question.status != crate::core::planning::QuestionStatus::Open {
            self.toasts
                .warning("Only an open clarification can be deferred.");
            return;
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let actor = project.state.effective_user().name;
        let actor = if actor.trim().is_empty() {
            "(guest)"
        } else {
            &actor
        };
        let plan_revision = question.plan_revision;
        if let Err(error) = crate::core::planning::defer_question(
            &mut question,
            expected_revision,
            plan_revision,
            details,
            actor,
            now_ms,
        ) {
            self.toasts
                .warning(format!("Clarification could not be deferred: {error}"));
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
                    "Clarification changed before deferral was saved: {error}"
                ));
                return;
            }
        };
        let deferred = question
            .deferred
            .as_ref()
            .expect("validated deferral has details");
        let message = format!(
            "Optional clarification deferred by {}. Assumption: {}. Owner: {}; risk: {}; verify with {}; follow-up: {}.",
            actor,
            deferred.assumption,
            deferred.owner,
            deferred.risk,
            deferred.verification_test,
            deferred.follow_up_task,
        );
        let readiness_message = match readiness {
            Some(crate::core::planning::ReadinessResult::Ready) => {
                "Current readiness evidence passes with this optional assumption recorded."
                    .to_owned()
            }
            Some(crate::core::planning::ReadinessResult::NeedsWork { .. }) => {
                "The deferral is recorded; other readiness work remains.".to_owned()
            }
            Some(crate::core::planning::ReadinessResult::PlanningStalled { .. }) => {
                "The deferral is recorded; planning remains stalled.".to_owned()
            }
            Some(crate::core::planning::ReadinessResult::InvalidEvidence { reasons }) => {
                format!(
                    "The deferral is recorded, but readiness evidence is invalid: {}",
                    reasons.join("; ")
                )
            }
            None => "The deferral is recorded; no readiness checklist exists yet.".to_owned(),
        };
        project.task_chats.ensure_loaded(&project.chat_slug);
        let key = format!("clarification:{uid}");
        if let Err(error) = project.task_chats.append(
            &project.chat_slug,
            &key,
            vec![
                crate::domain::chatlog::ChatMessage::new(
                    crate::domain::chatlog::ChatRole::User,
                    &message,
                    Some(key.clone()),
                ),
                crate::domain::chatlog::ChatMessage::new(
                    crate::domain::chatlog::ChatRole::System,
                    &readiness_message,
                    Some(key.clone()),
                ),
            ],
        ) {
            self.toasts.warning(format!(
                "Deferral saved, but its clarification thread could not be updated: {error}"
            ));
        } else if readiness_message.starts_with("Current readiness evidence passes") {
            self.toasts.success(readiness_message);
        } else {
            self.toasts.warning(readiness_message);
        }
    }
}
