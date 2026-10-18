use super::*;
use crate::core::workflow::{self, TurnPurpose};
use crate::ui::feature_approval::Action;
#[path = "feature_approval/actions.rs"]
mod actions;
#[path = "feature_approval/approval.rs"]
mod approval;
#[path = "feature_approval/board.rs"]
mod board;
#[path = "feature_approval/candidate.rs"]
mod candidate;
#[path = "feature_approval/readiness.rs"]
pub(super) mod readiness;
#[path = "feature_approval/request_changes.rs"]
mod request_changes;
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
                let candidate = candidate::from_feature(&p.state.planning_store, id, body);
                ((!approved || prepare_tasks) && !reviewing).then(|| Action {
                    id: id.clone(),
                    specification: body.clone(),
                    candidate,
                    approved,
                    prepare_tasks,
                    compare_plans,
                    plan_comparison: comparison,
                })
            })
            .collect()
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
