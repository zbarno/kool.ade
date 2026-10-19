use super::RuntimeExecution;
use crate::execution::contract::{ExecutionEventKindV1, ExecutionEventV1};

impl RuntimeExecution {
    pub(in crate::execution::docker::backend::lifecycle) fn fail_start(
        &mut self,
        failure: crate::execution::errors::ExecutionError,
    ) -> Result<(), crate::execution::errors::ExecutionError> {
        if self.terminal {
            return Ok(());
        }
        self.result = Some(super::super::control::result(
            &self.request.execution_id,
            Some(failure),
            None,
            false,
            false,
        ));
        self.terminal = true;
        self.host_capacity_lease.take();
        self.push(ExecutionEventKindV1::Lifecycle {
            state: crate::execution::contract::ExecutionLifecycleV1::Failed,
        })
    }

    pub(in crate::execution::docker::backend::lifecycle) fn set_lifecycle_if_changed(
        &mut self,
        state: crate::execution::contract::ExecutionLifecycleV1,
    ) -> Result<(), crate::execution::errors::ExecutionError> {
        if self.terminal
            || matches!(
                self.lifecycle_state,
                crate::execution::contract::ExecutionLifecycleV1::Succeeded
                    | crate::execution::contract::ExecutionLifecycleV1::Failed
                    | crate::execution::contract::ExecutionLifecycleV1::Cancelled
                    | crate::execution::contract::ExecutionLifecycleV1::Interrupted
            )
        {
            return Err(crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::PolicyDenied,
                "A terminal execution cannot change lifecycle state",
                false,
            ));
        }
        if self.lifecycle_state != state {
            self.push(ExecutionEventKindV1::Lifecycle { state })?;
        }
        Ok(())
    }

    pub(in crate::execution::docker::backend::lifecycle) fn mark_collecting(
        &mut self,
    ) -> Result<(), crate::execution::errors::ExecutionError> {
        if self.terminal {
            return Err(crate::execution::errors::ExecutionError::new(
                crate::execution::errors::ExecutionErrorKind::PolicyDenied,
                "A terminal execution cannot collect results",
                false,
            ));
        }
        if self.lifecycle_state != crate::execution::contract::ExecutionLifecycleV1::Collecting {
            self.push(ExecutionEventKindV1::Lifecycle {
                state: crate::execution::contract::ExecutionLifecycleV1::Collecting,
            })?;
        }
        Ok(())
    }

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
