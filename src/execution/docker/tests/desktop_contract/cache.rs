use super::*;
use std::sync::atomic::Ordering;

#[test]
fn desktop_capacity_cache_is_short_lived_and_bound_to_endpoint_and_daemon() {
    let endpoint = DockerEndpoint::NamedPipe {
        path: r"\\.\pipe\docker_engine".into(),
        desktop: true,
    };
    let api = desktop_api_with_capacity(desktop_snapshot(), 2 * 1024 * 1024 * 1024);
    let discovery = EngineDiscovery::with_test_verified_desktop_transport(endpoint, api.clone());
    let workspace = tempfile::tempdir().unwrap();

    let first = block_on(discovery.probe(workspace.path())).unwrap();
    assert!(first.ready);
    assert!(first.desktop_vm_capacity_verified);
    assert_eq!(api.capacity_calls.load(Ordering::SeqCst), 1);

    let same_daemon = block_on(discovery.probe(workspace.path())).unwrap();
    assert!(same_daemon.ready);
    assert!(same_daemon.desktop_vm_capacity_verified);
    assert_eq!(api.capacity_calls.load(Ordering::SeqCst), 1);

    api.snapshot.lock().unwrap().daemon_id = "replacement-daemon".into();
    let changed_daemon = block_on(discovery.probe(workspace.path())).unwrap();
    assert!(changed_daemon.ready);
    assert!(changed_daemon.desktop_vm_capacity_verified);
    assert_eq!(api.capacity_calls.load(Ordering::SeqCst), 2);
}
