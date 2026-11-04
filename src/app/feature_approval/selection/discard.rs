use super::super::super::*;

impl KooladeApp {
    pub(in crate::app) fn discard_feature_plans(&mut self, id: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some() {
            self.toasts
                .warning("Wait for the comparison turn to finish before discarding it.");
            return;
        }
        let Some(mut record) = project.state.workflow.plan_comparisons.get(id).cloned() else {
            self.toasts
                .warning("This feature has no locally saved plan comparison to discard.");
            return;
        };
        if record.status != crate::core::workflow::PlanComparisonStatus::Proposed {
            self.toasts
                .warning("An adopted plan cannot be discarded; start a new feature revision.");
            return;
        }

        record.status = crate::core::workflow::PlanComparisonStatus::Discarded;
        record.selected_plan = None;
        record.alternatives.selected_plan = None;
        record.updated_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or(record.updated_at_ms);
        if let Err(error) = record.validate() {
            self.toasts
                .danger(format!("Cannot discard comparison: {error}"));
            return;
        }

        let _guard = crate::core::writer_gate::acquire();
        let store = &project.state.planning_store;
        let mut workflow = project.state.workflow.clone();
        workflow.plan_comparisons.insert(id.to_owned(), record);
        let (changes, checks) =
            match crate::artifacts::task_docs::workflow_record_changes(store, &workflow) {
                Ok(changes) => changes,
                Err(error) => {
                    self.toasts
                        .danger(format!("Cannot save comparison: {error}"));
                    return;
                }
            };
        let revision = match store.transaction_with_revision_and_record_revisions(
            &changes,
            Some(&project.state.baseline_planning_revision),
            &checks,
        ) {
            Ok((_, revision)) => revision,
            Err(error) => {
                self.toasts
                    .danger(format!("Discard was rolled back: {error}"));
                return;
            }
        };
        project.state.workflow = match crate::artifacts::task_docs::load_workflow(store) {
            Ok(workflow) => workflow,
            Err(error) => {
                self.toasts.danger(format!(
                    "Discard was saved but workflow reload failed: {error}"
                ));
                return;
            }
        };
        project.state.baseline_planning_revision = revision;
        self.toasts.success(format!(
            "Comparison discarded for {id}; the prior transcript is retained."
        ));
    }
}
