use super::*;

#[path = "tests/authorization.rs"]
mod authorization;
#[path = "tests/persistent.rs"]
mod persistent;
#[path = "tests/validation.rs"]
mod validation;
use crate::execution::{
    backend::{BackendFuture, BackendProbeV1, WorkspaceLeaseV1},
    contract::{
        EventPageRequestV1, ExecutionEventKindV1, ExecutionEventV1, ExecutionLifecycleV1,
        ResourceMetricsV1,
    },
};

struct FakeBackend {
    request: Mutex<Option<ExecutionRequestV1>>,
    released: AtomicBool,
    workspace_released: AtomicBool,
    cancelled: AtomicBool,
    expired_lease: AtomicBool,
    events: Mutex<Vec<ExecutionEventV1>>,
    result: Mutex<ExecutionResultV1>,
    prepare_delay: Mutex<std::time::Duration>,
    invalidate_authorization_on_exec: Mutex<Option<Arc<AtomicBool>>>,
    invalidate_authorization_on_events: Mutex<Option<Arc<AtomicBool>>>,
    lifecycle_updates: Mutex<Vec<ExecutionLifecycleV1>>,
    workspace_result_applied: AtomicBool,
    start_error: Mutex<Option<ExecutionError>>,
}

impl ExecutionBackend for FakeBackend {
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
            let delay = *self.prepare_delay.lock().unwrap();
            tokio::time::sleep(delay).await;
            let expires_unix_ms = if self.expired_lease.load(Ordering::Acquire) {
                0
            } else {
                now_unix_ms().saturating_add(60_000)
            };
            Ok(WorkspaceLeaseV1 {
                lease_id: uuid::Uuid::new_v4().to_string(),
                workspace: workspace.clone(),
                expires_unix_ms,
            })
        })
    }

    fn start<'a>(
        &'a self,
        _: &'a WorkspaceLeaseV1,
        request: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ExecutionHandleV1> {
        Box::pin(async move {
            if let Some(error) = self.start_error.lock().unwrap().take() {
                return Err(error);
            }
            *self.request.lock().unwrap() = Some(request.clone());
            for event in self.events.lock().unwrap().iter_mut() {
                if event.execution_id == execution_id() {
                    event.execution_id.clone_from(&request.execution_id);
                }
            }
            self.result
                .lock()
                .unwrap()
                .execution_id
                .clone_from(&request.execution_id);
            Ok(ExecutionHandleV1 {
                execution_id: request.execution_id.clone(),
                task_id: request.task_id.clone(),
                target: request.target.clone(),
                request_sequence: request.request_sequence,
            })
        })
    }

    fn set_lifecycle_state(
        &self,
        _: &ExecutionHandleV1,
        state: ExecutionLifecycleV1,
    ) -> Result<(), ExecutionError> {
        self.lifecycle_updates.lock().unwrap().push(state);
        Ok(())
    }

    fn release_workspace<'a>(&'a self, _: &'a WorkspaceLeaseV1) -> BackendFuture<'a, ()> {
        self.workspace_released.store(true, Ordering::Release);
        Box::pin(async { Ok(()) })
    }

    fn exec<'a>(
        &'a self,
        _: &'a ExecutionHandleV1,
        _: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ()> {
        if let Some(invalidated) = self
            .invalidate_authorization_on_exec
            .lock()
            .unwrap()
            .as_ref()
        {
            invalidated.store(true, Ordering::Release);
        }
        Box::pin(async { Ok(()) })
    }

    fn events<'a>(
        &'a self,
        _: &'a ExecutionHandleV1,
        page: EventPageRequestV1,
    ) -> BackendFuture<'a, Vec<ExecutionEventV1>> {
        if let Some(invalidated) = self
            .invalidate_authorization_on_events
            .lock()
            .unwrap()
            .as_ref()
        {
            invalidated.store(true, Ordering::Release);
        }
        Box::pin(async move {
            Ok(self
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.sequence > page.after_sequence)
                .take(usize::from(page.max_events))
                .cloned()
                .collect())
        })
    }

    fn cancel<'a>(&'a self, _: &'a ExecutionHandleV1, _: u64) -> BackendFuture<'a, ()> {
        self.cancelled.store(true, Ordering::Release);
        Box::pin(async { Ok(()) })
    }

    fn collect_artifacts<'a>(
        &'a self,
        _: &'a ExecutionHandleV1,
    ) -> BackendFuture<'a, ExecutionResultV1> {
        Box::pin(async move { Ok(self.result.lock().unwrap().clone()) })
    }

    fn apply_workspace_result<'a>(
        &'a self,
        _: &'a ExecutionHandleV1,
        _: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ()> {
        self.workspace_result_applied.store(true, Ordering::Release);
        Box::pin(async { Ok(()) })
    }

    fn release<'a>(&'a self, _: &'a ExecutionHandleV1) -> BackendFuture<'a, ()> {
        self.released.store(true, Ordering::Release);
        Box::pin(async { Ok(()) })
    }
}

