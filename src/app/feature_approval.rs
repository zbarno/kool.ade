use super::*;
use crate::core::workflow::{self, TurnPurpose};
use crate::ui::feature_approval::Action;
#[path = "feature_approval/actions.rs"]
mod actions;
#[path = "feature_approval/board.rs"]
mod board;
#[path = "feature_approval/readiness.rs"]
pub(super) mod readiness;
#[path = "feature_approval/selection.rs"]
mod selection;

impl KooladeApp {
    pub(super) fn available_feature_actions(&self, conversation: Option<&str>) -> Vec<Action> {
        let Screen::Connected(p) = &self.screen else {
            return Vec::new();
        };
        let related = conversation.and_then(|key| {
            p.state
                .items
                .iter()
                .chain(&p.state.resolved_items)
                .find(|item| item.conversation_key() == key)
                .and_then(|item| item.feature_id.as_deref())
                .or_else(|| {
                    p.task_documents
                        .iter()
                        .find(|doc| doc.path == key)
                        .and_then(|doc| {
                            doc.text
                                .lines()
                                .find_map(|line| line.strip_prefix("Feature ID: "))
                        })
                })
        });
        p.state
            .active_features
            .iter()
            .filter_map(|(id, body)| {
                if conversation.is_some() && related != Some(id.as_str()) {
                    return None;
                }
                let Ok(metadata) = crate::domain::ChangeMetadata::require_markdown(body) else {
                    return None;
                };
                if !metadata.status.approval_eligible() {
                    return None;
                }
                let comparison = p
                    .state
                    .workflow
                    .plan_comparisons
                    .get(id)
                    .filter(|record| {
                        record.status != crate::core::workflow::PlanComparisonStatus::Discarded
                    })
                    .map(|record| record.alternatives.clone())
                    .or_else(|| metadata.plan_comparison.clone());
                let compare_plans = metadata.status == crate::domain::ChangeStatus::Ready
                    && metadata.schema_version == 2
                    && comparison.is_none();
                let approved =
                    workflow::feature_approved(&p.state.planning_store, &p.state.workflow, id);
                let reviewing = p.activity.plan_review_feature.as_deref() == Some(id.as_str());
                let prepare_tasks = p
                    .state
                    .active_feature
                    .as_ref()
                    .is_some_and(|(active, _)| active == id)
                    && !has_current_task_batch(p);
                ((!approved || prepare_tasks) && !reviewing).then(|| Action {
                    id: id.clone(),
                    specification: body.clone(),
                    approved,
                    prepare_tasks,
                    compare_plans,
                    plan_comparison: comparison,
                })
            })
            .collect()
    }

    fn approve_feature_action(&mut self, id: &str, _prepare_tasks: bool) {
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
        let snapshot = match crate::core::lifecycle::PlanSnapshotV1::from_feature(
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
        #[cfg(test)]
        let review_start: Result<_, String> =
            Err("test-only reviewer outcomes must be supplied explicitly".into());
        #[cfg(not(test))]
        let review_start = {
            let harness = super::configured_harness_for(
                &mut self.task_harness,
                Some(crate::persistence::harness_settings::PLAN_REVIEW),
            );
            crate::app::root::plan_review::PlanReview::start(p, id.to_owned(), harness)
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

    /// Only the completion of our Main Chat review can consume this continuation.
    /// A failed/cancelled review or changed contract never triggers generation.
    pub(super) fn continue_feature_generation(&mut self, applied: bool) {
        let Some((repo, id, contract, work_key)) = self.pending_feature_generation.take() else {
            return;
        };
        let Screen::Connected(p) = &mut self.screen else {
            return;
        };
        if p.state.repo_root != repo {
            return;
        }
        let current = p
            .state
            .active_feature
            .as_ref()
            .is_some_and(|(active, text)| {
                active == &id && workflow::feature_contract(text) == contract
            });
        if applied
            && current
            && p.state.workflow.ready(p.state.planning_contract())
            && !has_current_task_batch(p)
            && workflow::feature_approved(&p.state.planning_store, &p.state.workflow, &id)
        {
            self.start_turn_for_work(
                &format!("Generate task stories for approved feature {id}."),
                TurnPurpose::GenerateTasks,
                None,
                (!work_key.is_empty()).then_some(work_key),
            );
        } else {
            let reason = if !current {
                "The feature contract or planning focus changed; review the current feature before continuing."
            } else {
                "The planning review did not produce a current, ready task plan. Resolve the reported issue, then use Prepare tasks to retry; the existing feature approval is retained."
            };
            p.remember_chat(vec![ChatMessage::new(
                ChatRole::System,
                format!("Task preparation for {id} paused. {reason}"),
                None,
            )]);
            if !work_key.is_empty()
                && let Some(work) = p.planning_work.iter_mut().find(|work| work.key == work_key)
            {
                work.status = crate::core::planning_work::WorkStatus::NeedsAttention;
                work.detail = format!("Task generation paused. {reason}");
                if let Err(error) = p.save_planning_work() {
                    self.toasts
                        .danger(format!("Cannot save the task generation status: {error}"));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "root/feature_approval/tests.rs"]
pub(crate) mod tests;
