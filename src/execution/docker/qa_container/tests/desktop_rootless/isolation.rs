pub(super) async fn assert_desktop_container_isolated(
    docker: &bollard::Docker,
    container_id: &str,
    runtime_user: &str,
    expected_env: &[String],
    image_env: &[String],
) {
    let inspect = docker.inspect_container(container_id, None).await.unwrap();
    let host = inspect
        .host_config
        .expect("Docker reports applied host policy");
    assert_eq!(host.privileged, Some(false));
    assert_eq!(host.readonly_rootfs, Some(true));
    assert_eq!(host.network_mode.as_deref(), Some("none"));
    assert_eq!(host.ipc_mode.as_deref(), Some("private"));
    assert!(host.pid_mode.as_deref().unwrap_or_default().is_empty());
    assert_eq!(
        host.cap_drop.as_deref(),
        Some(["ALL".to_owned()].as_slice())
    );
    assert!(host.cap_add.unwrap_or_default().is_empty());
    assert!(host.devices.unwrap_or_default().is_empty());
    assert!(host.device_requests.unwrap_or_default().is_empty());
    assert!(host.device_cgroup_rules.unwrap_or_default().is_empty());
    assert!(host.binds.unwrap_or_default().is_empty());
    assert!(host.mounts.unwrap_or_default().is_empty());
    assert_ne!(host.publish_all_ports, Some(true));
    assert!(host.port_bindings.unwrap_or_default().is_empty());
    assert!(host.volumes_from.unwrap_or_default().is_empty());
    assert_eq!(
        host.security_opt.as_deref(),
        Some(["no-new-privileges:true".to_owned()].as_slice())
    );

    let config = inspect
        .config
        .expect("Docker reports applied container config");
    assert_eq!(config.user.as_deref(), Some(runtime_user));
    let mut allowed_env = expected_env.to_vec();
    allowed_env.extend_from_slice(image_env);
    allowed_env.sort();
    allowed_env.dedup();
    let mut actual_env = config.env.unwrap_or_default();
    actual_env.sort();
    assert_eq!(actual_env, allowed_env);
    assert!(
        allowed_env
            .iter()
            .all(|entry| !entry.starts_with("QA_RELAY_TOKEN="))
    );
}
