use super::RuntimeExecution;
use crate::execution::contract::{ExecutionEventKindV1, ExecutionEventV1};

impl RuntimeExecution {
    pub(in crate::execution::docker::backend::lifecycle) fn push(
        &mut self,
        event: ExecutionEventKindV1,
    ) -> Result<(), crate::execution::errors::ExecutionError> {
        let is_lifecycle = matches!(event, ExecutionEventKindV1::Lifecycle { .. });
        let recorded = ExecutionEventV1 {
            schema_version: 1,
            execution_id: self.request.execution_id.clone(),
            sequence: self.next_event_sequence,
            timestamp_unix_ms: crate::execution::docker::backend::now_unix_ms().max(1),
            event,
        };
        crate::persistence::execution_records::append_event(&self.request.task_id, &recorded)
            .map_err(|_| {
                crate::execution::errors::ExecutionError::new(
                    crate::execution::errors::ExecutionErrorKind::ExecutionFailed,
                    "Execution event could not be durably recorded",
                    false,
                )
            })?;
        if let ExecutionEventKindV1::Lifecycle { state } = &recorded.event {
            self.lifecycle_state = *state;
        }
        self.events.push(recorded);
        self.next_event_sequence = self.next_event_sequence.saturating_add(1);
        if self.events.len() > 256 {
            self.events.remove(0);
        }
        if is_lifecycle {
            self.persist()?;
        }
        Ok(())
    }
}
