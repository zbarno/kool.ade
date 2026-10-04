use super::policy::{Decision, classify};
use super::{ResourceBridge, ResourceRequest, ResourceResponse};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
};

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
fn uncertain_resource_request_is_returned_and_recorded_for_operator_attention() {
    let worktree =
        std::env::temp_dir().join(format!("koolade-resource-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&worktree).unwrap();
    let bridge = ResourceBridge::start(&worktree).unwrap();
    let mut client = UnixStream::connect(bridge.socket_path()).unwrap();
    serde_json::to_writer(
        &mut client,
        &ResourceRequest {
            action: super::ResourceAction::Fetch,
            manager: None,
            url: Some("https://example.com/needed-file".into()),
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
    let bridge = ResourceBridge::start(&worktree).unwrap();
    let mut client = UnixStream::connect(bridge.socket_path()).unwrap();
    serde_json::to_writer(
        &mut client,
        &ResourceRequest {
            action: super::ResourceAction::UnsupportedManager,
            manager: Some("pnpm".into()),
            url: None,
            purpose: "Install locked dependencies".into(),
        },
    )
    .unwrap();
    client.write_all(b"\n").unwrap();
    client.shutdown(std::net::Shutdown::Write).unwrap();
    let mut response = String::new();
    BufReader::new(client).read_line(&mut response).unwrap();
    let response: ResourceResponse = serde_json::from_str(&response).unwrap();
    assert_eq!(response.status, "needs_attention");
    assert!(response.summary.contains("pnpm"));
    assert!(bridge.attention_detail().unwrap().contains("pnpm"));
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
