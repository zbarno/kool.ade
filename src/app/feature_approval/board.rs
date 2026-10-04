use super::super::{KooladeApp, Screen};

impl KooladeApp {
    pub(crate) fn approve_feature_from_board(&mut self, id: &str) {
        self.approve_feature_only(id);
        let Screen::Connected(project) = &mut self.screen else {
            return;
        };
        let approved = project
            .state
            .workflow
            .approved_features
            .get(id)
            .is_some_and(|saved| {
                project
                    .state
                    .active_features
                    .iter()
                    .find(|(feature_id, _)| feature_id == id)
                    .is_some_and(|(_, body)| {
                        *saved == crate::core::workflow::feature_contract(body)
                    })
            });
        if !approved {
            return;
        }
        let Some((_, body)) = project
            .state
            .active_features
            .iter()
            .find(|(feature_id, _)| feature_id == id)
        else {
            return;
        };
        let Ok(Some(identity)) = crate::domain::ArtifactIdentity::from_markdown(body) else {
            return;
        };
        if project.planning_work.iter().any(|work| {
            work.kind == crate::core::planning_work::WorkKind::TaskGeneration
                && work.feature_uid.as_deref() == Some(identity.uid.as_str())
                && work.status != crate::core::planning_work::WorkStatus::NeedsAttention
        }) {
            return;
        }
        let title = body
            .lines()
            .next()
            .unwrap_or(id)
            .trim_start_matches('#')
            .trim();
        let mut work = crate::core::planning_work::Work::new(
            String::new(),
            format!("Generate tasks: {title}"),
            format!("Generate implementation tasks for approved feature {id}."),
            "Ready to generate implementation tasks".into(),
        );
        work.kind = crate::core::planning_work::WorkKind::TaskGeneration;
        work.status = crate::core::planning_work::WorkStatus::Todo;
        work.feature_id = Some(id.to_owned());
        work.feature_uid = Some(identity.uid);
        work.key = format!("task-generation:{}", work.uid);
        project.planning_work.push(work);
        if let Err(error) =
            crate::core::planning_work::save(&project.state.repo_root, &project.planning_work)
        {
            project.planning_work.pop();
            self.toasts.danger(format!(
                "Feature was approved, but the task generation card could not be saved: {error}"
            ));
            return;
        }
        self.toasts.info(format!(
            "{id} approved. Generate tasks from the new Kanban card."
        ));
    }
}
