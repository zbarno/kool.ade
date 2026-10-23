use super::*;

impl KooladeApp {
    pub(super) fn approve_feature_action(&mut self, id: &str, _prepare_tasks: bool) {
        self.approve_feature_action_inner(id, _prepare_tasks, None);
    }

    #[cfg(test)]
    pub(crate) fn approve_feature_action_with_harness_for_test(
        &mut self,
        id: &str,
        harness: Box<dyn crate::harness::AiHarness>,
    ) {
        self.approve_feature_action_inner(id, false, Some(harness));
    }

    fn approve_feature_action_inner(
        &mut self,
        id: &str,
        _prepare_tasks: bool,
        review_harness: Option<Box<dyn crate::harness::AiHarness>>,
    ) {
        let Some(action) = self
            .available_feature_actions(None)
            .into_iter()
            .find(|a| a.id == id)
        else {
            return;
        };
        if action.compare_plans {
            self.start_comparison_turn(id);
            return;
        }
        if action
            .plan_comparison
            .as_ref()
            .is_some_and(|comparison| comparison.selected_plan.is_none())
        {
            self.toasts
                .warning("Choose Plan A or Plan B before approving this feature.");
            return;
        }
        let Screen::Connected(p) = &mut self.screen else {
            return;
        };
        if p.active_turn.is_some()
            || p.task_turns.keys().any(|key| {
                p.state
                    .items
                    .iter()
                    .chain(&p.state.resolved_items)
                    .any(|item| {
                        item.conversation_key() == key && item.feature_id.as_deref() == Some(id)
                    })
            })
        {
            self.toasts
                .warning("Wait for the current planning reply before approving this feature.");
            return;
        }
        let contract = workflow::feature_contract(&action.specification);
        let Some(identity) = crate::domain::ArtifactIdentity::from_markdown(&action.specification)
            .ok()
            .flatten()
        else {
            self.toasts
                .danger(format!("Cannot approve {id}: feature identity is missing."));
            return;
        };
        let snapshot = match crate::core::lifecycle::plan_snapshot::from_feature_with_author(
            &p.state.planning_store,
            id,
            &identity,
            &action.specification,
            &contract,
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.toasts.danger(format!("Cannot snapshot {id}: {error}"));
                return;
            }
        };
        if let Err(reason) = readiness::current(&p.state.planning_store, &snapshot) {
            let message = format!("Cannot confirm {id}: {reason}");
            p.remember_chat(vec![ChatMessage::new(ChatRole::System, &message, None)]);
            self.toasts.warning(message);
            return;
        }
        let mut approved_workflow = p.state.workflow.clone();
        let receipt = match workflow::approve_feature_if_current_with_snapshot_with_revision(
            &p.state.planning_store,
            &mut approved_workflow,
            id,
            Some(&contract),
            Some(&snapshot.content_hash),
            &p.state.baseline_planning_revision,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                p.remember_chat(vec![ChatMessage::new(
                    ChatRole::System,
                    format!("Cannot approve {id}: {error}"),
                    None,
                )]);
                self.toasts.danger(format!("Cannot approve {id}: {error}"));
                return;
            }
        };
        p.state.workflow = approved_workflow;
        p.state.baseline_planning_revision = receipt.planning_revision;
        if let Err(error) = receipt.commit_result {
            self.toasts.warning(format!(
                "{id} was approved, but its Git checkpoint failed: {error}"
            ));
            return;
        }
        let review_start = if let Some(harness) = review_harness {
            crate::app::root::plan_review::PlanReview::start(p, id.to_owned(), harness)
        } else {
            #[cfg(test)]
            {
                Err("test-only reviewer outcomes must be supplied explicitly".into())
            }
            #[cfg(not(test))]
            {
                let harness = crate::app::root::configured_harness_for_project(
                    &mut self.task_harness,
                    Some(crate::persistence::harness_settings::PLAN_REVIEW),
                    &p.state.harness_routes.routes,
                );
                crate::app::root::plan_review::PlanReview::start(p, id.to_owned(), harness)
            }
        };
        match review_start {
            Ok(review) => {
                p.activity.plan_review = Some(review);
                p.activity.plan_review_feature = Some(id.to_owned());
                p.activity.plan_review_revision = Some(p.state.baseline_planning_revision.clone());
                p.live_progress = crate::harness::LiveProgress {
                    activity: Some(format!("Independent PlanReviewer is reviewing {id}…")),
                    ..Default::default()
                };
            }
            Err(error) => {
                p.activity
                    .pending
                    .push(format!("Plan review for {id} could not start: {error}"));
                self.toasts.warning(format!(
                    "Plan confirmation saved, but independent review could not start: {error}"
                ));
            }
        }
        let message = format!(
            "Plan confirmation recorded for {id}. Task generation remains blocked until a separate PlanReviewer approves this exact snapshot."
        );
        p.activity.pending.push(message.clone());
        p.remember_chat(vec![ChatMessage::new(ChatRole::System, &message, None)]);
        // Put the application receipt beside requests in resolved conversations too.
        let keys = p
            .state
            .items
            .iter()
            .chain(&p.state.resolved_items)
            .filter(|item| item.feature_id.as_deref() == Some(id))
            .map(|item| item.conversation_key().to_owned())
            .collect::<Vec<_>>();
        for key in keys {
            if p.task_chats.messages.contains_key(&key) {
                p.task_chats.remember_response(
                    &p.chat_slug,
                    &key,
                    vec![ChatMessage::new(
                        ChatRole::System,
                        &message,
                        Some(key.clone()),
                    )],
                );
            }
        }
        self.toasts.warning(&message);
    }
}
