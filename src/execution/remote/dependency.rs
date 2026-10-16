//! Identity-bound dependency broker relay wire values.
//!
//! These values carry requests only. They never carry credentials, cache bytes,
//! approval tokens, or an authorization decision from the worker.

use serde::{Deserialize, Serialize};

pub const MAX_RELAY_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DependencyRelayRequestV1 {
    pub request_id: String,
    pub worker_id: String,
    pub execution_id: String,
    pub task_id: String,
    pub workspace: crate::execution::contract::WorkspaceRef,
    pub authorization_generation: String,
    pub cancellation_generation: u64,
    pub need: crate::harness::DependencyNeed,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyRelayOutcomeV1 {
    Prepared,
    AwaitingManager,
    Denied,
    Failed,
}

/// A relay response contains the broker decision and request identity only.
/// Approved cache bytes require a separate digest-bound workspace transfer.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DependencyRelayResponseV1 {
    pub request_id: String,
    pub worker_id: String,
    pub execution_id: String,
    pub task_id: String,
    pub workspace: crate::execution::contract::WorkspaceRef,
    pub authorization_generation: String,
    pub cancellation_generation: u64,
    pub outcome: DependencyRelayOutcomeV1,
    /// Broker-owned identity used to retrieve the prepared cache snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_request_id: Option<String>,
}