fn event(sequence: u64, event: ExecutionEventKindV1) -> ExecutionEventV1 {
    ExecutionEventV1 {
        schema_version: 1,
        execution_id: execution_id(),
        sequence,
        timestamp_unix_ms: sequence,
        event,
    }
}

fn execution_id() -> String {
    "00000000-0000-4000-8000-000000000003".into()
}

fn backend() -> Arc<FakeBackend> {
    let events = vec![
        event(
            1,
            ExecutionEventKindV1::Lifecycle {
                state: ExecutionLifecycleV1::Queued,
            },
        ),
        event(
            2,
            ExecutionEventKindV1::Lifecycle {
                state: ExecutionLifecycleV1::Preparing,
            },
        ),
        event(
            3,
            ExecutionEventKindV1::Lifecycle {
                state: ExecutionLifecycleV1::Running,
            },
        ),
        event(
            4,
            ExecutionEventKindV1::Stdout {
                chunk: "hello\n".into(),
            },
        ),
        event(
            5,
            ExecutionEventKindV1::Stderr {
                chunk: "warning\n".into(),
            },
        ),
        event(
            6,
            ExecutionEventKindV1::Lifecycle {
                state: ExecutionLifecycleV1::Collecting,
            },
        ),
        event(
            7,
            ExecutionEventKindV1::Lifecycle {
                state: ExecutionLifecycleV1::Succeeded,
            },
        ),
    ];
    Arc::new(FakeBackend {
        request: Mutex::new(None),
        released: AtomicBool::new(false),
        workspace_released: AtomicBool::new(false),
        cancelled: AtomicBool::new(false),
        expired_lease: AtomicBool::new(false),
        events: Mutex::new(events),
        result: Mutex::new(ExecutionResultV1 {
            schema_version: 1,
            execution_id: execution_id(),
            exit_code: Some(0),
            signal: None,
            timed_out: false,
            cancelled: false,
            metrics: ResourceMetricsV1 {
                cpu_millis: 0,
                peak_memory_bytes: 0,
                process_count: 0,
                disk_bytes: 0,
            },
            phase_timings: Default::default(),
            artifacts: Vec::new(),
            failure: None,
        }),
        prepare_delay: Mutex::new(std::time::Duration::ZERO),
        invalidate_authorization_on_exec: Mutex::new(None),
        invalidate_authorization_on_events: Mutex::new(None),
        lifecycle_updates: Mutex::new(Vec::new()),
        workspace_result_applied: AtomicBool::new(false),
        start_error: Mutex::new(None),
    })
}

fn session(backend: Arc<FakeBackend>) -> ExecutionSession {
    let workspace = WorkspaceRef {
        project_id: "00000000-0000-4000-8000-000000000001".into(),
        task_id: "00000000-0000-4000-8000-000000000002".into(),
        source_sha: "a".repeat(40),
    };
    ExecutionSession::new(
        backend,
        workspace,
        ExecutionTarget::local(),
        ExecutionModeV1::Implementation,
        format!("sha256:{}", "b".repeat(64)),
        ResourceBudgetV1 {
            cpu_millis: 1000,
            memory_bytes: 1024 * 1024 * 1024,
            process_count: 32,
            temporary_disk_bytes: 1024 * 1024 * 1024,
            output_bytes: 1024 * 1024,
        },
    )
    .unwrap()
}

pub(super) fn telemetry_test_session() -> ExecutionSession {
    session(backend())
}

#[path = "tests/commands.rs"]
mod commands;
