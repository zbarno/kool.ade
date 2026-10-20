use super::*;

impl KooladeApp {
    pub(crate) fn available_feature_actions(&self, conversation: Option<&str>) -> Vec<Action> {
        let Screen::Connected(project) = &self.screen else {
            return Vec::new();
        };
        let related = conversation.and_then(|key| {
            project
                .state
                .items
                .iter()
                .chain(&project.state.resolved_items)
                .find(|item| item.conversation_key() == key)
                .and_then(|item| item.feature_id.as_deref())
                .or_else(|| {
                    project
                        .task_documents
                        .iter()
                        .find(|doc| doc.path == key)
                        .and_then(|doc| {
                            doc.text
                                .lines()
                                .find_map(|line| line.strip_prefix("Feature ID: "))
                        })
                })
        });
        project
            .state
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
                let comparison = project
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
                let approved = crate::core::workflow::feature_approved(
                    &project.state.planning_store,
                    &project.state.workflow,
                    id,
                );
                let reviewing =
                    project.activity.plan_review_feature.as_deref() == Some(id.as_str());
                let plan_snapshot = self.current_plan_snapshot(project, id, body);
                let review_verdict = plan_snapshot.as_ref().and_then(|snapshot| {
                    crate::core::lifecycle::plan_snapshot::plan_review_verdict(
                        &project.state.planning_store,
                        snapshot,
                    )
                    .ok()
                    .flatten()
                });
                let user_confirmed = plan_snapshot.as_ref().is_some_and(|snapshot| {
                    crate::core::lifecycle::plan_snapshot::has_current_user_confirmation(
                        &project.state.planning_store,
                        snapshot,
                    )
                    .unwrap_or(false)
                });
                let ready = plan_snapshot.as_ref().is_some_and(|snapshot| {
                    super::readiness::current(&project.state.planning_store, snapshot).is_ok()
                });
                let prepare_tasks = project
                    .state
                    .active_feature
                    .as_ref()
                    .is_some_and(|(active, _)| active == id)
                    && approved
                    && ready
                    && !crate::app::manager::has_pending_plan_repair(project, id)
                    && !crate::app::manager::has_pending_plan_contest(project, id)
                    && !super::super::has_current_task_batch(project);
                let retry_review = user_confirmed
                    && ready
                    && !approved
                    && plan_snapshot
                        .as_ref()
                        .is_some_and(|snapshot| self.review_action_available(project, snapshot));
                let review_attention = review_verdict.as_ref().is_some_and(|verdict| !verdict.pass);
                (((!user_confirmed && ready)
                    || prepare_tasks
                    || retry_review
                    || review_attention
                    || (user_confirmed && !ready))
                    && !reviewing)
                    .then(|| Action {
                        id: id.clone(),
                        specification: body.clone(),
                        approved,
                        user_confirmed,
                        ready,
                        plan_snapshot,
                        review_verdict,
                        prepare_tasks,
                        retry_review,
                        compare_plans,
                        plan_comparison: comparison,
                    })
            })
            .collect()
    }

    fn review_action_available(
        &self,
        project: &super::super::session::Project,
        snapshot: &crate::core::lifecycle::PlanSnapshotV1,
    ) -> bool {
        let retry = crate::core::lifecycle::plan_snapshot::review_retry_available(
            &project.state.planning_store,
            snapshot,
        )
        .unwrap_or(false);
        let not_started = crate::core::lifecycle::plan_snapshot::plan_review_receipt(
            &project.state.planning_store,
            snapshot,
        )
        .is_ok_and(|receipt| receipt.is_none());
        retry || not_started
    }

    fn current_plan_snapshot(
        &self,
        project: &super::super::session::Project,
        id: &str,
        markdown: &str,
    ) -> Option<crate::core::lifecycle::PlanSnapshotV1> {
        let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .ok()
            .flatten()?;
        let contract = crate::core::workflow::feature_contract(markdown);
        crate::core::lifecycle::plan_snapshot::from_feature_with_author(
            &project.state.planning_store,
            id,
            &identity,
            markdown,
            &contract,
        )
        .ok()
    }
}
