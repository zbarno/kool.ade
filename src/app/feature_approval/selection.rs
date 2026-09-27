use super::super::*;

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
        let path = match crate::artifacts::product_docs::document_path(
            &project.state.repo_root,
            &format!("feature:{id}"),
        ) {
            Ok(path) => path,
            Err(error) => {
                self.toasts.danger(format!("Cannot select plan: {error}"));
                return;
            }
        };
        let guard = crate::core::writer_gate::acquire();
        let current = match crate::artifacts::read_utf8_lossy(&path) {
            Ok(current) if current == *body => current,
            Ok(_) => {
                self.toasts
                    .warning("The feature changed. Reopen it and review the current plans.");
                return;
            }
            Err(error) => {
                self.toasts.danger(format!("Cannot read feature: {error}"));
                return;
            }
        };
        let metadata = match crate::domain::ChangeMetadata::require_markdown(&current) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.toasts
                    .danger(format!("Cannot read plan comparison: {error}"));
                return;
            }
        };
        let Some(comparison) = metadata.plan_comparison.as_ref() else {
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
        let (adr_path, adr_content) = match crate::artifacts::packet::prepare_plan_choice_record(
            &project.state.repo_root,
            id,
            &identity.uid,
            comparison,
            plan_id,
        ) {
            Ok(record) => record,
            Err(error) => {
                self.toasts
                    .danger(format!("Cannot prepare decision record: {error}"));
                return;
            }
        };
        let updated =
            match crate::domain::ChangeMetadata::adopt_plan(&current, plan_id, Some(&adr_path)) {
                Ok(updated) => updated,
                Err(error) => {
                    self.toasts.danger(format!("Cannot adopt plan: {error}"));
                    return;
                }
            };
        let mut workflow =
            match crate::artifacts::task_docs::load_workflow(&project.state.repo_root) {
                Ok(workflow) => workflow,
                Err(error) => {
                    self.toasts
                        .danger(format!("Cannot load approval state: {error}"));
                    return;
                }
            };
        workflow.approved_features.remove(id);
        let layout = crate::artifacts::layout::ArtifactLayout::new(&project.state.repo_root);
        let feature_rel = path
            .strip_prefix(&project.state.repo_root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let workflow_rel = layout
            .workflow_state()
            .strip_prefix(&project.state.repo_root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let changes = vec![
            (feature_rel.clone(), updated.clone()),
            (adr_path.clone(), adr_content),
            (
                workflow_rel.clone(),
                match serde_json::to_string_pretty(&workflow) {
                    Ok(json) => json,
                    Err(error) => {
                        self.toasts
                            .danger(format!("Cannot serialize approval state: {error}"));
                        return;
                    }
                },
            ),
        ];
        let committed_paths =
            match crate::artifacts::transaction::apply(&project.state.repo_root, &changes) {
                Ok(paths) => paths,
                Err(error) => {
                    self.toasts
                        .danger(format!("Plan adoption was rolled back: {error}"));
                    return;
                }
            };
        project.state.workflow = workflow;
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
            &format!("planner: adopt plan {plan_id} for {id}"),
            &committed_paths,
        );
        drop(guard);
        match commit {
            Ok(_) => self.toasts.success(format!(
                "Plan {plan_id} adopted for {id}; its ADR is saved and approval is now available."
            )),
            Err(error) => self.toasts.warning(format!(
                "Plan {plan_id} was adopted with its ADR, but the Git checkpoint failed: {error}"
            )),
        }
    }
}
