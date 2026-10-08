use super::policy::{Decision, classify};
use super::{ResourceBridge, ResourceRequest, ResourceResponse};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    sync::{Arc, atomic::AtomicBool, mpsc},
};

mod end_to_end;
mod rejection;
mod success;

#[test]
fn stalled_clients_cannot_exceed_the_broker_worker_limit() {
    let worktree =
        std::env::temp_dir().join(format!("koolade-resource-stalled-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&worktree).unwrap();
    let (bridge, _updates) = start_bridge(&worktree, None);
    let mut clients = (0..super::MAX_IN_FLIGHT_REQUESTS)
        .map(|_| UnixStream::connect(bridge.socket_path()).unwrap())
        .collect::<Vec<_>>();
    let mut excess = UnixStream::connect(bridge.socket_path()).unwrap();
    excess
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    let mut response = Vec::new();
    let read = std::io::Read::read_to_end(&mut excess, &mut response).unwrap();
    assert_eq!(
        read, 0,
        "the excess connection should be closed without a worker"
    );
    assert!(response.is_empty());

    drop(excess);
    clients.clear();
    drop(bridge);
    std::fs::remove_dir_all(worktree).unwrap();
}

fn start_bridge(
    worktree: &std::path::Path,
    task_id: Option<&str>,
) -> (ResourceBridge, mpsc::Receiver<crate::harness::LiveProgress>) {
    let (progress, updates) = mpsc::channel();
    let bridge = ResourceBridge::start(
        worktree,
        task_id,
        progress,
        Arc::new(AtomicBool::new(false)),
    )
    .unwrap();
    (bridge, updates)
}

fn take_dependency_update(
    updates: &mpsc::Receiver<crate::harness::LiveProgress>,
) -> crate::harness::DependencyRequest {
    loop {
        let update = updates
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("dependency request should be delivered to task activity");
        if let Some(request) = update.dependency_requests.into_iter().next() {
            return request;
        }
    }
}

fn answer(
    id: &str,
    decision: crate::harness::DependencyDecision,
    scope: Option<crate::harness::DependencyAuthorizationScope>,
    rationale: &str,
) {
    assert!(crate::harness::dependency_authorization::answer(
        id,
        crate::harness::dependency_authorization::DependencyResolution {
            decision,
            scope,
            rationale: rationale.into(),
        }
    ));
}

#[test]
fn public_npm_registry_package_paths_are_auto_allowed() {
    for url in [
        "https://registry.npmjs.org/react",
        "https://registry.npmjs.org/react/18.3.1",
        "https://registry.npmjs.org/@scope%2fpkg",
        "https://registry.npmjs.org/pkg/-/pkg-1.2.3.tgz",
        "https://registry.npmjs.org/@scope/pkg/-/pkg-1.2.3.tgz",
    ] {
        assert!(
            matches!(classify(url).unwrap(), Decision::Allow(_)),
            "{url}"
        );
    }
}

#[test]
fn prepare_nuget_audit_action_is_a_recognized_mediated_resource_request() {
    let request = ResourceRequest {
        action: super::ResourceAction::PrepareNugetAudit,
        manager: None,
        url: None,
        dependency: None,
        purpose: "Retry verification with public NuGet vulnerability data".into(),
    };
    let encoded = serde_json::to_string(&request).unwrap();
    assert!(encoded.contains("prepare_nuget_audit"));
    let decoded: ResourceRequest = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.action, super::ResourceAction::PrepareNugetAudit);
}

#[test]
fn uncertain_resource_request_is_returned_and_recorded_for_operator_attention() {
    let worktree =
        std::env::temp_dir().join(format!("koolade-resource-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&worktree).unwrap();
    let (bridge, _updates) = start_bridge(&worktree, None);
    let mut client = UnixStream::connect(bridge.socket_path()).unwrap();
    serde_json::to_writer(
        &mut client,
        &ResourceRequest {
            action: super::ResourceAction::Fetch,
            manager: None,
            url: Some("https://example.com/needed-file".into()),
            dependency: None,
            purpose: "Read a required API reference".into(),
        },
    )
    .unwrap();
    client.write_all(b"\n").unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    let mut response = String::new();
    BufReader::new(client).read_line(&mut response).unwrap();
    let response: ResourceResponse = serde_json::from_str(&response).unwrap();
    assert_eq!(response.status, "needs_attention");
    assert!(response.summary.contains("example.com"));
    assert!(bridge.attention_detail().unwrap().contains("example.com"));
    drop(bridge);
    std::fs::remove_dir_all(worktree).unwrap();
}

#[test]
fn unsupported_package_manager_is_returned_for_operator_attention() {
    let worktree =
        std::env::temp_dir().join(format!("koolade-resource-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&worktree).unwrap();
    let (bridge, updates) = start_bridge(&worktree, None);
    let mut client = UnixStream::connect(bridge.socket_path()).unwrap();
    serde_json::to_writer(
        &mut client,
        &ResourceRequest {
            action: super::ResourceAction::UnsupportedManager,
            manager: Some("pnpm".into()),
            url: None,
            dependency: None,
            purpose: "Install locked dependencies".into(),
        },
    )
    .unwrap();
    client.write_all(b"\n").unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    let request = take_dependency_update(&updates);
    assert_eq!(
        request.status,
        crate::harness::DependencyRequestStatus::ManagerReviewing
    );
    answer(
        &request.id,
        crate::harness::DependencyDecision::RequiresUserAuthorization,
        None,
        "The unsupported package manager needs an explicit decision.",
    );
    let awaiting = take_dependency_update(&updates);
    assert_eq!(
        awaiting.status,
        crate::harness::DependencyRequestStatus::AwaitingUser
    );
    answer(
        &request.id,
        crate::harness::DependencyDecision::Reject,
        None,
        "The operator denied the unsupported package manager request.",
    );
    let mut response = String::new();
    BufReader::new(client).read_line(&mut response).unwrap();
    let response: ResourceResponse = serde_json::from_str(&response).unwrap();
    assert_eq!(response.status, "denied");
    assert!(response.summary.contains("pnpm"));
    assert_eq!(
        response
            .dependency_request
            .as_ref()
            .map(|request| request.task_id.as_str()),
        Some("unknown-task")
    );
    assert_eq!(
        bridge.dependency_request().unwrap().status,
        crate::harness::DependencyRequestStatus::Denied
    );
    assert!(bridge.attention_detail().unwrap().contains("pnpm"));
    drop(bridge);
    std::fs::remove_dir_all(worktree).unwrap();
}

#[test]
fn structured_dependency_request_uses_application_task_identity_and_public_source_policy() {
    use crate::harness::{DependencyKind, DependencyNeed, PackageEcosystem};

    let worktree =
        std::env::temp_dir().join(format!("koolade-resource-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&worktree).unwrap();
    let (bridge, updates) = start_bridge(&worktree, Some("TASK-82"));
    let mut client = UnixStream::connect(bridge.socket_path()).unwrap();
    serde_json::to_writer(
        &mut client,
        &ResourceRequest {
            action: super::ResourceAction::DependencyRequest,
            manager: None,
            url: None,
            dependency: Some(DependencyNeed {
                ecosystem: PackageEcosystem::Npm,
                package: Some("zod".into()),
                version: Some("^4.0.0".into()),
                source: Some("https://registry.npmjs.org".into()),
                command: "npm install zod".into(),
                reason: "Validate the imported settings schema".into(),
                kind: DependencyKind::NewProjectDependency,
                lockfile_identity: None,
                introduced_packages: Vec::new(),
            }),
            purpose: "Validate the imported settings schema".into(),
        },
    )
    .unwrap();
    client.write_all(b"\n").unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    let pending = take_dependency_update(&updates);
    answer(
        &pending.id,
        crate::harness::DependencyDecision::RequiresUserAuthorization,
        None,
        "Man.ager requests approval for this new package.",
    );
    let awaiting = take_dependency_update(&updates);
    assert_eq!(
        awaiting.status,
        crate::harness::DependencyRequestStatus::AwaitingUser
    );
    answer(
        &pending.id,
        crate::harness::DependencyDecision::Reject,
        None,
        "The user denied this dependency request.",
    );
    let mut response = String::new();
    BufReader::new(client).read_line(&mut response).unwrap();
    let response: ResourceResponse = serde_json::from_str(&response).unwrap();
    assert_eq!(response.status, "denied");
    let request = response.dependency_request.unwrap();
    assert_eq!(request.task_id, "TASK-82");
    assert_eq!(request.need.package.as_deref(), Some("zod"));
    assert_eq!(request.decision, crate::harness::DependencyDecision::Reject);
    assert_eq!(
        request.status,
        crate::harness::DependencyRequestStatus::Denied
    );
    assert!(request.rationale.contains("denied"));
    assert_eq!(bridge.dependency_request().unwrap().id, request.id);
    drop(bridge);
    std::fs::remove_dir_all(worktree).unwrap();
}

#[test]
fn uncertain_and_unsafe_urls_do_not_auto_fetch() {
    for url in [
        "https://example.com/file",
        "https://registry.npmjs.org/react?redirect=example.com",
        "http://registry.npmjs.org/react",
        "https://user:pass@registry.npmjs.org/react",
        "https://registry.npmjs.org/react/../../etc/passwd",
        "https://registry.npmjs.org/react:444",
    ] {
        let result = classify(url);
        assert!(
            result.is_err() || matches!(result.unwrap(), Decision::NeedsAttention(_)),
            "unsafe URL was allowed: {url}"
        );
    }
}
