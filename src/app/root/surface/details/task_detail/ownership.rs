use super::*;

pub(super) fn cancelled(app: &KooladeApp, ticket: &str) -> bool {
    match &app.screen {
        Screen::Connected(project) => project
            .task_documents
            .iter()
            .find(|doc| doc.path == ticket)
            .is_some_and(|doc| {
                project
                    .cancelled_work
                    .contains(&crate::persistence::cancelled_work::task_id(doc))
            }),
        Screen::Welcome => false,
    }
}

pub(super) fn intent(app: &KooladeApp, ticket: &str) -> Option<String> {
    let Screen::Connected(project) = &app.screen else {
        return None;
    };
    let document = project
        .task_documents
        .iter()
        .find(|doc| doc.path == ticket)?;
    crate::artifacts::task_docs::task_intent_uid_for_document(
        document,
        &project.task_documents,
        &project.planning_work,
    )
}
