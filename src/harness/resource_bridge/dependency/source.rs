use crate::harness::{DependencyNeed, PackageEcosystem};
use url::Url;

pub(super) fn private_registry_source(need: &DependencyNeed) -> bool {
    let Some(source) = need.source.as_deref() else {
        return false;
    };
    let Ok(url) = url::Url::parse(source) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    !matches!(
        need.ecosystem,
        PackageEcosystem::System | PackageEcosystem::Other
    ) && !default_source(need.ecosystem).is_some_and(|default| {
        url::Url::parse(default)
            .ok()
            .and_then(|expected| expected.host_str().map(str::to_owned))
            .is_some_and(|expected| expected == host)
    }) && (host.contains("registry") || host.contains("packages"))
}

pub(super) fn approved_source(need: &DependencyNeed) -> bool {
    let Some(source) = need.source.as_deref() else {
        return default_source(need.ecosystem).is_some();
    };
    let expected: &[&str] = match need.ecosystem {
        PackageEcosystem::Npm | PackageEcosystem::Pnpm | PackageEcosystem::Yarn => {
            &["registry.npmjs.org"]
        }
        PackageEcosystem::Cargo => &["index.crates.io", "static.crates.io"],
        PackageEcosystem::Nuget => &["api.nuget.org", "globalcdn.nuget.org"],
        PackageEcosystem::Pip => &["pypi.org"],
        PackageEcosystem::Go => &["proxy.golang.org"],
        PackageEcosystem::Uv | PackageEcosystem::Poetry => &["pypi.org", "files.pythonhosted.org"],
        PackageEcosystem::System | PackageEcosystem::Other => return false,
    };
    let Ok(url) = url::Url::parse(source) else {
        return false;
    };
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|port| port == 443)
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.host_str().is_some_and(|host| expected.contains(&host))
}

pub(super) fn default_source(ecosystem: PackageEcosystem) -> Option<&'static str> {
    match ecosystem {
        PackageEcosystem::Npm | PackageEcosystem::Pnpm | PackageEcosystem::Yarn => {
            Some("https://registry.npmjs.org")
        }
        PackageEcosystem::Cargo => Some("https://index.crates.io"),
        PackageEcosystem::Nuget => Some("https://api.nuget.org"),
        PackageEcosystem::Go => Some("https://proxy.golang.org"),
        PackageEcosystem::Pip | PackageEcosystem::Uv | PackageEcosystem::Poetry => {
            Some("https://pypi.org")
        }
        PackageEcosystem::System | PackageEcosystem::Other => None,
    }
}

pub(in crate::harness::resource_bridge) fn npm_registry_url(source: Option<&str>) -> Option<Url> {
    let source = source.unwrap_or("https://registry.npmjs.org");
    let url = parse_npm_registry_root(source)?;
    let host = url.host_str()?;
    if host != "registry.npmjs.org" && additional_npm_registry_host(source).is_none() {
        return None;
    }
    Some(url)
}

pub(super) fn additional_npm_registry_host(source: &str) -> Option<String> {
    let url = parse_npm_registry_root(source)?;
    let host = url.host_str()?.to_ascii_lowercase();
    (host != "registry.npmjs.org").then_some(host)
}

pub(super) fn custom_npm_registry_path_unsupported(need: &DependencyNeed) -> bool {
    if need.ecosystem != PackageEcosystem::Npm {
        return false;
    }
    let Some(source) = need.source.as_deref() else {
        return false;
    };
    let Ok(url) = Url::parse(source) else {
        return false;
    };
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host != "registry.npmjs.org")
        && url.path() != "/"
}

fn parse_npm_registry_root(source: &str) -> Option<Url> {
    if source.len() > 2_048
        || source.contains('\\')
        || source.contains('%')
        || source.contains("/../")
        || source.contains("/./")
        || source.ends_with("/..")
        || source.ends_with("/.")
    {
        return None;
    }
    let url = Url::parse(source).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || host.parse::<std::net::IpAddr>().is_ok()
        || !host.contains('.')
        || host.ends_with('.')
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return None;
    }
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::{approved_source, default_source};
    use crate::harness::{DependencyKind, DependencyNeed, PackageEcosystem};

    fn pip_need(source: Option<&str>) -> DependencyNeed {
        DependencyNeed {
            ecosystem: PackageEcosystem::Pip,
            package: None,
            version: None,
            source: source.map(str::to_owned),
            command: "python3 -m pip install --require-hashes --no-deps -r requirements.txt".into(),
            reason: "Restore the locked Python runtime packages".into(),
            kind: DependencyKind::ExistingRestore,
            lockfile_identity: None,
            introduced_packages: Vec::new(),
        }
    }

    #[test]
    fn pip_registry_source_is_the_public_pypi_origin_only() {
        assert_eq!(
            default_source(PackageEcosystem::Pip),
            Some("https://pypi.org")
        );
        assert!(approved_source(&pip_need(None)));
        assert!(approved_source(&pip_need(Some("https://pypi.org/"))));
        for source in [
            "https://files.pythonhosted.org/",
            "https://pypi.org/simple",
            "http://pypi.org/",
            "https://pypi.org/?index=other",
            "https://pypi.org.attacker.example/",
        ] {
            assert!(
                !approved_source(&pip_need(Some(source))),
                "accepted unsafe Pip index {source}"
            );
        }
    }
}