impl DependencyRelayRequestV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        crate::execution::contract::validate_identifier(&self.request_id)?;
        crate::execution::contract::validate_identifier(&self.worker_id)?;
        crate::execution::contract::validate_identifier(&self.execution_id)?;
        crate::execution::contract::validate_identifier(&self.task_id)?;
        self.workspace.validate()?;
        anyhow::ensure!(
            self.workspace.task_id == self.task_id,
            "dependency relay workspace task does not match"
        );
        crate::execution::contract::validate_identifier(&self.authorization_generation)?;
        anyhow::ensure!(
            self.cancellation_generation > 0,
            "dependency relay cancellation generation is invalid"
        );
        anyhow::ensure!(
            serde_json::to_vec(self)?.len() <= MAX_RELAY_BYTES,
            "dependency relay request exceeds the bounded payload size"
        );
        anyhow::ensure!(
            self.need.command.len() <= 2048
                && self.need.reason.len() <= 1024
                && self.need.introduced_packages.len() <= 128,
            "dependency relay request exceeds field limits"
        );
        if let Some(source) = self.need.source.as_deref() {
            let url = url::Url::parse(source)?;
            anyhow::ensure!(
                url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "dependency relay source must not contain credential-bearing URL components"
            );
        }
        Ok(())
    }

    pub fn matches_response(&self, response: &DependencyRelayResponseV1) -> bool {
        response.request_id == self.request_id
            && response.worker_id == self.worker_id
            && response.execution_id == self.execution_id
            && response.task_id == self.task_id
            && response.workspace == self.workspace
            && response.authorization_generation == self.authorization_generation
            && response.cancellation_generation == self.cancellation_generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{
        DependencyKind, DependencyNeed, DependencyPackageIdentity, PackageEcosystem,
    };

    fn request() -> DependencyRelayRequestV1 {
        let id = "00000000-0000-4000-8000-000000000001".to_owned();
        DependencyRelayRequestV1 {
            request_id: id.clone(),
            worker_id: id.clone(),
            execution_id: id.clone(),
            task_id: id.clone(),
            workspace: crate::execution::contract::WorkspaceRef {
                project_id: id.clone(),
                task_id: id.clone(),
                source_sha: "a".repeat(40),
            },
            authorization_generation: id,
            cancellation_generation: 1,
            need: DependencyNeed {
                ecosystem: PackageEcosystem::Npm,
                package: Some("left-pad".into()),
                version: Some("1.0.0".into()),
                source: Some("https://registry.npmjs.org".into()),
                command: "npm install left-pad@1.0.0".into(),
                reason: "Required by the task".into(),
                kind: DependencyKind::NewProjectDependency,
                lockfile_identity: None,
                introduced_packages: Vec::new(),
            },
        }
    }

    #[test]
    fn relay_is_bounded_and_rejects_credential_bearing_source_urls() {
        let mut invalid = request();
        assert!(invalid.validate().is_ok());
        invalid.cancellation_generation = 0;
        assert!(invalid.validate().is_err());
        invalid.cancellation_generation = 1;
        invalid.need.source = Some("https://user:secret@example.invalid/pkg?token=secret".into());
        assert!(invalid.validate().is_err());

        let mut command_boundary = request();
        command_boundary.need.command = "x".repeat(2048);
        assert!(command_boundary.validate().is_ok());
        command_boundary.need.command.push('x');
        assert!(command_boundary.validate().is_err());

        let mut reason_boundary = request();
        reason_boundary.need.reason = "x".repeat(1024);
        assert!(reason_boundary.validate().is_ok());
        reason_boundary.need.reason.push('x');
        assert!(reason_boundary.validate().is_err());

        let package = DependencyPackageIdentity {
            package: "pkg".into(),
            version: "1".into(),
            source: "registry".into(),
            integrity: "hash".into(),
        };
        let mut package_boundary = request();
        package_boundary.need.introduced_packages = vec![package.clone(); 128];
        assert!(package_boundary.validate().is_ok());
        package_boundary.need.introduced_packages.push(package);
        assert!(package_boundary.validate().is_err());

        let mut oversized = request();
        oversized.need.source = Some(format!(
            "https://registry.npmjs.org/{}",
            "x".repeat(MAX_RELAY_BYTES)
        ));
        assert!(oversized.validate().is_err());
    }

    #[test]
    fn response_must_match_all_controller_owned_identities() {
        let request = request();
        let response = DependencyRelayResponseV1 {
            request_id: request.request_id.clone(),
            worker_id: request.worker_id.clone(),
            execution_id: request.execution_id.clone(),
            task_id: request.task_id.clone(),
            workspace: request.workspace.clone(),
            authorization_generation: request.authorization_generation.clone(),
            cancellation_generation: request.cancellation_generation,
            outcome: DependencyRelayOutcomeV1::Prepared,
            dependency_request_id: Some("00000000-0000-4000-8000-000000000002".into()),
        };
        assert!(request.matches_response(&response));
        let mut stale = response.clone();
        stale.authorization_generation = "00000000-0000-4000-8000-000000000002".into();
        assert!(!request.matches_response(&stale));

        let mut stale_workspace = response.clone();
        stale_workspace.workspace.source_sha = "b".repeat(40);
        assert!(!request.matches_response(&stale_workspace));

        let mut stale_cancellation = response;
        stale_cancellation.cancellation_generation += 1;
        assert!(!request.matches_response(&stale_cancellation));
    }

    #[test]
    fn two_workers_cannot_accept_each_others_dependency_decisions() {
        let first = request();
        let mut second = request();
        second.request_id = "00000000-0000-4000-8000-000000000002".into();
        second.worker_id = "00000000-0000-4000-8000-000000000003".into();
        second.execution_id = "00000000-0000-4000-8000-000000000004".into();
        second.workspace.source_sha = "b".repeat(40);

        let decision_for_first = DependencyRelayResponseV1 {
            request_id: first.request_id.clone(),
            worker_id: first.worker_id.clone(),
            execution_id: first.execution_id.clone(),
            task_id: first.task_id.clone(),
            workspace: first.workspace.clone(),
            authorization_generation: first.authorization_generation.clone(),
            cancellation_generation: first.cancellation_generation,
            outcome: DependencyRelayOutcomeV1::Prepared,
            dependency_request_id: Some("00000000-0000-4000-8000-000000000005".into()),
        };

        assert!(first.matches_response(&decision_for_first));
        assert!(!second.matches_response(&decision_for_first));
    }
}
