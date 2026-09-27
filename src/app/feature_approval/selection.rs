use super::super::*;

impl PacketApp {
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
        let updated = match crate::domain::ChangeMetadata::select_plan(&current, plan_id) {
            Ok(updated) => updated,
            Err(error) => {
                self.toasts.danger(format!("Cannot select plan: {error}"));
                return;
            }
        };
        if let Err(error) = crate::artifacts::atomic_write(&path, &updated) {
            self.toasts
                .danger(format!("Cannot save plan choice: {error}"));
            return;
        }
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
        let relative = match path.strip_prefix(&project.state.repo_root) {
            Ok(path) => path.to_string_lossy().replace('\\', "/"),
            Err(error) => {
                self.toasts.danger(format!(
                    "Plan choice saved but checkpoint path failed: {error}"
                ));
                return;
            }
        };
        let commit = crate::core::gitops::commit(
            &project.state.repo_root,
            &format!("planner: choose plan {plan_id} for {id}"),
            &[relative],
        );
        drop(guard);
        match commit {
            Ok(_) => self.toasts.success(format!(
                "Plan {plan_id} selected for {id}. You can now approve the feature."
            )),
            Err(error) => self.toasts.warning(format!(
                "Plan {plan_id} was saved, but its Git checkpoint failed: {error}"
            )),
        }
    }
}
