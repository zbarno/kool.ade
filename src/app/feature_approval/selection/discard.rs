use super::super::super::*;

impl PacketApp {
    pub(in crate::app) fn discard_feature_plans(&mut self, id: &str) {
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        if project.active_turn.is_some() {
            self.toasts
                .warning("Wait for the comparison turn to finish before discarding it.");
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
        if let Some(mut record) = project.state.workflow.plan_comparisons.get(id).cloned() {
            let guard = crate::core::writer_gate::acquire();
            record.status = crate::core::workflow::PlanComparisonStatus::Discarded;
            record.selected_plan = None;
            record.alternatives.selected_plan = None;
            record.updated_at_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis())
                .unwrap_or(record.updated_at_ms);
            let mut workflow = project.state.workflow.clone();
            workflow.plan_comparisons.insert(id.to_owned(), record);
            let layout = crate::artifacts::layout::ArtifactLayout::new(&project.state.repo_root);
            let path = layout.workflow_state();
            let relative = path
                .strip_prefix(&project.state.repo_root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let content = match serde_json::to_string_pretty(&workflow) {
                Ok(content) => content,
                Err(error) => {
                    self.toasts
                        .danger(format!("Cannot save comparison: {error}"));
                    return;
                }
            };
            let paths = match crate::artifacts::transaction::apply(
                &project.state.repo_root,
                &[(relative, content)],
            ) {
                Ok(paths) => paths,
                Err(error) => {
                    self.toasts
                        .danger(format!("Discard was rolled back: {error}"));
                    return;
                }
            };
            project.state.workflow = workflow;
            let commit = crate::core::gitops::commit(
                &project.state.repo_root,
                &format!("planner: discard plan comparison for {id}"),
                &paths,
            );
            drop(guard);
            match commit {
                Ok(_) => self
                    .toasts
                    .success(format!("Comparison discarded for {id}.")),
                Err(error) => self.toasts.warning(format!(
                    "Comparison was discarded, but its Git checkpoint failed: {error}"
                )),
            }
            return;
        }
        let path = match crate::artifacts::product_docs::document_path(
            &project.state.repo_root,
            &format!("feature:{id}"),
        ) {
            Ok(path) => path,
            Err(error) => {
                self.toasts
                    .danger(format!("Cannot locate feature: {error}"));
                return;
            }
        };
        let guard = crate::core::writer_gate::acquire();
        let current = match crate::artifacts::read_utf8_lossy(&path) {
            Ok(current) if current == *body => current,
            Ok(_) => {
                self.toasts
                    .warning("The feature changed. Reopen it and review the latest comparison.");
                return;
            }
            Err(error) => {
                self.toasts.danger(format!("Cannot read feature: {error}"));
                return;
            }
        };
        let updated = match crate::domain::ChangeMetadata::discard_plan_comparison(&current) {
            Ok(updated) => updated,
            Err(error) => {
                self.toasts
                    .danger(format!("Cannot discard comparison: {error}"));
                return;
            }
        };
        let relative = path
            .strip_prefix(&project.state.repo_root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let paths = match crate::artifacts::transaction::apply(
            &project.state.repo_root,
            &[(relative, updated.clone())],
        ) {
            Ok(paths) => paths,
            Err(error) => {
                self.toasts
                    .danger(format!("Discard was rolled back: {error}"));
                return;
            }
        };
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
        let commit = crate::core::gitops::commit(
            &project.state.repo_root,
            &format!("planner: discard plan comparison for {id}"),
            &paths,
        );
        drop(guard);
        match commit {
            Ok(_) => self.toasts.success(format!(
                "Comparison discarded for {id}; the prior transcript is retained."
            )),
            Err(error) => self.toasts.warning(format!(
                "Comparison was discarded, but its Git checkpoint failed: {error}"
            )),
        }
    }
}
