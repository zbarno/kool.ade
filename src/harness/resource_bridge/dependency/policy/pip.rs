use crate::harness::{DependencyKind, DependencyNeed};

pub(super) fn restore_decision_allowed(need: &DependencyNeed, source_ok: bool) -> bool {
    need.kind == DependencyKind::ExistingRestore
        && need.package.is_none()
        && need.version.is_none()
        && super::valid_lockfile_identity(need)
        && super::valid_introduced_packages(need, false)
        && source_ok
        && crate::harness::resource_bridge::dependency::validation::safe_pip_restore_command(
            &need.command,
        )
}

pub(super) fn valid_introduced_packages(need: &DependencyNeed) -> bool {
    need.introduced_packages.iter().all(|package| {
        package.source == "https://pypi.org"
            && crate::harness::resource_bridge::dependency::validation::valid_package_identity(
                &package.package,
            )
            && valid_pip_version(&package.version)
            && valid_pip_integrity(&package.integrity)
    })
}

fn valid_pip_version(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().any(|byte| byte.is_ascii_digit())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'_'))
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
}

fn valid_pip_integrity(value: &str) -> bool {
    let Some(hashes) = value.strip_prefix("sha256:") else {
        return false;
    };
    let hashes = hashes.split(',').collect::<Vec<_>>();
    !hashes.is_empty()
        && hashes.len() <= 16
        && hashes
            .iter()
            .all(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
