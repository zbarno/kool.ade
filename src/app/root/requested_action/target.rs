pub(super) fn task_matches(
    document: &crate::artifacts::task_docs::TaskDocument,
    target: &str,
) -> bool {
    document
        .identity
        .as_ref()
        .is_some_and(|identity| identity.uid == target || identity.display_id == target)
}

pub(super) fn task_title(
    documents: &[crate::artifacts::task_docs::TaskDocument],
    ticket: &str,
) -> String {
    documents
        .iter()
        .find(|document| document.path == ticket)
        .map(|document| document.title.clone())
        .unwrap_or_else(|| "the task".into())
}
