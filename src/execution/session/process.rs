use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use crate::execution::{
    backend::{ExecutionBackend, ExecutionHandleV1, WorkspaceLeaseV1},
    errors::ExecutionError,
};

use super::{ExecutionOutput, ExecutionSession, RequestParameters, internal_error, policy_error};

struct PersistentExecution {
    backend: Arc<dyn ExecutionBackend>,
    handle: ExecutionHandleV1,
    lease: WorkspaceLeaseV1,
    runtime: tokio::runtime::Runtime,
}

/// App-owned reference to a successfully started process. Its execution
/// container remains reserved until `stop` or the last handle is dropped.
pub struct RuntimeProcessHandle {
    execution: Mutex<Option<PersistentExecution>>,
    active: AtomicBool,
    logs: String,
}

impl RuntimeProcessHandle {
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Startup logs are bounded by the execution session's output budget.
    pub fn logs(&self) -> &str {
        &self.logs
    }

    /// Stops the service by releasing its owned execution container.
    pub fn stop(&self) -> Result<(), ExecutionError> {
        let mut execution = self.execution.lock().map_err(|_| internal_error())?;
        let Some(owned) = execution.as_ref() else {
            return Ok(());
        };
        owned.runtime.block_on(
            owned
                .backend
                .release_persistent(&owned.handle, &owned.lease),
        )?;
        execution.take();
        self.active.store(false, Ordering::Release);
        Ok(())
    }
}

impl Drop for RuntimeProcessHandle {
    fn drop(&mut self) {
        let Ok(execution) = self.execution.get_mut() else {
            return;
        };
        if let Some(owned) = execution.take() {
            let _ = owned.runtime.block_on(
                owned
                    .backend
                    .release_persistent(&owned.handle, &owned.lease),
            );
        }
        self.active.store(false, Ordering::Release);
    }
}

impl ExecutionSession {
    /// Starts a bounded launcher script and retains the execution container
    /// when it exits successfully. The script must perform its readiness check
    /// before exiting; a failed or timed out launcher is cleaned up normally.
    pub fn start_runtime_process(
        &self,
        script: &str,
        timeout: Duration,
    ) -> Result<(ExecutionOutput, Option<Arc<RuntimeProcessHandle>>), ExecutionError> {
        self.start_runtime_process_with_artifacts(script, timeout, &[])
    }

    /// Starts a bounded launcher and records hashes for selected workspace
    /// artifacts in the execution result.
    pub fn start_runtime_process_with_artifacts(
        &self,
        script: &str,
        timeout: Duration,
        artifact_paths: &[String],
    ) -> Result<(ExecutionOutput, Option<Arc<RuntimeProcessHandle>>), ExecutionError> {
        self.verify_authorization()?;
        if script.len() > crate::execution::contract::MAX_SCRIPT_BYTES
            || script.contains('\0')
            || timeout.is_zero()
            || timeout > Duration::from_secs(1200)
        {
            return Err(policy_error(
                "Runtime startup script or timeout is outside limits",
            ));
        }
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(policy_error(
                "An execution is already active in this session",
            ));
        }
        self.cancel_requested.store(false, Ordering::Release);
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let cancellation_generation = self.cancellation_generation.fetch_add(1, Ordering::AcqRel);
        let request = self.request(RequestParameters {
            argv: vec!["sh".into(), "-s".into()],
            stdin: Some(script.to_owned()),
            working_directory_relative: ".".into(),
            environment_allowlist: Default::default(),
            timeout,
            request_sequence: sequence,
            cancellation_generation,
            policy_override: None,
            artifact_paths: artifact_paths.to_vec(),
        });
        let request = match request {
            Ok(request) => request,
            Err(error) => {
                self.busy.store(false, Ordering::Release);
                return Err(error);
            }
        };
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(_) => {
                self.busy.store(false, Ordering::Release);
                return Err(internal_error());
            }
        };
        let result = self.runtime.block_on(self.run_persistent_request(request));
        self.clear_active();
        let (output, handle) = result?;
        let Some((handle, lease)) = handle else {
            self.verify_authorization()?;
            return Ok((output, None));
        };
        let process = Arc::new(RuntimeProcessHandle {
            execution: Mutex::new(Some(PersistentExecution {
                backend: self.backend.clone(),
                handle,
                lease,
                runtime,
            })),
            active: AtomicBool::new(true),
            logs: output.stdout.clone() + &output.stderr,
        });
        self.runtime_processes
            .lock()
            .map_err(|_| internal_error())?
            .push(process.clone());
        if let Err(error) = self.verify_authorization() {
            let _ = process.stop();
            return Err(error);
        }
        Ok((output, Some(process)))
    }

    /// Stops every process owned by this execution session.
    pub fn stop_runtime_processes(&self) -> Result<(), ExecutionError> {
        let processes = std::mem::take(
            &mut *self
                .runtime_processes
                .lock()
                .map_err(|_| internal_error())?,
        );
        let mut first_error = None;
        for process in processes {
            if let Err(error) = process.stop() {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}
