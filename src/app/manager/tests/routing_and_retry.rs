use super::*;
use crate::harness::{AiHarness, HarnessOutcome};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

struct FailingHarness {
    calls: Arc<AtomicUsize>,
    error: crate::error::AppError,
    route: crate::harness::HarnessRouteProvenance,
    observed: std::sync::mpsc::Sender<(
        crate::harness::ExecutionMode,
        Option<crate::execution::contract::WorkspaceRef>,
    )>,
}

impl AiHarness for FailingHarness {
    fn label(&self) -> String {
        "failing-provider".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok("fixture ready".into())
    }

    fn route_provenance(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Option<crate::harness::HarnessRouteProvenance> {
        Some(self.route.clone())
    }

    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<HarnessOutcome, crate::error::AppError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _ = self
            .observed
            .send((request.mode, request.execution_workspace.clone()));
        Err(self.error.clone())
    }
}

#[test]
fn manager_uses_planning_mode_with_a_pinned_read_only_project_workspace() {
    let (_temp, project, _) = opportunities::initialized_repository();
    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let workspace = manager_execution_workspace(&project, &task_id)
        .expect("Git project should bind Manager reads to the selected project checkout");
    workspace.validate().unwrap();
    assert_eq!(workspace.task_id, task_id);
    assert_eq!(
        workspace.project_id,
        project
            .state
            .planning_store
            .project_id
            .hyphenated()
            .to_string()
    );
}

#[test]
fn manager_binds_a_valid_linked_worktree_as_its_read_only_target() {
    use std::process::Command;

    let (_repository, mut project, _) = opportunities::initialized_repository();
    let worktree_parent = tempfile::tempdir().unwrap();
    let worktree = worktree_parent.path().join("linked-checkout");
    let output = Command::new("git")
        .args(["worktree", "add", "--detach"])
        .arg(&worktree)
        .arg("HEAD")
        .current_dir(&project.state.repo_root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git worktree add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(worktree.join(".git").is_file());
    project.state.repo_root = worktree;

    let task_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let workspace = manager_execution_workspace(&project, &task_id)
        .expect("valid linked worktree must be bindable for read-only Manager access");
    workspace.validate().unwrap();
    assert_eq!(workspace.task_id, task_id);
}

#[test]
fn manager_blocks_instead_of_invoking_without_a_pinned_read_only_workspace() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = tempfile::tempdir().unwrap();
    environment.set("KOOLADE_HOME", home.path());
    let (repository, mut project, _) = opportunities::initialized_repository();
    let invalid_root = repository.path().join("not-a-git-checkout");
    std::fs::create_dir(&invalid_root).unwrap();
    project.state.repo_root = invalid_root.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let (observed_tx, _observed_rx) = std::sync::mpsc::channel();
    let manager = Manager::start(
        &project,
        &["manager-refresh".into()],
        Box::new(FailingHarness {
            calls: calls.clone(),
            error: crate::error::AppError::Other("provider must not run".into()),
            route: crate::harness::HarnessRouteProvenance {
                provider_id: "codex".into(),
                role: crate::execution::contract::HarnessRole::Manager,
                work_category: Some("manager".into()),
                selected_model: None,
                requested_model: None,
                capability_probe_id: None,
                capability_generation: None,
            },
            observed: observed_tx,
        }),
    );

    let error = match manager.result().unwrap() {
        Err(error) => error,
        Ok(_) => panic!("Manager must not run without a read-only workspace"),
    };
    assert!(error.contains("read-only Manager workspace"));
    assert!(error.contains("did not invoke a provider"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let slug = crate::persistence::project_slug(&invalid_root.canonicalize().unwrap());
    let (records, skipped) = crate::persistence::telemetry::load(&slug);
    assert_eq!(skipped, 0);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, "blocked");
    assert_eq!(
        records[0].failure_category.as_deref(),
        Some("tool_bridge_unavailable")
    );
}

#[test]
fn manager_does_not_retry_a_typed_provider_failure_marked_unsafe() {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let telemetry_home = tempfile::tempdir().unwrap();
    environment.set("KOOLADE_HOME", telemetry_home.path());
    let (_temp, project, _) = opportunities::initialized_repository();
    let calls = Arc::new(AtomicUsize::new(0));
    let (observed_tx, observed_rx) = std::sync::mpsc::channel();
    let failure = crate::harness::HarnessExecutionFailure {
        provider_id: "codex".into(),
        role: crate::execution::contract::HarnessRole::Manager,
        mode: crate::harness::ExecutionMode::Planning,
        target: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
        category: crate::harness::HarnessFailureCategory::HostCodeModeDisabled,
        provider_code: None,
        sanitized_message: "host code mode is disabled by application policy".into(),
        actionable_fix: Some("Use the approved Docker tool bridge".into()),
        retryable: false,
    };
    let manager = Manager::start(
        &project,
        &["manager-refresh".into()],
        Box::new(FailingHarness {
            calls: calls.clone(),
            error: crate::error::AppError::HarnessExecutionFailed {
                failure: Box::new(failure),
            },
            route: crate::harness::HarnessRouteProvenance {
                provider_id: "codex".into(),
                role: crate::execution::contract::HarnessRole::Manager,
                work_category: Some("manager".into()),
                selected_model: Some("manager-default".into()),
                requested_model: None,
                capability_probe_id: Some("synthetic-probe".into()),
                capability_generation: Some(3),
            },
            observed: observed_tx,
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = manager.result() {
            assert!(result.is_err());
            break;
        }
        assert!(Instant::now() < deadline, "manager harness timed out");
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let (mode, workspace) = observed_rx.try_recv().unwrap();
    assert_eq!(mode, crate::harness::ExecutionMode::Planning);
    assert!(
        workspace.is_some(),
        "Manager should receive Docker read tools"
    );
    let slug = crate::persistence::project_slug(&project.state.repo_root.canonicalize().unwrap());
    let (records, skipped) = crate::persistence::telemetry::load(&slug);
    assert_eq!(skipped, 0);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].routed_harness.as_deref(), Some("codex"));
    assert_eq!(records[0].route_role.as_deref(), Some("Manager"));
    assert_eq!(records[0].model.as_deref(), Some("manager-default"));
    assert_eq!(records[0].requested_model, None);
    assert_eq!(records[0].capability_generation, Some(3));
    assert_eq!(records[0].thinking_level.as_deref(), Some("xhigh"));
    assert_eq!(records[0].execution_mode.as_deref(), Some("Planning"));
    assert_eq!(
        records[0].execution_target.as_deref(),
        workspace.map(|w| w.task_id).as_deref()
    );
    assert_eq!(
        records[0].failure_category.as_deref(),
        Some("hostcodemodedisabled")
    );
    assert!(
        records[0]
            .failure_message
            .as_deref()
            .is_some_and(|message| message.contains("host code mode is disabled"))
    );
}
