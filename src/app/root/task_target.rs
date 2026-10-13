use super::session::Project;

pub(super) fn key(project: &Project, ticket: &str) -> String {
    let implementation_uid = project
        .implementation_states
        .get(ticket)
        .and_then(|state| state.task_uid.as_deref());
    let artifact_uid = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
        .and_then(|document| document.metadata.as_ref())
        .map(|metadata| metadata.uid.as_str());
    from_ids(implementation_uid, artifact_uid, ticket)
}

fn from_ids(implementation_uid: Option<&str>, artifact_uid: Option<&str>, ticket: &str) -> String {
    implementation_uid
        .or(artifact_uid)
        .unwrap_or(ticket)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn planning_task_preferences_use_stable_task_identity() {
        assert_eq!(
            from_ids(
                None,
                Some("00000000-0000-4000-8000-000000000001"),
                "tasks/feature.md"
            ),
            "00000000-0000-4000-8000-000000000001"
        );
        assert_eq!(
            from_ids(
                Some("00000000-0000-4000-8000-000000000004"),
                Some("00000000-0000-4000-8000-000000000001"),
                "tasks/feature.md"
            ),
            "00000000-0000-4000-8000-000000000004"
        );
        assert_eq!(from_ids(None, None, "tasks/legacy.md"), "tasks/legacy.md");
    }
}
