/// Rootless Docker boundary: inspect the live browser driver and ensure it
/// has no host mounts, published ports, or provider/browser-account secrets.
/// The only credential is the exact run-scoped relay token; its home is fresh
/// tmpfs. The assertion runs on both the fixture route and selected-task
/// Build/Start/browser path. A container-config unit test cannot prove the
/// launched container retained that isolation. It inspects the image and
/// existing container without creating another container.
pub(super) async fn assert_no_host_browser_identity(
    docker: &bollard::Docker,
    container_id: &str,
    relay_token: &str,
    expected_env: &[String],
    image_reference: &str,
) -> Result<(), String> {
    let image_env = docker
        .inspect_image(image_reference)
        .await
        .map_err(|_| "locked browser QA image could not be inspected".to_owned())?
        .config
        .and_then(|config| config.env)
        .unwrap_or_default();
    let container = docker
        .inspect_container(container_id, None)
        .await
        .map_err(|_| "browser QA container could not be inspected".to_owned())?;
    let config = container
        .config
        .as_ref()
        .ok_or_else(|| "browser QA container config is missing".to_owned())?;
    let host = container
        .host_config
        .as_ref()
        .ok_or_else(|| "browser QA host config is missing".to_owned())?;
    let env = config.env.as_deref().unwrap_or_default();
    let mut allowed_env = expected_env.to_vec();
    allowed_env.extend_from_slice(&image_env);
    allowed_env.sort();
    allowed_env.dedup();
    let mut actual_env = env.to_vec();
    actual_env.sort();
    actual_env.dedup();
    let relay_entries = env
        .iter()
        .filter(|entry| entry.starts_with("QA_RELAY_TOKEN="))
        .collect::<Vec<_>>();
    let home_entries = env
        .iter()
        .filter(|entry| entry.starts_with("HOME="))
        .collect::<Vec<_>>();
    if actual_env != allowed_env
        || home_entries.len() != 1
        || home_entries[0].as_str() != "HOME=/tmp"
        || relay_entries.len() != 1
        || relay_entries[0] != &format!("QA_RELAY_TOKEN={relay_token}")
    {
        return Err("browser QA received ambient or unexpected credential environment".into());
    }
    let mounts = container.mounts.as_deref().unwrap_or_default();
    if !host.binds.as_deref().unwrap_or_default().is_empty()
        || !host.mounts.as_deref().unwrap_or_default().is_empty()
        || host
            .tmpfs
            .as_ref()
            .is_none_or(|tmpfs| !tmpfs.contains_key("/tmp"))
        || host.publish_all_ports == Some(true)
        || host
            .port_bindings
            .as_ref()
            .is_some_and(|ports| !ports.is_empty())
        || mounts
            .iter()
            .any(|mount| mount.typ.as_deref() != Some("tmpfs"))
    {
        return Err("browser QA can access host identity, storage, or published ports".into());
    }
    Ok(())
}
