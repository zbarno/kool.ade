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
        PackageEcosystem::Pip => path == "requirements.txt",
        PackageEcosystem::Go => path == "go.sum",
        _ => false,
    }
}

pub(super) fn is_target_lockfile(
    path: &str,
    ecosystem: PackageEcosystem,
    dependency_relative_root: &str,
) -> bool {
    if matches!(
        ecosystem,
        PackageEcosystem::Cargo | PackageEcosystem::Pip | PackageEcosystem::Go
    ) {
        let filename = match ecosystem {
            PackageEcosystem::Cargo => "Cargo.lock",
            PackageEcosystem::Pip => "requirements.txt",
            PackageEcosystem::Go => "go.sum",
            _ => unreachable!(),
        };
        return if dependency_relative_root == "." {
            path == filename
        } else {
            format!("{dependency_relative_root}/{filename}") == path
        };
    }
    under_dependency_root(path, dependency_relative_root) && is_lockfile(path, ecosystem)
}
