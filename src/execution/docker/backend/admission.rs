use super::{DockerExecutionBackend, internal_error_with_context, starting_guard::StartingGuard};
use crate::execution::errors::{ExecutionError, ExecutionErrorKind};

impl DockerExecutionBackend {
    pub(super) fn reserve_start(
        &self,
        execution_id: &str,
        cpu_millis: u32,
        memory_bytes: u64,
    ) -> Result<StartingGuard<'_>, ExecutionError> {
        let mut starting = self
            .starting
            .lock()
            .map_err(|_| internal_error_with_context("reserving an execution start"))?;
        let executions = self
            .executions
            .lock()
            .map_err(|_| internal_error_with_context("checking the execution registry"))?;
        if executions.contains_key(execution_id) || starting.contains_key(execution_id) {
            return Err(ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "Execution ID is already active",
                false,
            ));
        }
        let cpu_capacity = crate::execution::resources::effective_cpu_capacity_millis();
        let memory_capacity = crate::execution::resources::effective_memory_capacity_bytes();
        let host_capacity_lease = Some(acquire_local_capacity_lease(
            execution_id,
            cpu_millis,
            memory_bytes,
            cpu_capacity,
            memory_capacity,
        )?);
        starting.insert(execution_id.to_owned(), (cpu_millis, memory_bytes));
        Ok(StartingGuard {
            starting: &self.starting,
            execution_id: execution_id.to_owned(),
            host_capacity_lease,
        })
    }
}

fn acquire_local_capacity_lease(
    execution_id: &str,
    cpu_millis: u32,
    memory_bytes: u64,
    cpu_capacity: Option<u32>,
    memory_capacity: Option<u64>,
) -> Result<crate::execution::remote::WorkerCapacityLease, ExecutionError> {
    let unknown = cpu_capacity.is_none() || memory_capacity.is_none();
    let pool = if unknown {
        crate::execution::remote::WorkerCapacityPool::new_local_unknown(
            &crate::persistence::state_root(),
        )
    } else {
        crate::execution::remote::WorkerCapacityPool::new_local(&crate::persistence::state_root())
    }
    .map_err(|_| capacity_error(unknown))?;
    let reservation = if unknown {
        pool.acquire_unknown_local()
    } else {
        pool.acquire_resources(
            execution_id,
            cpu_millis,
            memory_bytes,
            cpu_capacity.expect("known CPU capacity"),
            memory_capacity.expect("known memory capacity"),
        )
    };
    reservation.map_err(|_| capacity_error(unknown))
}

fn capacity_error(unknown: bool) -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::EnvironmentUnavailable,
        if unknown {
            crate::execution::errors::LOCAL_CAPACITY_UNKNOWN_REJECTION
        } else {
            crate::execution::errors::LOCAL_START_CAPACITY_REJECTION
        },
        true,
    )
}

#[cfg(test)]
#[path = "admission/tests.rs"]
mod tests;
