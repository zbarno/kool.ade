use crate::execution::{
    backend::ExecutionHandleV1,
    contract::{EventPageRequestV1, ExecutionEventV1},
    docker::backend::{DockerExecutionBackend, internal_error, invalid_request},
    errors::ExecutionError,
};

pub(in crate::execution::docker::backend::lifecycle) async fn events(
    backend: &DockerExecutionBackend,
    handle: &ExecutionHandleV1,
    page: EventPageRequestV1,
) -> Result<Vec<ExecutionEventV1>, ExecutionError> {
    if !(1..=256).contains(&page.max_events) {
        return Err(invalid_request());
    }
    super::load_record(backend, handle)?;
    crate::persistence::execution_records::load_events(&handle.task_id, &handle.execution_id, page)
        .map_err(|_| internal_error())
}
