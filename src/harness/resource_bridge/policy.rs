use url::Url;

pub(super) enum Decision {
    Allow(Url),
    NeedsAttention(String),
}

pub(super) fn classify(raw: &str) -> anyhow::Result<Decision> {
    anyhow::ensure!(raw.len() <= 2_048, "Resource URL exceeds the size limit");
    anyhow::ensure!(
        !raw.chars().any(char::is_control),
        "Resource URL contains control characters"
    );
    let raw_lower = raw.to_ascii_lowercase();
    anyhow::ensure!(
        !raw_lower.contains("/../")
            && !raw_lower.ends_with("/..")
            && !raw_lower.contains("/./")
            && !raw_lower.ends_with("/.")
            && !raw_lower.contains("%2e")
            && !raw.contains('\\'),
        "Resource URL contains an ambiguous path"
    );
    let url = Url::parse(raw)?;
    anyhow::ensure!(
        url.scheme() == "https",
        "Only HTTPS resources can be requested"
    );
    anyhow::ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443)
            && url.port().is_none_or(|port| port == 443)
            && url.fragment().is_none(),
        "Resource URL contains credentials, a nonstandard port, or a fragment"
    );
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Resource URL has no host"))?
        .to_ascii_lowercase();
    anyhow::ensure!(
        url.query().is_none(),
        "Resource URLs with query parameters require operator review"
    );
    if host == "registry.npmjs.org" && safe_npm_path(url.path()) {
        return Ok(Decision::Allow(url));
    }
    let display = format!("{host}{}", url.path());
    Ok(Decision::NeedsAttention(format!(
        "The worker requested an internet resource from {display}. Kool.ad/e only auto-retrieves public npm registry packages at this time; this source needs review before it can be fetched."
    )))
}

fn safe_npm_path(path: &str) -> bool {
    let value = path.strip_prefix('/').unwrap_or_default();
    if value.is_empty() || value.len() > 512 {
        return false;
    }
    let segments = value.split('/').collect::<Vec<_>>();
    let package = segments.first().copied().unwrap_or_default();
    if !safe_package_name(package) {
        return false;
    }
    match segments.as_slice() {
        [_] => true,
        [_, version] => safe_version(version),
        [_, dash, filename] if *dash == "-" => safe_tarball(filename),
        _ => false,
    }
}

fn safe_package_name(value: &str) -> bool {
    let value = value.replace("%2f", "/").replace("%2F", "/");
    let value = value.strip_prefix('@').unwrap_or(&value);
    !value.is_empty()
        && value.split('/').all(|part| {
            !part.is_empty()
                && part.len() <= 128
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte))
        })
}

fn safe_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._+~".contains(&byte))
}

fn safe_tarball(value: &str) -> bool {
    value.ends_with(".tgz") && safe_version(value)
}
