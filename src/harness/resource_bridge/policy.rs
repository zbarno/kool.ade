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

pub(super) fn classify_npm_registry_package(
    raw: &str,
    authorized_registry: &Url,
) -> anyhow::Result<Decision> {
    let default_decision = classify(raw)?;
    if authorized_registry.host_str() == Some("registry.npmjs.org") {
        return Ok(default_decision);
    }
    if matches!(&default_decision, Decision::Allow(_)) {
        return Ok(default_decision);
    }
    classify_npm_registry_redirect(raw, authorized_registry)
}

/// Validate a package URL or redirect against one registry origin. For an
/// additional registry, this deliberately does not allow a redirect to the
/// default registry or a CDN, even though those are otherwise public hosts.
pub(super) fn classify_npm_registry_redirect(
    raw: &str,
    allowed_registry: &Url,
) -> anyhow::Result<Decision> {
    let common = classify(raw)?;
    let url = Url::parse(raw)?;
    if allowed_registry.host_str() == Some("registry.npmjs.org") {
        return Ok(common);
    }
    // `classify` returns NeedsAttention for custom hosts after completing its
    // HTTPS, port, credential, query, and path-ambiguity checks.
    let host = url.host_str().unwrap_or_default();
    let prefix = allowed_registry.path();
    let relative = url.path().strip_prefix(prefix).unwrap_or_default();
    let package_path = format!("/{relative}");
    if host == allowed_registry.host_str().unwrap_or_default()
        && !relative.is_empty()
        && safe_npm_path(&package_path)
    {
        return Ok(Decision::Allow(url));
    }
    Ok(Decision::NeedsAttention(format!(
        "An npm package URL or redirect is outside the authorized registry {}.",
        allowed_registry.host_str().unwrap_or_default()
    )))
}

fn safe_npm_path(path: &str) -> bool {
    let value = path.strip_prefix('/').unwrap_or_default();
    if value.is_empty() || value.len() > 512 {
        return false;
    }
    let segments = value.split('/').collect::<Vec<_>>();
    let (package, remainder) = if segments
        .first()
        .is_some_and(|part| part.starts_with('@') && !part.to_ascii_lowercase().contains("%2f"))
    {
        let Some(scope_package) = segments.get(1) else {
            return false;
        };
        (format!("{}/{}", segments[0], scope_package), &segments[2..])
    } else {
        (segments[0].to_owned(), &segments[1..])
    };
    if !safe_package_name(&package) {
        return false;
    }
    match remainder {
        [] => true,
        [version] => safe_version(version),
        [dash, filename] if *dash == "-" => safe_tarball(filename),
        _ => false,
    }
}

fn safe_package_name(value: &str) -> bool {
    let decoded = value.replace("%2f", "/").replace("%2F", "/");
    let valid_component = |part: &str| {
        !part.is_empty()
            && part.len() <= 128
            && !part
                .chars()
                .next()
                .is_some_and(|character| matches!(character, '.' | '_' | '-'))
            && !part.ends_with('.')
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte))
    };
    if let Some(scoped) = decoded.strip_prefix('@') {
        scoped.split_once('/').is_some_and(|(scope, name)| {
            !name.contains('/') && valid_component(scope) && valid_component(name)
        })
    } else {
        !decoded.contains('/') && valid_component(&decoded)
    }
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
