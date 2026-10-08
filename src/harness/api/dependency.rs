use serde::{Deserialize, Serialize};

/// Package ecosystem reported by an implementation worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageEcosystem {
    Npm,
    Pnpm,
    Yarn,
    Cargo,
    Nuget,
    Pip,
    Uv,
    Poetry,
    System,
    Other,
}

/// Why the worker needs a package operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    ExistingRestore,
    NewProjectDependency,
    DevelopmentDependency,
    SystemTool,
}

/// Package identity read from an app-validated lockfile.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyPackageIdentity {
    pub package: String,
    pub version: String,
    pub source: String,
    pub integrity: String,
}

/// Worker supplied details. Task identity and authorization are application-owned.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyNeed {
    pub ecosystem: PackageEcosystem,
    pub package: Option<String>,
    pub version: Option<String>,
    pub source: Option<String>,
    pub command: String,
    pub reason: String,
    pub kind: DependencyKind,
    /// App-computed identity for the exact lockfile package set being restored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lockfile_identity: Option<String>,
    /// App-computed package entries not present in the task's initial commit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub introduced_packages: Vec<DependencyPackageIdentity>,
}

/// Kool.ad/e Man.ager's triage result. Broker policy still validates every grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyDecision {
    AutoAuthorize,
    AuthorizeForTask,
    AuthorizeForProject,
    UserAuthorizeForTask,
    UserAuthorizeForProject,
    RequiresUserAuthorization,
    Reject,
}

/// Persistent or one-run scope selected by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyAuthorizationScope {
    Once,
    Project,
}

/// Lifecycle state of a dependency request in task activity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyRequestStatus {
    Pending,
    ManagerReviewing,
    AwaitingUser,
    Authorized,
    Denied,
    Prepared,
    Failed,
}

/// Typed result returned by a package-manager adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyPreparationStatus {
    Prepared,
    AlreadyAvailable,
    AuthorizationRequired,
    Denied,
    Unsupported,
    IntegrityFailure,
    SourceRejected,
    CredentialsRequired,
    Error,
}

/// Where the effective dependency grant came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyAuthorizationSource {
    Automatic,
    Manager,
    User,
}

/// Outcome of the single bounded offline package-operation retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyRetryResult {
    Succeeded,
    Failed,
}

/// Structured broker result and non-secret activity counters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DependencyPreparationTelemetry {
    pub status: Option<DependencyPreparationStatus>,
    pub package_count: u64,
    pub cache_hits: u64,
    pub packages_downloaded: u64,
    pub bytes_downloaded: u64,
    pub authorization_source: Option<DependencyAuthorizationSource>,
    pub retry_result: Option<DependencyRetryResult>,
}

impl DependencyRequestStatus {
    pub(crate) fn is_active(self) -> bool {
        matches!(
            self,
            Self::Pending | Self::ManagerReviewing | Self::AwaitingUser | Self::Authorized
        )
    }
}

/// Stable classification for dependency activity and recovery decisions.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyFailureCategory {
    #[default]
    Unknown,
    DependencyMissing,
    DependencyRestoreRequired,
    DependencyNewPackageRequested,
    DependencySourceNotAuthorized,
    DependencyManagerUnsupported,
    DependencyIntegrityFailure,
    DependencyPrivateRegistry,
    DependencySystemPackageRequired,
    DependencyPolicyDenied,
}

/// App-authored record attached to one task's private activity snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DependencyRequest {
    pub id: String,
    pub task_id: String,
    pub need: DependencyNeed,
    #[serde(default)]
    pub category: DependencyFailureCategory,
    pub decision: DependencyDecision,
    pub rationale: String,
    pub risk: String,
    pub status: DependencyRequestStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preparation: Option<DependencyPreparationTelemetry>,
}

impl DependencyRequest {
    pub fn summary(&self) -> String {
        let package = self.need.package.as_deref().unwrap_or({
            if self.need.introduced_packages.is_empty() {
                "unspecified package"
            } else {
                "new lockfile package set"
            }
        });
        let version = self
            .need
            .version
            .as_deref()
            .map(|version| format!("@{version}"))
            .unwrap_or_default();
        let source = self
            .need
            .source
            .as_deref()
            .unwrap_or("source not specified");
        let state = match self.status {
            DependencyRequestStatus::Pending | DependencyRequestStatus::ManagerReviewing => {
                "Dependency request"
            }
            DependencyRequestStatus::AwaitingUser => "Dependency authorization required",
            DependencyRequestStatus::Authorized => "Dependency authorized",
            DependencyRequestStatus::Prepared => "Dependency prepared",
            DependencyRequestStatus::Denied => "Dependency request denied",
            DependencyRequestStatus::Failed => "Dependency preparation failed",
        };
        let ecosystem = match self.need.ecosystem {
            PackageEcosystem::Npm => "npm",
            PackageEcosystem::Pnpm => "pnpm",
            PackageEcosystem::Yarn => "Yarn",
            PackageEcosystem::Cargo => "Cargo",
            PackageEcosystem::Nuget => "NuGet",
            PackageEcosystem::Pip => "pip",
            PackageEcosystem::Uv => "uv",
            PackageEcosystem::Poetry => "Poetry",
            PackageEcosystem::System => "system package manager",
            PackageEcosystem::Other => "unsupported package manager",
        };
        format!(
            "{state} for {package}{version} ({ecosystem}) from {source}: {}",
            self.rationale
        )
    }
}

#[cfg(test)]
mod tests {
    use super::DependencyRequestStatus as Status;

    #[test]
    fn dependency_activity_only_keeps_nonterminal_requests_actionable() {
        for status in [
            Status::Pending,
            Status::ManagerReviewing,
            Status::AwaitingUser,
            Status::Authorized,
        ] {
            assert!(status.is_active(), "{status:?}");
        }
        for status in [Status::Denied, Status::Prepared, Status::Failed] {
            assert!(!status.is_active(), "{status:?}");
        }
    }
}
