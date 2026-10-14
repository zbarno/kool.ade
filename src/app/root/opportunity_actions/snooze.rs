use super::super::{KooladeApp, Screen};

impl KooladeApp {
    pub(crate) fn snooze_opportunity(&mut self, uid: &str, expected_revision: u64, days: u8) {
        if !(1..=90).contains(&days) {
            self.toasts
                .warning("Opportunity snooze must be between 1 and 90 days.");
            return;
        }
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some() {
            self.toasts
                .warning("Finish the active planning turn before snoozing an opportunity.");
            return;
        }
        let store = crate::core::opportunity::OpportunityStore::new(&project.state.planning_store);
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
                .warning("This opportunity changed in another session. Refresh before snoozing.");
            return;
        }
        if project.state.identity.source == crate::domain::IdentitySource::Guest {
            self.toasts.warning(
                "Set a current user identity in workspace settings before snoozing an opportunity.",
            );
            return;
        }
        let actor_id = project.state.effective_user().name.trim().to_owned();
        if actor_id.is_empty() {
            self.toasts.warning(
                "Set a current user identity in workspace settings before snoozing an opportunity.",
            );
            return;
        }
        let now = chrono::Utc::now();
        let decision = crate::harness::prompt_catalog::OpportunityDecisionV1 {
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
            decision: crate::harness::prompt_catalog::OpportunityChoice::NotNow,
            occurred_at: now.to_rfc3339(),
            snooze_until: Some((now + chrono::Duration::days(i64::from(days))).to_rfc3339()),
            explanation: Some(format!("User snoozed this proposal for {days} days.")),
        };
        match store.record_user_decision(expected_revision, &decision, &actor_id) {
            Ok(_) => self
                .toasts
                .success(format!("Opportunity snoozed for {days} days.")),
            Err(error) => self
                .toasts
                .warning(format!("Opportunity snooze was not saved: {error}")),
        }
    }
}
