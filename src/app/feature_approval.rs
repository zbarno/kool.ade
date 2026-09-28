use super::*;
use crate::core::workflow::{self, TurnPurpose};
use crate::ui::feature_approval::Action;
#[path = "feature_approval/selection.rs"]
mod selection;

impl PacketApp {
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
                let approved = p
                    .state
                    .workflow
                    .approved_features
                    .get(id)
                    .is_some_and(|saved| *saved == workflow::feature_contract(body));
                let prepare_tasks = p
                    .state
                    .active_feature
                    .as_ref()
                    .is_some_and(|(active, _)| active == id)
                    && !has_current_task_batch(p);
                (!approved || prepare_tasks).then(|| Action {
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

    pub(super) fn approve_and_prepare_feature(&mut self, id: &str) {
        self.approve_feature_action(id, true);
    }

    pub(super) fn approve_feature_only(&mut self, id: &str) {
        self.approve_feature_action(id, false);
    }

    fn approve_feature_action(&mut self, id: &str, prepare_tasks: bool) {
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
        if let Err(error) = workflow::approve_feature_if_current(
            &p.state.repo_root,
            &mut p.state.workflow,
            id,
            Some(&contract),
        ) {
            p.remember_chat(vec![ChatMessage::new(
                ChatRole::System,
                format!("Cannot approve {id}: {error}"),
                None,
            )]);
            self.toasts.danger(format!("Cannot approve {id}: {error}"));
            return;
        }
        let message =
            format!("Approved {id} for implementation against the reviewed feature contract.");
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
        self.toasts.success(&message);
        if !prepare_tasks || !action.prepare_tasks {
            return;
        }
        if !p.active_implementations.is_empty() {
            p.remember_chat(vec![ChatMessage::new(ChatRole::System,
                format!("{id} is approved. Use Prepare tasks for {id} after the current implementation workers finish."), None)]);
            return;
        }
        if p.state.workflow.ready(p.state.planning_contract()) {
            self.start_turn_with_purpose(
                &format!("Generate task stories for approved feature {id}."),
                TurnPurpose::GenerateTasks,
            );
        } else {
            self.pending_feature_generation =
                Some((p.state.repo_root.clone(), id.into(), contract));
            self.start_turn_with_purpose(&format!(
                "Prepare the task-generation review for approved feature {id}. Its current specification is approved; do not ask for approval again or change its contract. Read the current feature and settled decisions, and return a complete interview brief naming {id}, ready_for_tasks=true when no blocking questions remain. Do not generate stories in this review turn. The application will generate them after a successful current review. If blocked, record the concrete blocking item."
            ), TurnPurpose::ReviewForGeneration);
            if !matches!(&self.screen, Screen::Connected(p) if p.active_turn.is_some()) {
                self.pending_feature_generation = None;
            }
        }
    }

    /// Only the completion of our Main Chat review can consume this continuation.
    /// A failed/cancelled review or changed contract never triggers generation.
    pub(super) fn continue_feature_generation(&mut self, applied: bool) {
        let Some((repo, id, contract)) = self.pending_feature_generation.take() else {
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
            && p.active_implementations.is_empty()
            && workflow::feature_approved(&repo, &p.state.workflow, &id)
        {
            self.start_turn_with_purpose(
                &format!("Generate task stories for approved feature {id}."),
                TurnPurpose::GenerateTasks,
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
        }
    }
}

#[cfg(test)]
#[path = "root/feature_approval/tests.rs"]
mod tests;
