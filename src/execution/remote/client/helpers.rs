use crate::execution::{
    backend::BackendProbeV1,
    errors::{ExecutionError, ExecutionErrorKind},
};

use super::super::{
    protocol::{EnvelopeKindV1 as Kind, PROTOCOL_VERSION_V1},
    ssh::{SshWorkerConfig, VerifiedSshWorker},
};

pub(super) fn verified(config: &SshWorkerConfig) -> Result<VerifiedSshWorker, ExecutionError> {
    config.verify_known_host().map_err(|_| incompatible())
}

pub(super) fn negotiated_probe(
    payload: serde_json::Value,
    expected_principal: &str,
) -> Result<BackendProbeV1, ExecutionError> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct HealthAck {
        wire_version: u16,
        authenticated_principal: String,
        probe: BackendProbeV1,
    }
    let ack: HealthAck = serde_json::from_value(payload)
        .map_err(|error| incompatible_with(format!("invalid worker health response: {error}")))?;
    if ack.wire_version != PROTOCOL_VERSION_V1 || ack.probe.wire_version != PROTOCOL_VERSION_V1 {
        return Err(incompatible_with(format!(
            "worker wire version mismatch: response={}, probe={}, expected={PROTOCOL_VERSION_V1}",
            ack.wire_version, ack.probe.wire_version
        )));
    }
    if ack.authenticated_principal != expected_principal {
        return Err(incompatible_with(format!(
            "worker SSH account mismatch: worker={}, expected={expected_principal}",
            ack.authenticated_principal
        )));
    }
    if !ack.probe.available || !ack.probe.compatible {
        return Err(incompatible_with(format!(
            "worker reports unavailable or incompatible execution: available={}, compatible={}, failure={:?}",
            ack.probe.available, ack.probe.compatible, ack.probe.failure
        )));
    }
    let capabilities =
        ack.probe.capabilities.as_ref().ok_or_else(|| {
            incompatible_with("worker health response omitted capabilities".into())
        })?;
    let expected_policy_hash = crate::execution::policy::policy_hash();
    if capabilities.app_version != env!("CARGO_PKG_VERSION") {
        return Err(incompatible_with(format!(
            "worker application version mismatch: worker={}, controller={}",
            capabilities.app_version,
            env!("CARGO_PKG_VERSION")
        )));
    }
    if capabilities.policy_hash != expected_policy_hash {
        return Err(incompatible_with(format!(
            "worker execution policy fingerprint mismatch: worker={}, controller={expected_policy_hash}",
            capabilities.policy_hash
        )));
    }
    Ok(ack.probe)
}

pub(in crate::execution::remote) fn matches_response(request: &Kind, response: &Kind) -> bool {
    matches!(
        (request, response),
        (Kind::Probe, Kind::ProbeResult)
            | (Kind::Subscribe, Kind::Event)
            | (Kind::RelayDependency, Kind::DependencyRequest)
            | (
                Kind::Health
                    | Kind::Heartbeat
                    | Kind::PrepareWorkspace
                    | Kind::BeginWorkspaceTransfer
                    | Kind::WorkspaceTransferChunk
                    | Kind::FinalizeWorkspaceTransfer
                    | Kind::BeginCacheTransfer
                    | Kind::CacheTransferChunk
                    | Kind::FinalizeCacheTransfer
                    | Kind::InspectSystemPackages
                    | Kind::BeginSystemImageTransfer
                    | Kind::SystemImageTransferChunk
                    | Kind::FinalizeSystemImageTransfer
                    | Kind::DependencyDecision
                    | Kind::AuthorizePrivateFileGrant
                    | Kind::PrivateFileChunk
                    | Kind::PrepareWorkspaceResult
                    | Kind::ReadWorkspaceResultChunk
                    | Kind::Start
                    | Kind::Exec
                    | Kind::Cancel
                    | Kind::CollectArtifacts
                    | Kind::ReleaseWorkspace
                    | Kind::Release,
                Kind::Acknowledge
            )
    )
}

pub(super) fn unavailable_probe() -> BackendProbeV1 {
    BackendProbeV1 {
        wire_version: PROTOCOL_VERSION_V1,
        available: false,
        compatible: false,
        architecture: None,
        failure: Some(ExecutionErrorKind::EnvironmentUnavailable),
        capabilities: None,
    }
}

pub(super) fn unavailable() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::EnvironmentUnavailable,
        "Remote worker request state is unavailable",
        false,
    )
}

pub(super) fn disconnected() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerDisconnected,
        "Remote worker connection is unavailable",
        true,
    )
}

pub(super) fn incompatible() -> ExecutionError {
    ExecutionError::new(
        ExecutionErrorKind::WorkerIncompatible,
        "Remote worker identity or protocol is incompatible",
        false,
    )
}

fn incompatible_with(message: String) -> ExecutionError {
    ExecutionError::new(ExecutionErrorKind::WorkerIncompatible, message, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiated_health_rejects_incompatible_wire_versions() {
        for (response_version, probe_version) in [
            (PROTOCOL_VERSION_V1 + 1, PROTOCOL_VERSION_V1),
            (PROTOCOL_VERSION_V1, PROTOCOL_VERSION_V1 + 1),
        ] {
            let error = negotiated_probe(
                serde_json::json!({
                    "wire_version": response_version,
                    "authenticated_principal": "builder",
                    "probe": {
                        "wire_version": probe_version,
                        "available": false,
                        "compatible": false,
                        "failure": null
                    }
                }),
                "builder",
            )
            .unwrap_err();

            assert_eq!(error.kind, ExecutionErrorKind::WorkerIncompatible);
            assert!(error.message.contains(&format!(
                "worker wire version mismatch: response={response_version}, probe={probe_version}"
            )));
        }
    }

    #[test]
    fn negotiated_health_rejects_a_different_authenticated_ssh_account() {
        let error = negotiated_probe(
            serde_json::json!({
                "wire_version": PROTOCOL_VERSION_V1,
                "authenticated_principal": "unexpected-account",
                "probe": {
                    "wire_version": PROTOCOL_VERSION_V1,
                    "available": false,
                    "compatible": false,
                    "failure": null
                }
            }),
            "builder",
        )
        .unwrap_err();

        assert_eq!(error.kind, ExecutionErrorKind::WorkerIncompatible);
        assert!(error.message.contains("worker SSH account mismatch"));
    }

    #[test]
    fn request_and_response_kinds_must_match() {
        assert!(matches_response(&Kind::Probe, &Kind::ProbeResult));
        assert!(matches_response(&Kind::Subscribe, &Kind::Event));
        assert!(matches_response(&Kind::Start, &Kind::Acknowledge));
        assert!(!matches_response(&Kind::Probe, &Kind::Acknowledge));
        assert!(!matches_response(&Kind::Heartbeat, &Kind::Event));
    }
}
