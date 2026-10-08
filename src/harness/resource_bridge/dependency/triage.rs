use super::{
    source::{
        approved_source, custom_npm_registry_path_unsupported, default_source, npm_registry_url,
        private_registry_source,
    },
    validation::{
        command_mentions_package, redact_and_bound, safe_cargo_restore_command,
        safe_npm_restore_command, validate,
    },
};
use crate::harness::{
    DependencyDecision, DependencyFailureCategory, DependencyKind, DependencyNeed,
    DependencyRequest, DependencyRequestStatus, PackageEcosystem,
};

pub(crate) fn triage(task_id: Option<&str>, mut need: DependencyNeed) -> DependencyRequest {
    let raw_request_valid = redact_and_bound(&mut need) && validate(&need).is_ok();
    let npm_registry = npm_registry_url(need.source.as_deref());
    let (decision, rationale, risk) = if !raw_request_valid {
        (
            DependencyDecision::Reject,
            "The dependency request is incomplete or malformed.".into(),
            "Kool.ad/e rejected the request before package acquisition.".into(),
        )
    } else if need.kind == DependencyKind::SystemTool || need.ecosystem == PackageEcosystem::System
    {
        (
            DependencyDecision::RequiresUserAuthorization,
            "System tools require an explicit operator decision and a separately managed runtime."
                .into(),
            "No host package manager or privileged operation is available to the worker.".into(),
        )
    } else if custom_npm_registry_path_unsupported(&need) {
        (
            DependencyDecision::Reject,
            "This custom npm registry uses a path prefix. The secure npm resolver can restrict custom registries by host, so only origin-root registry URLs are supported.".into(),
            "No permission was granted because the resolver cannot enforce a narrower URL path inside its HTTPS tunnel.".into(),
        )
    } else if !approved_source(&need) {
        (
            DependencyDecision::RequiresUserAuthorization,
            "The requested source is outside this ecosystem's approved public registry.".into(),
            "A custom, private, URL, or Git source needs a separate trust decision.".into(),
        )
    } else if !matches!(
        need.ecosystem,
        PackageEcosystem::Npm | PackageEcosystem::Cargo
    ) {
        (
            DependencyDecision::RequiresUserAuthorization,
            "The request is structured, but this package manager does not yet have a supported acquisition adapter.".into(),
            "Authorization alone cannot make an unsupported package manager safe to run.".into(),
        )
    } else if need.ecosystem == PackageEcosystem::Cargo
        && need.kind == DependencyKind::ExistingRestore
        && safe_cargo_restore_command(&need.command)
    {
        (
            DependencyDecision::RequiresUserAuthorization,
            "Man.ager is checking the Cargo.lock entries and crates.io sources before a bounded offline retry.".into(),
            "Cargo restores are prepared only from checksum-backed crates.io lockfile entries; Git and private registries are rejected.".into(),
        )
    } else if need.ecosystem == PackageEcosystem::Npm
        && need.kind == DependencyKind::ExistingRestore
        && need.package.is_none()
        && need.version.is_none()
        && safe_npm_restore_command(&need.command, npm_registry.as_ref())
    {
        (
            DependencyDecision::RequiresUserAuthorization,
            "Man.ager is checking the project's npm lockfiles before preparing a bounded offline restore.".into(),
            "Only public npm registry archives with matching SHA-512 lockfile integrity are retrieved; package scripts stay inside the sandbox.".into(),
        )
    } else if need.ecosystem == PackageEcosystem::Cargo {
        (
            DependencyDecision::Reject,
            "Kool.ad/e currently prepares locked Cargo restores only; this Cargo operation needs a supported adapter.".into(),
            "No package permission was granted and the worker remains offline.".into(),
        )
    } else if matches!(
        need.kind,
        DependencyKind::NewProjectDependency | DependencyKind::DevelopmentDependency
    ) && need.package.is_some()
        && need.reason.trim().len() >= 12
        && command_mentions_package(&need.command, need.package.as_deref().unwrap())
    {
        (
            DependencyDecision::RequiresUserAuthorization,
            "The public package identity and task-specific reason are recorded for Man.ager triage before the broker grants acquisition.".into(),
            "The package is new to the project and its manifest or lockfile changes require review.".into(),
        )
    } else if need.kind == DependencyKind::ExistingRestore
        || matches!(
            need.kind,
            DependencyKind::NewProjectDependency | DependencyKind::DevelopmentDependency
        )
    {
        (
            DependencyDecision::RequiresUserAuthorization,
            "The request does not contain enough package identity, command, or task rationale for safe manager triage.".into(),
            "The package identity or operation does not match the structured request.".into(),
        )
    } else {
        (
            DependencyDecision::RequiresUserAuthorization,
            "The request does not contain enough package identity and task rationale for safe automatic authorization.".into(),
            "The package identity, version, or purpose is incomplete.".into(),
        )
    };

    let category = if !raw_request_valid {
        DependencyFailureCategory::DependencyPolicyDenied
    } else if need.kind == DependencyKind::SystemTool || need.ecosystem == PackageEcosystem::System
    {
        DependencyFailureCategory::DependencySystemPackageRequired
    } else if !approved_source(&need) {
        if private_registry_source(&need) {
            DependencyFailureCategory::DependencyPrivateRegistry
        } else {
            DependencyFailureCategory::DependencySourceNotAuthorized
        }
    } else if (need.ecosystem == PackageEcosystem::Npm
        && need.kind == DependencyKind::ExistingRestore
        && need.package.is_none()
        && need.version.is_none()
        && safe_npm_restore_command(&need.command, npm_registry.as_ref()))
        || (need.ecosystem == PackageEcosystem::Cargo
            && need.kind == DependencyKind::ExistingRestore
            && safe_cargo_restore_command(&need.command))
    {
        DependencyFailureCategory::DependencyRestoreRequired
    } else if need.ecosystem != PackageEcosystem::Npm {
        DependencyFailureCategory::DependencyManagerUnsupported
    } else if matches!(
        need.kind,
        DependencyKind::NewProjectDependency | DependencyKind::DevelopmentDependency
    ) {
        DependencyFailureCategory::DependencyNewPackageRequested
    } else if need.package.is_none() || need.version.is_none() {
        DependencyFailureCategory::DependencyMissing
    } else {
        DependencyFailureCategory::DependencyRestoreRequired
    };

    DependencyRequest {
        id: format!("dependency-{}", uuid::Uuid::new_v4()),
        task_id: task_id.unwrap_or("unknown-task").to_owned(),
        need,
        category,
        decision,
        rationale,
        risk,
        status: if decision == DependencyDecision::Reject {
            DependencyRequestStatus::Failed
        } else {
            DependencyRequestStatus::Pending
        },
    }
}

pub(crate) fn from_unsupported_manager(manager: Option<&str>, purpose: &str) -> DependencyNeed {
    let ecosystem = match manager.unwrap_or_default().to_ascii_lowercase().as_str() {
        "npm" => PackageEcosystem::Npm,
        "pnpm" => PackageEcosystem::Pnpm,
        "yarn" => PackageEcosystem::Yarn,
        "cargo" => PackageEcosystem::Cargo,
        "nuget" | "dotnet" => PackageEcosystem::Nuget,
        "pip" => PackageEcosystem::Pip,
        "uv" => PackageEcosystem::Uv,
        "poetry" => PackageEcosystem::Poetry,
        "system" => PackageEcosystem::System,
        _ => PackageEcosystem::Other,
    };
    DependencyNeed {
        ecosystem,
        package: None,
        version: None,
        source: default_source(ecosystem).map(str::to_owned),
        command: format!("{} install", manager.unwrap_or("unknown manager")),
        reason: purpose.to_owned(),
        kind: DependencyKind::ExistingRestore,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    }
}
