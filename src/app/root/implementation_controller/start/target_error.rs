pub(super) fn message(ticket: &str, error: &anyhow::Error) -> String {
    if let Some(repository_id) = error
        .to_string()
        .strip_prefix("No local checkout mapped for ")
    {
        format!(
            "Cannot start {ticket}: code repository {repository_id} has no local checkout. Open Workspace settings > Code repositories and map its checkout before implementation."
        )
    } else {
        format!("Cannot start {ticket}: {error}")
    }
}
