use std::{fs, path::PathBuf};

use super::super::RuntimeExecution;
use super::RuntimeExecutionContext;
use crate::execution::{
    contract::{
        ExecutionEventKindV1, ExecutionLifecycleV1, ExecutionModeV1, ExecutionRequestV1,
        ExecutionTarget, ResourceBudgetV1, WorkspaceRef,
    },
    docker::{
        images::{ImageLock, RuntimeProfile},
        sandbox::ValidatedWorkspace,
    },
    policy::ExecutionPolicyV1,
};

struct IsolatedHome {
    _lock: crate::test_support::ProcessEnvironmentLock,
    previous: Option<std::ffi::OsString>,
    root: PathBuf,
}

impl IsolatedHome {
    fn new() -> Self {
        let lock = crate::test_support::lock_process_environment();
        let previous = std::env::var_os("KOOLADE_HOME");
        let root =
            std::env::temp_dir().join(format!("koolade-runtime-record-{}", uuid::Uuid::new_v4()));
        unsafe { std::env::set_var("KOOLADE_HOME", &root) };
        Self {
            _lock: lock,
            previous,
            root,
        }
    }
}

impl Drop for IsolatedHome {
    fn drop(&mut self) {
        unsafe {
            if let Some(previous) = self.previous.take() {
                std::env::set_var("KOOLADE_HOME", previous);
            } else {
                std::env::remove_var("KOOLADE_HOME");
            }
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn lifecycle_updates_replace_the_durable_cursor_and_state() {
    let _home = IsolatedHome::new();
    let workspace_path = std::env::temp_dir().join(format!(
        "koolade-runtime-workspace-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(workspace_path.join(".git")).unwrap();
    let workspace = ValidatedWorkspace::inspect(&workspace_path).unwrap();
    let project_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let budget = ResourceBudgetV1 {
        cpu_millis: 1000,
        memory_bytes: 1024 * 1024 * 1024,
        process_count: 16,
        temporary_disk_bytes: 1024 * 1024 * 1024,
        output_bytes: 1024 * 1024,
    };
    let image = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let deadline_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + crate::execution::contract::MAX_EXECUTION_DURATION_MS
        - 60_000;
    let lease_expires_at_unix_ms = deadline_unix_ms.saturating_add(60_000);
    let request = ExecutionRequestV1 {
        schema_version: 1,
        execution_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        project_id: project_id.clone(),
        task_id: task_id.clone(),
        target: ExecutionTarget::local(),
        controller_target_id: None,
        workspace: WorkspaceRef {
            project_id,
            task_id: task_id.clone(),
            source_sha: "a".repeat(40),
        },
        mode: ExecutionModeV1::Implementation,
        image_digest: image.digest,
        authorization_generation: uuid::Uuid::new_v4().hyphenated().to_string(),
        argv: vec!["sh".into(), "-s".into()],
        working_directory_relative: ".".into(),
        stdin: Some("true".into()),
        environment_allowlist: Default::default(),
        resource_budget: budget.clone(),
        deadline_unix_ms,
        allowed_service_ports: Default::default(),
        broker_capability_ids: Default::default(),
        dependency_cache_refs: Vec::new(),
        cancellation_generation: 1,
        request_sequence: 1,
        policy: ExecutionPolicyV1::for_mode(
            ExecutionModeV1::Implementation,
            budget,
            Default::default(),
        )
        .unwrap(),
    };
    let execution_id = request.execution_id.clone();
    let mut execution = RuntimeExecution::new(
        request,
        RuntimeExecutionContext {
            container_id: "container-test".into(),
            container_name: "koolade-exec-test".into(),
            workspace,
            lease_id: uuid::Uuid::new_v4().to_string(),
            owner_instance_id: uuid::Uuid::new_v4().to_string(),
            policy_version: "docker-sandbox-v1".into(),
            lease_expires_at_unix_ms,
            task_lock: crate::persistence::execution_records::try_task_lock(&task_id)
                .unwrap()
                .unwrap(),
            workspace_snapshot: None,
        },
    );
    execution
        .push(ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Queued,
        })
        .unwrap();
    let queued = crate::persistence::execution_records::load(&task_id, &execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        queued.state,
        crate::persistence::execution_records::ExecutionRecordStateV1::Preparing
    );
    assert_eq!(queued.event_cursor, 1);
    assert_eq!(
        queued.lease_expires_at_unix_ms,
        Some(lease_expires_at_unix_ms)
    );

    execution
        .push(ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Running,
        })
        .unwrap();
    let running = crate::persistence::execution_records::load(&task_id, &execution_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        running.state,
        crate::persistence::execution_records::ExecutionRecordStateV1::Running
    );
    assert_eq!(running.event_cursor, 2);
    assert_eq!(
        running.lease_expires_at_unix_ms,
        Some(lease_expires_at_unix_ms)
    );

    drop(execution);
    let _ = fs::remove_dir_all(workspace_path);
}
