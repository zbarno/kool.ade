use std::sync::{
    Mutex,
    atomic::{AtomicI32, Ordering},
};

use crate::execution::{
    backend::{
        BackendFuture, BackendProbeV1, ExecutionBackend, ExecutionHandleV1, WorkspaceLeaseV1,
    },
    contract::{
        EventPageRequestV1, ExecutionEventKindV1, ExecutionEventV1, ExecutionLifecycleV1,
        ExecutionRequestV1, ExecutionResultV1, ExecutionTarget, ResourceMetricsV1, WorkspaceRef,
    },
};

#[derive(Default)]
pub(super) struct CapturingDockerBackend {
    request: Mutex<Option<ExecutionRequestV1>>,
    exit_code: AtomicI32,
}

impl CapturingDockerBackend {
    pub fn request(&self) -> ExecutionRequestV1 {
        self.request.lock().expect("request lock").clone().unwrap()
    }

    pub fn clear_request(&self) {
        *self.request.lock().expect("request lock") = None;
    }

    pub fn has_request(&self) -> bool {
        self.request.lock().expect("request lock").is_some()
    }

    pub fn set_exit_code(&self, exit_code: i32) {
        self.exit_code.store(exit_code, Ordering::Relaxed);
    }
}

impl ExecutionBackend for CapturingDockerBackend {
    fn probe(&self, _: &ExecutionTarget) -> BackendFuture<'_, BackendProbeV1> {
        Box::pin(async {
            Ok(BackendProbeV1 {
                wire_version: 1,
                available: true,
                compatible: true,
                architecture: Some("amd64".into()),
                failure: None,
                capabilities: None,
            })
        })
    }

    fn prepare_workspace<'a>(
        &'a self,
        workspace: &'a WorkspaceRef,
        _: &'a ExecutionTarget,
    ) -> BackendFuture<'a, WorkspaceLeaseV1> {
        Box::pin(async move {
            Ok(WorkspaceLeaseV1 {
                lease_id: uuid::Uuid::new_v4().hyphenated().to_string(),
                workspace: workspace.clone(),
                expires_unix_ms: now_ms() + 60_000,
            })
        })
    }

    fn start<'a>(
        &'a self,
        _: &'a WorkspaceLeaseV1,
        request: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ExecutionHandleV1> {
        *self.request.lock().expect("request lock") = Some(request.clone());
        Box::pin(async move {
            Ok(ExecutionHandleV1 {
                execution_id: request.execution_id.clone(),
                task_id: request.task_id.clone(),
                target: request.target.clone(),
                request_sequence: request.request_sequence,
            })
        })
    }

    fn release_workspace<'a>(&'a self, _: &'a WorkspaceLeaseV1) -> BackendFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn exec<'a>(
        &'a self,
        _: &'a ExecutionHandleV1,
        _: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn events<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
        page: EventPageRequestV1,
    ) -> BackendFuture<'a, Vec<ExecutionEventV1>> {
        let terminal_state = if self.exit_code.load(Ordering::Relaxed) == 0 {
            ExecutionLifecycleV1::Succeeded
        } else {
            ExecutionLifecycleV1::Failed
        };
        Box::pin(async move {
            Ok([
                ExecutionLifecycleV1::Queued,
                ExecutionLifecycleV1::Preparing,
                ExecutionLifecycleV1::Running,
                ExecutionLifecycleV1::Collecting,
                terminal_state,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, state)| ExecutionEventV1 {
                schema_version: 1,
                execution_id: handle.execution_id.clone(),
                sequence: index as u64 + 1,
                timestamp_unix_ms: 10_000 + index as u64,
                event: ExecutionEventKindV1::Lifecycle { state },
            })
            .filter(|event| event.sequence > page.after_sequence)
            .take(usize::from(page.max_events))
            .collect())
        })
    }

    fn cancel<'a>(&'a self, _: &'a ExecutionHandleV1, _: u64) -> BackendFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn collect_artifacts<'a>(
        &'a self,
        handle: &'a ExecutionHandleV1,
    ) -> BackendFuture<'a, ExecutionResultV1> {
        let exit_code = self.exit_code.load(Ordering::Relaxed);
        Box::pin(async move {
            Ok(ExecutionResultV1 {
                schema_version: 1,
                execution_id: handle.execution_id.clone(),
                exit_code: Some(exit_code),
                signal: None,
                timed_out: false,
                cancelled: false,
                metrics: ResourceMetricsV1 {
                    cpu_millis: 1,
                    peak_memory_bytes: 1,
                    process_count: 1,
                    disk_bytes: 1,
                },
                phase_timings: Default::default(),
                artifacts: Vec::new(),
                failure: None,
            })
        })
    }

    fn release<'a>(&'a self, _: &'a ExecutionHandleV1) -> BackendFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
