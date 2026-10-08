use crate::harness::{DependencyKind, DependencyNeed, PackageEcosystem};

mod authorization;
mod redaction;

fn lockfile_identity() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn npm_add(source: Option<&str>, command: &str, version: &str) -> DependencyNeed {
    DependencyNeed {
        ecosystem: PackageEcosystem::Npm,
        package: Some("zod".into()),
        version: Some(version.into()),
        source: source.map(str::to_owned),
        command: command.into(),
        reason: "Validate imported settings data".into(),
        kind: DependencyKind::NewProjectDependency,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    }
}
