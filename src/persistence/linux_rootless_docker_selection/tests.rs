use super::*;

const ENDPOINT_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DAEMON_A: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DAEMON_B: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn observation<'a>(
    user_id: u32,
    socket_owner_id: Option<u32>,
    endpoint_fingerprint: Option<&'a str>,
    daemon_fingerprint: Option<&'a str>,
    rootless: Option<bool>,
) -> Observation<'a> {
    Observation {
        user_id,
        socket: socket_owner_id
            .map(SocketObservation::Owned)
            .unwrap_or(SocketObservation::Missing),
        endpoint_fingerprint,
        daemon_fingerprint,
        rootless,
    }
}

fn selection(user_id: u32, endpoint: &str, daemon: &str) -> LinuxRootlessDockerSelectionV1 {
    LinuxRootlessDockerSelectionV1::new(user_id, user_id, endpoint, daemon).unwrap()
}

#[test]
fn selection_round_trip_stores_only_user_and_hashed_endpoint_daemon_identity() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    let saved = selection(1000, ENDPOINT_A, DAEMON_A);

    save_to(&path, &saved).unwrap();

    assert_eq!(load_from(&path), (Some(saved), None));
    let encoded = std::fs::read_to_string(path).unwrap();
    assert!(!encoded.contains("/run/user"));
    assert!(!encoded.contains("docker.sock"));
    assert!(!encoded.contains("unix://"));
}

#[test]
fn changed_user_socket_owner_endpoint_or_rootful_daemon_invalidates_selection() {
    for current in [
        observation(1001, Some(1001), Some(ENDPOINT_A), Some(DAEMON_A), None),
        observation(
            1000,
            Some(1001),
            Some(ENDPOINT_A),
            Some(DAEMON_A),
            Some(true),
        ),
        observation(1000, Some(1000), Some(DAEMON_B), Some(DAEMON_A), None),
        observation(
            1000,
            Some(1000),
            Some(ENDPOINT_A),
            Some(DAEMON_B),
            Some(false),
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(FILE);
        save_to(&path, &selection(1000, ENDPOINT_A, DAEMON_A)).unwrap();

        reconcile_at(&path, current).unwrap();

        assert!(load_from(&path).0.is_none());
    }
}

#[test]
fn verified_new_daemon_replaces_old_fingerprint_for_the_current_socket() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    save_to(&path, &selection(1000, ENDPOINT_A, DAEMON_A)).unwrap();

    reconcile_at(
        &path,
        observation(
            1000,
            Some(1000),
            Some(ENDPOINT_A),
            Some(DAEMON_B),
            Some(true),
        ),
    )
    .unwrap();

    assert_eq!(
        load_from(&path).0,
        Some(selection(1000, ENDPOINT_A, DAEMON_B))
    );
}

#[test]
fn verified_new_user_replaces_the_previous_users_selection() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    save_to(&path, &selection(1000, ENDPOINT_A, DAEMON_A)).unwrap();

    reconcile_at(
        &path,
        observation(
            1001,
            Some(1001),
            Some(ENDPOINT_A),
            Some(DAEMON_B),
            Some(true),
        ),
    )
    .unwrap();

    assert_eq!(
        load_from(&path).0,
        Some(selection(1001, ENDPOINT_A, DAEMON_B))
    );
}

#[test]
fn daemon_outage_preserves_matching_saved_endpoint_identity() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    let saved = selection(1000, ENDPOINT_A, DAEMON_A);
    save_to(&path, &saved).unwrap();

    reconcile_at(&path, observation(1000, None, Some(ENDPOINT_A), None, None)).unwrap();

    assert_eq!(load_from(&path).0, Some(saved));
}

#[test]
fn inaccessible_socket_does_not_preserve_saved_endpoint_identity() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    save_to(&path, &selection(1000, ENDPOINT_A, DAEMON_A)).unwrap();

    reconcile_at(
        &path,
        Observation {
            user_id: 1000,
            socket: SocketObservation::Unknown,
            endpoint_fingerprint: Some(ENDPOINT_A),
            daemon_fingerprint: None,
            rootless: None,
        },
    )
    .unwrap();

    assert!(load_from(&path).0.is_none());
}

#[cfg(target_os = "linux")]
#[test]
fn persisted_selection_requires_private_runtime_directory_and_safe_socket_mode() {
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};

    let runtime_dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(runtime_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = runtime_dir.path().join("docker.sock");
    let _listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o660)).unwrap();
    let user_id = unsafe { libc::geteuid() };
    if user_id == 0 {
        return;
    }

    assert!(matches!(
        socket_observation_at(runtime_dir.path(), &socket, user_id),
        SocketObservation::Owned(owner) if owner == user_id
    ));
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(matches!(
        socket_observation_at(runtime_dir.path(), &socket, user_id),
        SocketObservation::Unknown
    ));

    let settings_dir = tempfile::tempdir().unwrap();
    let path = settings_dir.path().join(FILE);
    save_to(&path, &selection(user_id, ENDPOINT_A, DAEMON_A)).unwrap();
    reconcile_at(
        &path,
        Observation {
            user_id,
            socket: socket_observation_at(runtime_dir.path(), &socket, user_id),
            endpoint_fingerprint: Some(ENDPOINT_A),
            daemon_fingerprint: None,
            rootless: None,
        },
    )
    .unwrap();
    assert!(load_from(&path).0.is_none());

    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o660)).unwrap();
    std::fs::set_permissions(runtime_dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        socket_observation_at(runtime_dir.path(), &socket, user_id),
        SocketObservation::Unknown
    ));
}

#[test]
fn malformed_or_path_bearing_records_are_not_accepted() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    std::fs::write(
        &path,
        r#"{"schemaVersion":1,"userId":1000,"socketOwnerId":1000,"endpointFingerprint":"/run/user/1000/docker.sock","daemonFingerprint":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"#,
    )
    .unwrap();

    assert!(load_from(&path).1.is_some());
    reconcile_at(
        &path,
        observation(
            1000,
            Some(1000),
            Some(ENDPOINT_A),
            Some(DAEMON_A),
            Some(true),
        ),
    )
    .unwrap();
    assert_eq!(
        load_from(&path).0,
        Some(selection(1000, ENDPOINT_A, DAEMON_A))
    );
}
