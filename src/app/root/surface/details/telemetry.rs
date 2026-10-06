use super::super::super::Project;

pub(super) fn reports(
    project: &Project,
    ticket: &str,
) -> (
    Option<crate::persistence::telemetry::ImplementationMetrics>,
    Option<crate::persistence::telemetry::ImplementationMetrics>,
) {
    let (records, _) = crate::persistence::telemetry::load(&project.chat_slug);
    let document = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket);
    let task_id = project
        .implementation_states
        .get(ticket)
        .and_then(|record| record.task_uid.clone())
        .or_else(|| document.and_then(|document| document.metadata.as_ref().map(|m| m.uid.clone())))
        .or_else(|| {
            document.and_then(|document| document.identity.as_ref().map(|id| id.uid.clone()))
        });
    let task_metrics = task_id.as_deref().and_then(|task_id| {
        records
            .iter()
            .any(|record| record.task.as_deref() == Some(task_id))
            .then(|| crate::persistence::telemetry::report::for_task(&records, task_id))
    });
    let feature_id = document
        .and_then(|document| {
            document
                .text
                .lines()
                .find_map(|line| line.strip_prefix("Feature: "))
        })
        .and_then(|value| {
            crate::core::workflow::feature_ids_in(value)
                .into_iter()
                .next()
        });
    let feature_metrics = feature_id.as_deref().and_then(|feature| {
        records
            .iter()
            .any(|record| record.feature.as_deref() == Some(feature))
            .then(|| crate::persistence::telemetry::report::for_feature(&records, feature))
    });
    (task_metrics, feature_metrics)
}
