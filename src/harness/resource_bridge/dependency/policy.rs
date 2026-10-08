use super::{
    source::{additional_npm_registry_host, approved_source, npm_registry_url},
    validation::{
        command_mentions_package, safe_cargo_restore_command, safe_npm_add_command,
        safe_npm_restore_command, valid_exact_version, valid_package_identity, valid_version,
    },
};
use crate::harness::{DependencyDecision, DependencyKind, DependencyNeed, PackageEcosystem};

const MAX_INTRODUCED_PACKAGES: usize = 64;

pub(crate) fn decision_allowed(need: &DependencyNeed, decision: DependencyDecision) -> bool {
    let source_ok = approved_source(need);
    let user_authorized_npm_source = source_ok
        || (need.ecosystem == PackageEcosystem::Npm
            && need
                .source
                .as_deref()
                .is_some_and(|source| additional_npm_registry_host(source).is_some()));
    let package_ok = need.package.as_deref().is_some_and(valid_package_identity);
    let command_matches = need
        .package
        .as_deref()
        .is_some_and(|package| command_mentions_package(&need.command, package));
    let add_command_matches = need
        .package
        .as_deref()
        .zip(need.version.as_deref())
        .is_some_and(|(package, version)| {
            safe_npm_add_command(
                &need.command,
                package,
                version,
                npm_registry_url(need.source.as_deref()).as_ref(),
                need.kind,
            )
        });
    let new_add_inputs_unchanged = !matches!(
        need.kind,
        DependencyKind::NewProjectDependency | DependencyKind::DevelopmentDependency
    ) || (valid_lockfile_identity(need)
        && need.introduced_packages.is_empty());
    let requested_registry = npm_registry_url(need.source.as_deref());
    let manager_grant = matches!(
        decision,
        DependencyDecision::AutoAuthorize
            | DependencyDecision::AuthorizeForTask
            | DependencyDecision::AuthorizeForProject
    );
    let user_grant = matches!(
        decision,
        DependencyDecision::UserAuthorizeForTask | DependencyDecision::UserAuthorizeForProject
    );
    if (manager_grant || user_grant) && need.ecosystem == PackageEcosystem::Cargo {
        return need.kind == DependencyKind::ExistingRestore
            && need.package.is_none()
            && need.version.is_none()
            && valid_lockfile_identity(need)
            && valid_introduced_packages(need, false)
            && source_ok
            && safe_cargo_restore_command(&need.command);
    }
    let lockfile_restore = need.ecosystem == PackageEcosystem::Npm
        && need.kind == DependencyKind::ExistingRestore
        && need.package.is_none()
        && need.version.is_none()
        && valid_lockfile_identity(need)
        && valid_introduced_packages(need, false)
        && source_ok
        && safe_npm_restore_command(&need.command, requested_registry.as_ref());
    if lockfile_restore
        && matches!(
            decision,
            DependencyDecision::AutoAuthorize
                | DependencyDecision::AuthorizeForTask
                | DependencyDecision::AuthorizeForProject
                | DependencyDecision::UserAuthorizeForTask
                | DependencyDecision::UserAuthorizeForProject
        )
    {
        return true;
    }
    let user_registry_lockfile_restore = need.ecosystem == PackageEcosystem::Npm
        && need.kind == DependencyKind::ExistingRestore
        && need.package.is_none()
        && need.version.is_none()
        && valid_lockfile_identity(need)
        && valid_introduced_packages(need, true)
        && user_authorized_npm_source
        && safe_npm_restore_command(&need.command, requested_registry.as_ref());
    if user_registry_lockfile_restore
        && matches!(
            decision,
            DependencyDecision::UserAuthorizeForTask | DependencyDecision::UserAuthorizeForProject
        )
    {
        return true;
    }
    match decision {
        DependencyDecision::AutoAuthorize => {
            need.ecosystem == PackageEcosystem::Npm
                && matches!(
                    need.kind,
                    DependencyKind::ExistingRestore
                        | DependencyKind::NewProjectDependency
                        | DependencyKind::DevelopmentDependency
                )
                && source_ok
                && package_ok
                && new_add_inputs_unchanged
                && need.version.as_deref().is_some_and(|version| {
                    if need.kind == DependencyKind::ExistingRestore {
                        command_matches && valid_exact_version(version)
                    } else {
                        add_command_matches && valid_version(version)
                    }
                })
        }
        DependencyDecision::AuthorizeForTask | DependencyDecision::AuthorizeForProject => {
            need.ecosystem == PackageEcosystem::Npm
                && matches!(
                    need.kind,
                    DependencyKind::ExistingRestore
                        | DependencyKind::NewProjectDependency
                        | DependencyKind::DevelopmentDependency
                )
                && source_ok
                && package_ok
                && new_add_inputs_unchanged
                && need.version.as_deref().is_some_and(|version| {
                    if need.kind == DependencyKind::ExistingRestore {
                        command_matches && valid_exact_version(version)
                    } else {
                        add_command_matches && valid_version(version)
                    }
                })
        }
        DependencyDecision::UserAuthorizeForTask | DependencyDecision::UserAuthorizeForProject => {
            need.ecosystem == PackageEcosystem::Npm
                && matches!(
                    need.kind,
                    DependencyKind::ExistingRestore
                        | DependencyKind::NewProjectDependency
                        | DependencyKind::DevelopmentDependency
                )
                && user_authorized_npm_source
                && package_ok
                && new_add_inputs_unchanged
                && need.version.as_deref().is_some_and(|version| {
                    if need.kind == DependencyKind::ExistingRestore {
                        command_matches && valid_exact_version(version)
                    } else {
                        add_command_matches && valid_version(version)
                    }
                })
        }
        DependencyDecision::RequiresUserAuthorization | DependencyDecision::Reject => false,
    }
}

