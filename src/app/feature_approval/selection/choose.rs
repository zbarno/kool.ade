use super::super::super::*;

impl KooladeApp {
    pub(in crate::app) fn choose_feature_plan(&mut self, id: &str, plan_id: &str) {
        if !["A", "B"].contains(&plan_id) {
            self.toasts
                .danger("Choose one of the listed plans (A or B).");
            return;
        }
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some() {
            self.toasts
                .warning("Wait for the plan comparison to finish before choosing.");
            return;
        }
        let Some((_, body)) = project
            .state
            .active_features
            .iter()
            .find(|(feature_id, _)| feature_id == id)
        else {
            self.toasts
                .danger(format!("Feature {id} is no longer available."));
            return;
        };
        let store = &project.state.planning_store;
        let guard = crate::core::writer_gate::acquire();
        let current =
            match crate::artifacts::product_docs::current_document(store, &format!("feature:{id}"))
            {
                Ok(Some(current)) if current == *body => current,
                Ok(Some(_)) => {
                    self.toasts
                        .warning("The feature changed. Reopen it and review the current plans.");
                    return;
                }
                Ok(None) => {
                    self.toasts
                        .danger(format!("Cannot select plan: feature {id} is unavailable."));
                    return;
                }
                Err(error) => {
                    self.toasts.danger(format!("Cannot read feature: {error}"));
                    return;
                }
            };
        let comparison = project
            .state
            .workflow
            .plan_comparisons
            .get(id)
            .map(|record| record.alternatives.clone());
        let Some(comparison) = comparison else {
            self.toasts
                .danger("The feature has no saved plan comparison.");
            return;
        };
        let identity = match crate::domain::ArtifactIdentity::from_markdown(&current) {
            Ok(Some(identity)) => identity,
            Ok(None) => {
                self.toasts.danger("The feature has no stable identity.");
                return;
            }
            Err(error) => {
                self.toasts
                    .danger(format!("Cannot read feature identity: {error}"));
                return;
            }
        };
        let (adr_path, adr_content) = match crate::artifacts::koolade::prepare_plan_choice_record(
            store,
            id,
            &identity.uid,
            &comparison,
            plan_id,
        ) {
            Ok(record) => record,
            Err(error) => {
                self.toasts
                    .danger(format!("Cannot prepare decision record: {error}"));
                return;
            }
        };
        let updated = match crate::domain::ChangeMetadata::adopt_typed_plan(
            &current,
            &comparison,
            plan_id,
            Some(&adr_path),
        ) {
            Ok(updated) => updated,
            Err(error) => {
                self.toasts.danger(format!("Cannot adopt plan: {error}"));
                return;
            }
        };
        let mut workflow = match crate::artifacts::task_docs::load_workflow(store) {
            Ok(workflow) => workflow,
            Err(error) => {
                self.toasts
                    .danger(format!("Cannot load approval state: {error}"));
                return;
            }
        };
        if let Some(record) = workflow.plan_comparisons.get_mut(id) {
            record.alternatives.selected_plan = Some(plan_id.to_owned());
            record.selected_plan = Some(plan_id.to_owned());
            record.status = crate::core::workflow::PlanComparisonStatus::Adopted;
            record.updated_at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis())
                .unwrap_or(record.updated_at_ms);
            if let Err(error) = record.validate() {
                self.toasts.danger(format!("Cannot adopt plan: {error}"));
                return;
            }
        } else {
            let record = crate::core::workflow::PlanComparisonRecord {
                schema_version: 1,
                feature_id: id.to_owned(),
                alternatives: crate::domain::PlanComparison {
                    selected_plan: Some(plan_id.to_owned()),
                    ..comparison.clone()
                },
                history: Vec::new(),
                transcript: String::new(),
                status: crate::core::workflow::PlanComparisonStatus::Adopted,
                selected_plan: Some(plan_id.to_owned()),
                updated_at_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis())
                    .unwrap_or_default(),
            };
            if let Err(error) = record.validate() {
                self.toasts.danger(format!("Cannot adopt plan: {error}"));
                return;
            }
            workflow.plan_comparisons.insert(id.to_owned(), record);
        }
        workflow.approved_features.remove(id);
        let adr_relative = adr_path;
        let (mut changes, workflow_checks) =
            match crate::artifacts::task_docs::workflow_record_changes(store, &workflow) {
                Ok(changes) => changes,
                Err(error) => {
                    self.toasts
                        .danger(format!("Cannot serialize approval state: {error}"));
                    return;
                }
            };
        match crate::core::apply::private_document_draft_changes(
            store,
            &[(format!("feature:{id}"), updated.clone())],
        ) {
            Ok(draft_changes) => changes.extend(draft_changes),
            Err(error) => {
                self.toasts
                    .danger(format!("Cannot save selected plan: {error}"));
                return;
            }
        }
        changes.push((adr_relative, adr_content.into_bytes()));
        let committed_revision = match store.transaction_with_revision_and_record_revisions(
            &changes,
            Some(&project.state.baseline_planning_revision),
            &workflow_checks,
        ) {
            Ok((_, revision)) => revision,
            Err(error) => {
                self.toasts
                    .danger(format!("Plan adoption was rolled back: {error}"));
                return;
            }
        };
        project.state.workflow = match crate::artifacts::task_docs::load_workflow(store) {
            Ok(workflow) => workflow,
            Err(error) => {
                self.toasts.danger(format!(
                    "Plan was saved but workflow reload failed: {error}"
                ));
                return;
            }
        };
        project.state.baseline_planning_revision = committed_revision;
        if let Some((_, feature_body)) = project
            .state
            .active_features
            .iter_mut()
            .find(|(feature_id, _)| feature_id == id)
        {
            *feature_body = updated.clone();
        }
        if project
            .state
            .active_feature
            .as_ref()
            .is_some_and(|(feature_id, _)| feature_id == id)
        {
            project.state.active_feature = Some((id.to_owned(), updated));
        }
        let commit = store.revision();
        drop(guard);
        match commit {
            Ok(_) => self.toasts.success(format!(
                "Plan {plan_id} adopted for {id}; its ADR is saved and approval is now available."
            )),
            Err(error) => self.toasts.warning(format!(
                "Plan {plan_id} was adopted with its ADR, but local state could not be reread: {error}"
            )),
        }
    }
}
