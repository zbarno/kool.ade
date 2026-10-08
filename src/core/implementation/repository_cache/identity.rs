use sha2::{Digest, Sha256};
use std::path::Path;

pub(super) fn identity_for_repository(
    remote: &str,
    fetch_url: Option<&str>,
    push_url: Option<&str>,
    repo: &Path,
) -> anyhow::Result<String> {
    let origin = if remote.trim().is_empty() {
        format!("local:{}", repo.canonicalize()?.to_string_lossy())
    } else {
        format!("remote:{}", sanitize_remote(remote)?)
    };
    let fetch = fetch_url
        .map(sanitize_remote)
        .transpose()?
        .unwrap_or_default();
    let push = push_url
        .map(sanitize_remote)
        .transpose()?
        .unwrap_or_default();
    let identity = format!("{origin}\nfetch:{fetch}\npush:{push}");
    Ok(format!("{:x}", Sha256::digest(identity.as_bytes())))
}

pub(super) fn sanitize_remote(remote: &str) -> anyhow::Result<String> {
    let remote = remote.trim();
    anyhow::ensure!(
        !remote.is_empty() && !remote.chars().any(char::is_control),
        "Repository remote is empty or contains control characters"
    );
    let Some((scheme, rest)) = remote.split_once("://") else {
        return Ok(remote.to_owned());
    };
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return Ok(remote.to_owned());
    }
    let without_query = rest.split(['?', '#']).next().unwrap_or(rest);
    let authority_end = without_query.find('/').unwrap_or(without_query.len());
    let authority = &without_query[..authority_end];
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    anyhow::ensure!(!host.is_empty(), "Repository remote has no host");
    Ok(format!(
        "{scheme}://{host}{}",
        &without_query[authority_end..]
    ))
}
