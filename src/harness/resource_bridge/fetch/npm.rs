use super::{ResourceResponse, policy, retrieve_validated};
use std::path::Path;
use url::Url;

pub fn retrieve_npm_registry(
    worktree: &Path,
    raw_url: &str,
    purpose: &str,
    remaining_bytes: u64,
    authorized_registry: &Url,
) -> anyhow::Result<ResourceResponse> {
    anyhow::ensure!(
        (3..=240).contains(&purpose.trim().len()) && !purpose.chars().any(char::is_control),
        "Explain briefly what this resource is needed for"
    );
    let current = match policy::classify_npm_registry_package(raw_url, authorized_registry) {
        Ok(policy::Decision::Allow(url)) => url,
        Err(error) => {
            return Ok(ResourceResponse::needs_attention(format!(
                "The npm lockfile resource needs operator review: {error:#}"
            )));
        }
        Ok(policy::Decision::NeedsAttention(detail)) => {
            return Ok(ResourceResponse::needs_attention(detail));
        }
    };
    // Packages already locked to the public npm registry keep that origin.
    // Custom registry archives stay on the exact user-approved host and path
    // prefix through every redirect.
    let redirect_registry = if current.host_str() == Some("registry.npmjs.org") {
        Url::parse("https://registry.npmjs.org/")?
    } else {
        authorized_registry.clone()
    };
    retrieve_validated(worktree, current, purpose, remaining_bytes, |url| {
        policy::classify_npm_registry_redirect(url, &redirect_registry)
    })
}
