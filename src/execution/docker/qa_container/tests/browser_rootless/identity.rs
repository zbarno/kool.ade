/// Rootless Docker boundary: inspect the live browser driver and ensure it
/// has no host mounts, published ports, or provider/browser-account secrets.
/// The only credential is the exact run-scoped relay token; its home is fresh
/// tmpfs. A container-config unit test cannot prove the launched container
/// retained that isolation. Requires the existing rootless browser fixture
/// and candidate image; adds one inspect call and no new container.
pub(super) async fn assert_no_host_browser_identity(
    docker: &bollard::Docker,
    container_id: &str,
    relay_token: &str,
) -> Result<(), String> {
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
    let relay_entries = env
        .iter()
        .filter(|entry| entry.starts_with("QA_RELAY_TOKEN="))
        .collect::<Vec<_>>();
    if !env.iter().any(|entry| entry == "HOME=/tmp")
        || relay_entries.len() != 1
        || relay_entries[0] != &format!("QA_RELAY_TOKEN={relay_token}")
        || env
            .iter()
            .filter_map(|entry| entry.split_once('=').map(|pair| pair.0))
            .any(|name| name != "QA_RELAY_TOKEN" && contains_credential_marker(name))
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
        || mounts.iter().any(|mount| {
            mount.typ.as_deref() == Some("bind")
                || mount.typ.as_deref() == Some("npipe")
                || (mount.typ.as_deref() == Some("tmpfs")
                    && mount
                        .source
                        .as_deref()
                        .is_some_and(|source| !source.is_empty()))
        })
    {
        return Err("browser QA can access host identity, storage, or published ports".into());
    }
    Ok(())
}

fn contains_credential_marker(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("AWS_")
        || name.starts_with("AZURE_")
        || name.starts_with("GITHUB_")
        || name.starts_with("GH_")
        || name.starts_with("CODEX_")
        || name.starts_with("CLAUDE_")
        || name.starts_with("OPENAI_")
        || name.starts_with("ANTHROPIC_")
        || name == "SSH_AUTH_SOCK"
        || name.contains("TOKEN")
        || name.contains("SECRET")
        || name.contains("PASSWORD")
        || name.contains("COOKIE")
        || name.contains("CREDENTIAL")
}
