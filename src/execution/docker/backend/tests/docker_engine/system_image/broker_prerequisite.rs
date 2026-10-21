use super::super::support::{
    Fixture, OPT_IN, backend, committed_workspace, native_docker_arch, request,
};
use crate::execution::{
    contract::{ExecutionModeV1, ExecutionTarget},
    docker::images::{ImageLock, RuntimeProfile},
    session::ExecutionSession,
};
use crate::harness::{
    DependencyAuthorizationScope, DependencyDecision, DependencyKind, DependencyNeed,
    DependencyRequest, DependencyRequestStatus, PackageEcosystem,
};
use std::{
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::Duration,
};

/// Production boundary: recipe prerequisite request -> task-bound broker -> manager/user
/// authorization -> signed Debian closure -> offline derived Docker image -> executable tool.
/// Invariant: no package is acquired before one-time task-scoped user approval, and the exact
/// prepared image is selected by the existing session before a command can run. A unit test
/// cannot prove the broker/session binding, real image derivation, disabled network, and installed
/// package all agree. Requires rootless Linux Docker and HTTPS access to `deb.debian.org`; setup is
/// moderate and can take several minutes on a cold image/package-index cache.
#[test]
fn recipe_system_tool_prerequisite_flows_through_authorized_broker_before_execution() {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        return;
    }
    if !cfg!(target_os = "linux") {
        panic!("{OPT_IN}=1 currently requires a native Linux Docker Engine");
    }

    let fixture = Fixture::new();
    let (task, _task_clone) = committed_workspace(&fixture);
    let (backend, task_clone) = backend(&fixture, task.clone());
    let base = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, native_docker_arch())
        .unwrap();
    let seed = request(task.clone(), "");
    let session = Arc::new(
        ExecutionSession::new(
            Arc::new(backend),
            task.clone(),
            ExecutionTarget::local(),
            ExecutionModeV1::Verification,
            base.digest,
            seed.resource_budget,
        )
        .unwrap(),
    );
    let (progress_sender, progress) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let bridge = Arc::new(
        crate::harness::resource_bridge::ResourceBridge::start_for_task_repository(
            &task_clone,
            None,
            Some(&task.task_id),
            progress_sender,
            Arc::clone(&cancel),
        )
        .unwrap(),
    );
    bridge.bind_execution_session(Arc::clone(&session)).unwrap();

    let need = DependencyNeed {
        ecosystem: PackageEcosystem::System,
        package: Some("hello".into()),
        version: None,
        source: Some("debian:bookworm".into()),
        command: "apt-get install -y hello jq".into(),
        reason: "The approved runtime recipe requires hello and jq utilities".into(),
        kind: DependencyKind::SystemTool,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    };
    let request_bridge = Arc::clone(&bridge);
    let request_thread = std::thread::spawn(move || request_bridge.request_system_tools(need));
    let reviewing = next_dependency(&progress, DependencyRequestStatus::ManagerReviewing);
    assert!(!reviewing.need.introduced_packages.is_empty());
    answer(
        &reviewing,
        DependencyDecision::RequiresUserAuthorization,
        None,
        "System packages require explicit task-scoped user approval.",
    );
    let awaiting_user = next_dependency(&progress, DependencyRequestStatus::AwaitingUser);
    answer(
        &awaiting_user,
        DependencyDecision::UserAuthorizeForTask,
        Some(DependencyAuthorizationScope::Once),
        "Approve this exact signed Debian package closure for this task.",
    );

    let prepared = request_thread
        .join()
        .expect("broker request thread must finish")
        .expect("user-authorized recipe prerequisite must be prepared");
    assert_eq!(prepared.status, DependencyRequestStatus::Prepared);
    let hello_version = prepared
        .need
        .introduced_packages
        .iter()
        .find(|package| package.package == "hello")
        .expect("prepared closure must include the requested package")
        .version
        .clone();
    let output = session
        .run_tool_command(
            &format!(
                "set -eu\ntest \"$(dpkg-query -W -f='${{Version}}' hello)\" = '{hello_version}'\ncommand -v jq >/dev/null\ntest \"$(wc -l < /proc/net/route)\" -eq 1\nprintf 'broker-prerequisite-ready\\n'\n"
            ),
            Duration::from_secs(30),
        )
        .expect("recipe session must execute only in the prepared image");
    assert_eq!(output.result.exit_code, Some(0));
    assert!(output.stdout.contains("broker-prerequisite-ready"));
}

fn next_dependency(
    progress: &mpsc::Receiver<crate::harness::LiveProgress>,
    status: DependencyRequestStatus,
) -> DependencyRequest {
    loop {
        let update = progress
            .recv_timeout(Duration::from_secs(120))
            .expect("broker must publish each authorization state");
        if let Some(request) = update
            .dependency_requests
            .into_iter()
            .find(|request| request.status == status)
        {
            return request;
        }
    }
}

fn answer(
    request: &DependencyRequest,
    decision: DependencyDecision,
    scope: Option<DependencyAuthorizationScope>,
    rationale: &str,
) {
    assert!(crate::harness::dependency_authorization::answer(
        &request.id,
        &request.task_id,
        &request.need,
        crate::harness::dependency_authorization::DependencyResolution {
            decision,
            scope,
            rationale: rationale.into(),
        },
    ));
}
