use crate::harness::PackageEcosystem;

pub(super) fn under_dependency_root(path: &str, dependency_relative_root: &str) -> bool {
    dependency_relative_root == "."
        || path == dependency_relative_root
        || path
            .strip_prefix(dependency_relative_root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub(super) fn strip_dependency_root<'a>(
    path: &'a str,
    dependency_relative_root: &str,
) -> anyhow::Result<&'a str> {
    if dependency_relative_root == "." {
        return Ok(path);
    }
    path.strip_prefix(dependency_relative_root)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .ok_or_else(|| anyhow::anyhow!("baseline dependency path escaped its selected target root"))
}

pub(super) fn is_npm_package_manifest(path: &str) -> bool {
    path.rsplit('/').next() == Some("package.json")
        && !path.split('/').any(|part| {
            matches!(
                part,
                ".git"
                    | "node_modules"
                    | ".koolade-packet"
                    | "target"
                    | "dist"
                    | "build"
                    | "coverage"
                    | "vendor"
                    | "obj"
                    | "bin"
            )
        })
}

fn is_lockfile(path: &str, ecosystem: PackageEcosystem) -> bool {
    if ecosystem == PackageEcosystem::Npm
        && path.split('/').any(|part| {
            matches!(
                part,
                ".git"
                    | "node_modules"
                    | ".koolade-packet"
                    | "target"
                    | "dist"
                    | "build"
                    | "coverage"
                    | "vendor"
                    | "obj"
                    | "bin"
            )
        })
    {
        return false;
    }
    match ecosystem {
        PackageEcosystem::Npm => matches!(
            path.rsplit('/').next(),
            Some("package-lock.json" | "npm-shrinkwrap.json")
        ),
        PackageEcosystem::Cargo => path == "Cargo.lock",
        _ => false,
    }
}

pub(super) fn is_target_lockfile(
    path: &str,
    ecosystem: PackageEcosystem,
    dependency_relative_root: &str,
) -> bool {
    if ecosystem == PackageEcosystem::Cargo {
        return if dependency_relative_root == "." {
            path == "Cargo.lock"
        } else {
            format!("{dependency_relative_root}/Cargo.lock") == path
        };
    }
    under_dependency_root(path, dependency_relative_root) && is_lockfile(path, ecosystem)
}
