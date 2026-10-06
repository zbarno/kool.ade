use super::Project;

impl Project {
    pub fn task_cancelled(&self, ticket: &str) -> bool {
        self.task_documents
            .iter()
            .find(|doc| doc.path == ticket)
            .is_some_and(|doc| {
                if self.implementation_states.get(ticket).is_some_and(|state| {
                    state.status == crate::core::implementation::ImplementationStatus::Completed
                        || state.pr_state
                            == Some(crate::core::implementation::PullRequestState::Merged)
                }) {
                    return false;
                }
                if self
                    .cancelled_work
                    .contains(&crate::persistence::cancelled_work::task_id(doc))
                {
                    return true;
                }
                let feature_id = doc
                    .text
                    .lines()
                    .find_map(|line| line.strip_prefix("Feature ID: "));
                let Some(feature_id) = feature_id else {
                    return false;
                };
                self.planning_work.iter().any(|work| {
                    work.feature_id.as_deref() == Some(feature_id)
                        && self
                            .cancelled_work
                            .contains(&crate::persistence::cancelled_work::planning_id(&work.uid))
                }) || self.state.active_features.iter().any(|(id, body)| {
                    id == feature_id
                        && crate::domain::ArtifactIdentity::from_markdown(body)
                            .ok()
                            .flatten()
                            .is_some_and(|identity| {
                                self.cancelled_work.contains(
                                    &crate::persistence::cancelled_work::planning_id(&identity.uid),
                                )
                            })
                })
            })
    }

    pub fn cancelled_task_paths(&self) -> std::collections::BTreeSet<String> {
        self.task_documents
            .iter()
            .filter(|doc| self.task_cancelled(&doc.path))
            .map(|doc| doc.path.clone())
            .collect()
    }
}
