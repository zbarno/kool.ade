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
) -> Result<BackendProbeV1, ExecutionError> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct HealthAck {
        wire_version: u16,
        probe: BackendProbeV1,
    }
    let ack: HealthAck = serde_json::from_value(payload).map_err(|_| incompatible())?;
    if ack.wire_version != PROTOCOL_VERSION_V1
        || ack.probe.wire_version != PROTOCOL_VERSION_V1
        || !ack.probe.available
        || !ack.probe.compatible
    {
        return Err(incompatible());
    }
    let capabilities = ack.probe.capabilities.as_ref().ok_or_else(incompatible)?;
    if capabilities.app_version != env!("CARGO_PKG_VERSION")
        || capabilities.policy_hash != crate::execution::policy::policy_hash()
    {
        return Err(incompatible());
    }
    Ok(ack.probe)
}

pub(super) fn matches_response(request: &Kind, response: &Kind) -> bool {
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
                    | Kind::DependencyDecision
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiated_health_rejects_incompatible_wire_versions() {
        let result = negotiated_probe(serde_json::json!({
            "wire_version": PROTOCOL_VERSION_V1 + 1,
            "probe": {}
        }));
        assert_eq!(
            result.unwrap_err().kind,
            ExecutionErrorKind::WorkerIncompatible
        );
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
