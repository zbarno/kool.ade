use super::*;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
fn write_rootless_context(config_dir: &Path, host: &str) -> PathBuf {
    let context_dir = config_dir.join("contexts").join("meta").join(context_id());
    fs::create_dir_all(&context_dir).unwrap();
    let directories = [
        config_dir.to_path_buf(),
        config_dir.join("contexts"),
        config_dir.join("contexts/meta"),
        context_dir.clone(),
    ];
    for directory in &directories {
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let metadata_path = context_dir.join("meta.json");
    fs::write(
        &metadata_path,
        serde_json::json!({
            "Name": "rootless",
            "Endpoints": {"docker": {"Host": host}}
        })
        .to_string(),
    )
    .unwrap();
    fs::set_permissions(&metadata_path, fs::Permissions::from_mode(0o600)).unwrap();
    metadata_path
}

#[test]
#[cfg(unix)]
fn accepts_rootless_context_only_when_safe_metadata_matches_the_canonical_endpoint() {
    let config = tempfile::tempdir().unwrap();
    fs::set_permissions(config.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let uid = unsafe { libc::geteuid() };
    let expected = format!("unix:///run/user/{uid}/docker.sock");
    write_rootless_context(config.path(), &expected);
    assert!(verify_rootless_context(Some(config.path()), uid, &expected).is_ok());

    if uid == 0 {
        return;
    }

    for host in [None, Some(expected.as_str())] {
        let endpoint = DockerEndpoint::from_environment_with_config(
            host,
            Some("rootless"),
            None,
            Some(uid),
            HostPlatform::Linux,
            Some(config.path()),
        )
        .unwrap();
        assert_eq!(
            endpoint,
            DockerEndpoint::Unix {
                path: format!("/run/user/{uid}/docker.sock").into(),
                desktop: false,
            }
        );
    }
}

#[test]
#[cfg(unix)]
fn rejects_mismatched_or_symlinked_rootless_context_metadata() {
    let uid = unsafe { libc::geteuid() };
    let expected = format!("unix:///run/user/{uid}/docker.sock");
    let config = tempfile::tempdir().unwrap();
    fs::set_permissions(config.path(), fs::Permissions::from_mode(0o700)).unwrap();
    write_rootless_context(config.path(), "unix:///var/run/docker.sock");
    let reason = verify_rootless_context(Some(config.path()), uid, &expected).unwrap_err();
    assert!(reason.contains("different Docker endpoint"));
    if uid != 0 {
        let error = DockerEndpoint::from_environment_with_config(
            None,
            Some("rootless"),
            None,
            Some(uid),
            HostPlatform::Linux,
            Some(config.path()),
        )
        .unwrap_err();
        assert!(error.message.contains("different Docker endpoint"));
    }

    let config = tempfile::tempdir().unwrap();
    fs::set_permissions(config.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let metadata_path = write_rootless_context(config.path(), &expected);
    let external = config.path().join("external.json");
    fs::rename(&metadata_path, &external).unwrap();
    std::os::unix::fs::symlink(external, &metadata_path).unwrap();
    let reason = verify_rootless_context(Some(config.path()), uid, &expected).unwrap_err();
    assert!(reason.contains("not a safe, bounded"));
    if uid != 0 {
        let error = DockerEndpoint::from_environment_with_config(
            None,
            Some("rootless"),
            None,
            Some(uid),
            HostPlatform::Linux,
            Some(config.path()),
        )
        .unwrap_err();
        assert!(error.message.contains("not a safe, bounded"));
    }
}