fn valid_lockfile_identity(need: &DependencyNeed) -> bool {
    need.lockfile_identity
        .as_deref()
        .and_then(|identity| identity.strip_prefix("sha256:"))
        .is_some_and(|digest| {
            digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

fn valid_introduced_packages(need: &DependencyNeed, allow_custom_npm: bool) -> bool {
    if need.introduced_packages.len() > MAX_INTRODUCED_PACKAGES {
        return false;
    }
    match need.ecosystem {
        PackageEcosystem::Npm => {
            let Some(registry) = npm_registry_url(need.source.as_deref()) else {
                return false;
            };
            let custom_source_allowed = allow_custom_npm
                && additional_npm_registry_host(need.source.as_deref().unwrap_or_default())
                    .is_some();
            let public_registry_origin = url::Url::parse("https://registry.npmjs.org/")
                .expect("static npm registry URL is valid")
                .origin();
            need.introduced_packages.iter().all(|package| {
                let Ok(source) = url::Url::parse(&package.source) else {
                    return false;
                };
                let public_npm_package = source.origin() == public_registry_origin
                    && source.host_str() == Some("registry.npmjs.org");
                let user_approved_registry_package =
                    custom_source_allowed && source.origin() == registry.origin();
                (public_npm_package || user_approved_registry_package)
                    && super::validation::valid_package_identity(&package.package)
                    && super::validation::valid_exact_version(&package.version)
                    && package.integrity.starts_with("sha512-")
                    && package.integrity.len() <= 256
                    && package
                        .integrity
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"+/=-".contains(&byte))
            })
        }
        PackageEcosystem::Cargo => need.introduced_packages.iter().all(|package| {
            package.source == "https://index.crates.io"
                && super::validation::valid_package_identity(&package.package)
                && super::validation::valid_exact_version(&package.version)
                && package.integrity.len() == 71
                && package.integrity.starts_with("sha256:")
                && package.integrity[7..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
        }),
        _ => need.introduced_packages.is_empty(),
    }
}

/// Man.ager may recommend decisions only within the automatic public-source
/// policy. Custom registry grants can only originate from an explicit user
/// action, represented by the separate `UserAuthorize*` decisions.
pub(crate) fn manager_decision_allowed(
    need: &DependencyNeed,
    decision: DependencyDecision,
) -> bool {
    matches!(
        decision,
        DependencyDecision::AutoAuthorize
            | DependencyDecision::AuthorizeForTask
            | DependencyDecision::AuthorizeForProject
    ) && decision_allowed(need, decision)
}
