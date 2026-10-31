pub(super) fn message(ticket: &str, error: &anyhow::Error) -> String {
    if let Some(repository_id) = error
        .to_string()
        .strip_prefix("No local checkout mapped for ")
    {
        format!(
            "Cannot start {ticket}: Kool.ad/e has no managed clone for code repository {repository_id}. Clone the repository through the welcome screen before implementation."
        )
    } else {
        format!("Cannot start {ticket}: {error}")
    }
}
