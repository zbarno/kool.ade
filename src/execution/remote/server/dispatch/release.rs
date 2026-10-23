use super::{
    requests::HandleRequest,
    support::{decode, json_value, require_execution_id},
};
use crate::execution::remote::protocol::{EnvelopeKindV1 as Kind, EnvelopeV1};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

pub(super) fn dispatch(
    backend: Arc<dyn crate::execution::backend::ExecutionBackend>,
    stager: Option<Arc<crate::execution::remote::staging::WorkerStager>>,
    capacity_leases: Arc<
        Mutex<std::collections::HashMap<String, crate::execution::remote::WorkerCapacityLease>>,
    >,
    runtime: &tokio::runtime::Runtime,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), crate::execution::errors::ExecutionError> {
    let request: HandleRequest = decode(&envelope.payload)?;
    require_execution_id(envelope, &request.handle.execution_id)?;
    runtime.block_on(backend.release(&request.handle))?;

    let execution_id = &request.handle.execution_id;
    let task_id = &request.handle.task_id;
    let target_id = &request.handle.target.target_id;
    let phase_timings = crate::persistence::execution_records::load(task_id, execution_id)
        .ok()
        .flatten()
        .filter(|record| {
            record.execution_id == *execution_id
                && record.task_id == *task_id
                && record.target_id == *target_id
        })
        .map(|record| record.phase_timings);

    capacity_leases
        .lock()
        .map_err(|_| super::super::incompatible("worker capacity registry is unavailable"))?
        .remove(execution_id);
    if let Some(stager) = &stager {
        stager.release_execution(execution_id).map_err(|_| {
            super::super::incompatible("execution generation could not be released")
        })?;
    }
    json_value(json!({"released": true, "phase_timings": phase_timings}))
        .map(|value| (Kind::Acknowledge, value))
}
