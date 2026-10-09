use super::super::{ResourceAction, ResourceRequest, ResourceResponse};
use super::{answer, start_bridge, take_dependency_update};
use crate::harness::{DependencyDecision, DependencyRequestStatus};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    sync::{Arc, Barrier, atomic::AtomicBool, mpsc},
    time::Duration,
};

fn send_request(client: &mut UnixStream, request: ResourceRequest) {
    serde_json::to_writer(&mut *client, &request).unwrap();
    client.write_all(b"\n").unwrap();
}

fn response(client: &mut UnixStream) -> ResourceResponse {
    let mut line = String::new();
    BufReader::new(client).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

fn resource_request(
    action: ResourceAction,
    manager: Option<&str>,
    url: Option<&str>,
    purpose: &str,
) -> ResourceRequest {
    ResourceRequest {
        action,
        manager: manager.map(str::to_owned),
        url: url.map(str::to_owned),
        dependency: None,
        dependency_request_id: None,
        retry_succeeded: None,
        purpose: purpose.into(),
    }
}

fn temp_worktree(label: &str) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("koolade-resource-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    root
}

#[test]
fn independent_fetch_completes_while_an_approved_restore_is_running() {
    let worktree = temp_worktree("independent-fetch");
    std::fs::write(
        worktree.join("package-lock.json"),
        r#"{"lockfileVersion":3,"packages":{}}"#,
    )
    .unwrap();
    let (progress, updates) = mpsc::channel();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let hook_entered = entered.clone();
    let hook_release = release.clone();
    let hook: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        hook_entered.wait();
        hook_release.wait();
    });
    let bridge = super::super::ResourceBridge::start_with_preparation_hook(
        &worktree,
        Some("task-approved-restore"),
        progress,
        Arc::new(AtomicBool::new(false)),
        hook,
    )
    .unwrap();

    let mut restore = UnixStream::connect(bridge.socket_path()).unwrap();
    restore
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    send_request(
        &mut restore,
        resource_request(
            ResourceAction::PrepareNpm,
            None,
            None,
            "Restore the project's locked npm dependencies",
        ),
    );
    let pending = take_dependency_update(&updates);
    assert_eq!(pending.status, DependencyRequestStatus::ManagerReviewing);
    answer(
        &pending,
        DependencyDecision::AuthorizeForTask,
        None,
        "Approve this locked restore for the task",
    );
    entered.wait();

    let mut fetch = UnixStream::connect(bridge.socket_path()).unwrap();
    fetch
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    send_request(
        &mut fetch,
        resource_request(
            ResourceAction::Fetch,
            None,
            Some("https://example.com/independent-resource"),
            "Review an unrelated resource request",
        ),
    );
    let fetched = response(&mut fetch);
    assert_eq!(fetched.status, "needs_attention");
    assert!(fetched.summary.contains("example.com"));

    release.wait();
    let restored = response(&mut restore);
    assert_eq!(restored.status, "prepared", "{}", restored.summary);
    drop(fetch);
    drop(restore);
    drop(bridge);
    std::fs::remove_dir_all(worktree).unwrap();
}

#[test]
fn out_of_order_authorization_answers_remain_attached_to_their_requests() {
    let worktree = temp_worktree("out-of-order");
    let (bridge, updates) = start_bridge(&worktree, Some("task-out-of-order"));
    let mut first = UnixStream::connect(bridge.socket_path()).unwrap();
    let mut second = UnixStream::connect(bridge.socket_path()).unwrap();
    send_request(
        &mut first,
        resource_request(
            ResourceAction::UnsupportedManager,
            Some("manager-a"),
            None,
            "Review manager A",
        ),
    );
    let first_pending = take_dependency_update(&updates);
    send_request(
        &mut second,
        resource_request(
            ResourceAction::UnsupportedManager,
            Some("manager-b"),
            None,
            "Review manager B",
        ),
    );
    let second_pending = take_dependency_update(&updates);
    assert_ne!(first_pending.id, second_pending.id);

    answer(
        &second_pending,
        DependencyDecision::Reject,
        None,
        "Reject request B",
    );
    answer(
        &first_pending,
        DependencyDecision::Reject,
        None,
        "Reject request A",
    );
    let first_result = response(&mut first).dependency_request.unwrap();
    let second_result = response(&mut second).dependency_request.unwrap();
    assert_eq!(first_result.id, first_pending.id);
    assert_eq!(first_result.rationale, "Reject request A");
    assert_eq!(second_result.id, second_pending.id);
    assert_eq!(second_result.rationale, "Reject request B");
    drop(bridge);
    std::fs::remove_dir_all(worktree).unwrap();
}

#[test]
fn disconnect_while_waiting_unregisters_the_authorization() {
    let worktree = temp_worktree("disconnect");
    let (bridge, updates) = start_bridge(&worktree, Some("task-disconnect"));
    let mut client = UnixStream::connect(bridge.socket_path()).unwrap();
    send_request(
        &mut client,
        resource_request(
            ResourceAction::UnsupportedManager,
            Some("unsupported-manager"),
            None,
            "Review an unsupported manager request",
        ),
    );
    let pending = take_dependency_update(&updates);
    drop(client);

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut failed = false;
    while std::time::Instant::now() < deadline {
        let Ok(update) = updates.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        failed |= update.dependency_requests.iter().any(|request| {
            request.id == pending.id && request.status == DependencyRequestStatus::Failed
        });
        if failed {
            break;
        }
    }
    assert!(failed, "disconnect should publish a failed request state");
    assert!(!crate::harness::dependency_authorization::answer(
        &pending.id,
        &pending.task_id,
        &pending.need,
        crate::harness::dependency_authorization::DependencyResolution {
            decision: DependencyDecision::AuthorizeForTask,
            scope: None,
            rationale: "A delayed answer must be rejected".into(),
        }
    ));
    drop(bridge);
    std::fs::remove_dir_all(worktree).unwrap();
}

#[test]
fn cancellation_while_waiting_unregisters_the_authorization() {
    let worktree = temp_worktree("cancel-pending");
    let (progress, updates) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let bridge = super::super::ResourceBridge::start(
        &worktree,
        Some("task-cancel-pending"),
        progress,
        cancel.clone(),
    )
    .unwrap();
    let mut client = UnixStream::connect(bridge.socket_path()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    send_request(
        &mut client,
        resource_request(
            ResourceAction::UnsupportedManager,
            Some("unsupported-manager"),
            None,
            "Review an unsupported manager request",
        ),
    );
    let pending = take_dependency_update(&updates);
    assert_eq!(pending.status, DependencyRequestStatus::ManagerReviewing);

    cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    let result = response(&mut client);
    assert_eq!(result.status, "cancelled");

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    let mut failed = false;
    while std::time::Instant::now() < deadline {
        let Ok(update) = updates.recv_timeout(Duration::from_millis(200)) else {
            continue;
        };
        failed |= update.dependency_requests.iter().any(|request| {
            request.id == pending.id && request.status == DependencyRequestStatus::Failed
        });
        if failed {
            break;
        }
    }
    assert!(failed, "cancellation should publish a failed request state");
    assert!(!crate::harness::dependency_authorization::answer(
        &pending.id,
        &pending.task_id,
        &pending.need,
        crate::harness::dependency_authorization::DependencyResolution {
            decision: DependencyDecision::AuthorizeForTask,
            scope: None,
            rationale: "A delayed answer after cancellation must be rejected".into(),
        }
    ));
    drop(client);
    drop(bridge);
    std::fs::remove_dir_all(worktree).unwrap();
}
