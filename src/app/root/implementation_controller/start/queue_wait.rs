use crate::app::session::Project;

pub(super) fn take(
    project: &mut Project,
    ticket: &str,
    now_unix_ms: u64,
) -> (Option<String>, Option<u64>) {
    let task_id = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
        .and_then(|document| document.metadata.as_ref())
        .map(|metadata| metadata.uid.clone());
    let wait_ms = project.queue.take_queue_wait_ms(ticket, now_unix_ms);
    (task_id, wait_ms)
}

pub(super) fn record(task_id: Option<&str>, wait_ms: Option<u64>) {
    if let Some(task_id) = task_id {
        let _ = crate::persistence::remote_execution_telemetry::set_task_queue_wait_ms(
            task_id, wait_ms,
        );
    }
}
