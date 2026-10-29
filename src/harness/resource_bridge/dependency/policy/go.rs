use crate::harness::{DependencyKind, DependencyNeed};

pub(super) fn restore_decision_allowed(need: &DependencyNeed, source_ok: bool) -> bool {
    need.kind == DependencyKind::ExistingRestore
        && need.package.is_none()
        && need.version.is_none()
        && super::valid_lockfile_identity(need)
        && super::valid_introduced_packages(need, false)
        && source_ok
        && need.source.as_deref() == Some("https://proxy.golang.org")
        && super::super::validation::safe_go_restore_command(&need.command)
}

pub(super) fn valid_introduced_packages(need: &DependencyNeed) -> bool {
    need.introduced_packages.iter().all(|package| {
        package.source == "https://proxy.golang.org"
            && valid_go_module_path(&package.package)
            && valid_go_sum_version(&package.version)
            && valid_h1(&package.integrity)
    })
}

fn valid_go_sum_version(value: &str) -> bool {
    let version = value.strip_suffix("/go.mod").unwrap_or(value);
    version.starts_with('v')
        && version.len() <= 256
        && version[1..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".+-".contains(&byte))
        && version.split('.').count() >= 3
}

fn valid_go_module_path(value: &str) -> bool {
    value.len() <= 512
        && value.is_ascii()
        && value.split('/').count() >= 2
        && value.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && !part.ends_with('.')
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b".!-_~+".contains(&byte))
        })
        && value
            .split('/')
            .next()
            .is_some_and(|domain| domain.contains('.'))
}

fn valid_h1(value: &str) -> bool {
    value.strip_prefix("h1:").is_some_and(|encoded| {
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .is_ok_and(|digest| digest.len() == 32)
    })
}

use base64::Engine as _;
