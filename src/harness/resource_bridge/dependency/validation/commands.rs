use crate::harness::DependencyKind;

pub(crate) fn safe_cargo_restore_command(command: &str) -> bool {
    if command
        .chars()
        .any(|character| ";&|$`<>\\\n\r".contains(character))
    {
        return false;
    }
    let mut tokens = command.split_whitespace();
    if tokens.next() != Some("cargo")
        || !matches!(
            tokens.next(),
            Some("fetch" | "build" | "check" | "test" | "clippy" | "doc" | "bench")
        )
    {
        return false;
    }
    let mut needs_value = false;
    let mut rustc_args = false;
    for token in tokens {
        if rustc_args {
            continue;
        }
        if token == "--" {
            rustc_args = true;
            continue;
        }
        if needs_value {
            if token.starts_with('-') {
                return false;
            }
            needs_value = false;
            continue;
        }
        if matches!(
            token,
            "-p" | "--package" | "--features" | "-F" | "--exclude"
        ) {
            needs_value = true;
            continue;
        }
        if token == "--config"
            || token.starts_with("--config=")
            || token == "--registry"
            || token.starts_with("--registry=")
            || token == "--manifest-path"
            || token.starts_with("--manifest-path=")
            || token == "--target-dir"
            || token.starts_with("--target-dir=")
        {
            return false;
        }
        if !token.starts_with('-') {
            return false;
        }
    }
    !needs_value
}

pub(crate) fn command_mentions_package(command: &str, package: &str) -> bool {
    command.split_whitespace().any(|token| {
        let token = token.trim_matches(['\'', '"', '`', ',', ';']);
        token == package
            || token
                .strip_prefix(package)
                .is_some_and(|suffix| suffix.starts_with('@'))
    })
}

pub(crate) fn safe_npm_add_command(
    command: &str,
    package: &str,
    version: &str,
    requested_registry: Option<&url::Url>,
    kind: DependencyKind,
) -> bool {
    if command
        .chars()
        .any(|character| ";&|$`<>\\\n\r".contains(character))
    {
        return false;
    }
    let mut tokens = command.split_whitespace();
    if tokens.next() != Some("npm") || !matches!(tokens.next(), Some("install" | "i")) {
        return false;
    }
    let mut package_count = 0;
    let mut registry_seen = false;
    let mut save_dev = false;
    let mut save_prod = false;
    for raw in tokens {
        let token = raw.trim_matches(['\'', '"']);
        match token {
            "--save-dev" | "-D" => {
                save_dev = true;
                continue;
            }
            "--save-prod" => {
                save_prod = true;
                continue;
            }
            "--save-exact" => continue,
            _ => {}
        }
        if let Some(registry) = token
            .strip_prefix("--registry=")
            .map(|registry| registry.trim_matches(['\'', '"']))
        {
            let Some(requested_registry) = requested_registry else {
                return false;
            };
            let Ok(registry) = url::Url::parse(registry) else {
                return false;
            };
            if registry_seen || registry != *requested_registry {
                return false;
            }
            registry_seen = true;
            continue;
        }
        let matches_requested = if token == package {
            version == "latest"
        } else {
            token
                .strip_prefix(package)
                .and_then(|suffix| suffix.strip_prefix('@'))
                == Some(version)
        };
        if !matches_requested {
            return false;
        }
        package_count += 1;
    }
    let scope_matches = match kind {
        DependencyKind::DevelopmentDependency => save_dev && !save_prod,
        DependencyKind::NewProjectDependency => !save_dev,
        _ => false,
    };
    package_count == 1
        && scope_matches
        && requested_registry.is_some_and(|registry| {
            registry.host_str() == Some("registry.npmjs.org") || registry_seen
        })
}

pub(crate) fn safe_npm_restore_command(
    command: &str,
    requested_registry: Option<&url::Url>,
) -> bool {
    if command
        .chars()
        .any(|character| ";&|$`<>\\\n\r".contains(character))
    {
        return false;
    }
    let mut tokens = command.split_whitespace();
    if tokens.next() != Some("npm") || !matches!(tokens.next(), Some("ci" | "install" | "i")) {
        return false;
    }
    let mut registry_seen = false;
    for token in tokens {
        if let Some(value) = token.strip_prefix("--registry=") {
            let Some(requested_registry) = requested_registry else {
                return false;
            };
            let Ok(registry) = url::Url::parse(value.trim_matches(['\'', '"'])) else {
                return false;
            };
            if registry_seen || registry != *requested_registry {
                return false;
            }
            registry_seen = true;
            continue;
        }
        if !matches!(
            token,
            "--ignore-scripts"
                | "--no-audit"
                | "--no-fund"
                | "--offline"
                | "--prefer-offline"
                | "--strict-peer-deps"
                | "--legacy-peer-deps"
                | "--include=dev"
                | "--include=optional"
                | "--omit=optional"
                | "--omit=dev"
        ) {
            return false;
        }
    }
    let custom_registry = requested_registry
        .is_some_and(|registry| registry.host_str() != Some("registry.npmjs.org"));
    !custom_registry || registry_seen
}

pub(crate) fn valid_package_identity(package: &str) -> bool {
    if package.is_empty() || package.len() > 214 {
        return false;
    }
    let valid_component = |component: &str| {
        !component.is_empty()
            && !component
                .chars()
                .next()
                .is_some_and(|character| matches!(character, '.' | '_' | '-'))
            && !component.ends_with('.')
            && component.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            })
    };
    if let Some(scoped) = package.strip_prefix('@') {
        let Some((scope, name)) = scoped.split_once('/') else {
            return false;
        };
        !name.contains('/') && valid_component(scope) && valid_component(name)
    } else {
        !package.contains('/') && valid_component(package)
    }
}

pub(crate) fn valid_version(version: &str) -> bool {
    !version.trim().is_empty()
        && version.len() <= 128
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "*^~<>=.|+- ".contains(c))
}

pub(crate) fn valid_exact_version(version: &str) -> bool {
    version.contains('.')
        && !version.chars().any(|c| "*^~<>=| ".contains(c))
        && valid_version(version)
}
