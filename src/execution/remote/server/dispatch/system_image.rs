use super::{Arc, EnvelopeV1, ExecutionBackend, Kind, Value, json};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Default)]
pub(in crate::execution::remote::server) struct SystemImageTransfers(
    HashMap<String, Pending>,
    std::collections::HashSet<String>,
);

struct Pending {
    capability: String,
    proposal: crate::execution::docker::image_extension::SystemImageExtensionRequest,
    decision: crate::harness::DependencyDecision,
    workspace: crate::execution::contract::WorkspaceRef,
    identities: Vec<crate::execution::docker::image_extension::builder::PackageArtifactIdentity>,
    bytes: Vec<Vec<u8>>,
    total_bytes: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectRequest {
    base: crate::execution::docker::image_extension::BaseImageSelection,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeginRequest {
    transfer_id: String,
    capability: String,
    proposal: crate::execution::docker::image_extension::SystemImageExtensionRequest,
    decision: crate::harness::DependencyDecision,
    workspace: crate::execution::contract::WorkspaceRef,
    manifest: Vec<crate::execution::docker::image_extension::builder::PackageArtifactIdentity>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChunkRequest {
    transfer_id: String,
    capability: String,
    package_index: usize,
    offset: usize,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FinalizeRequest {
    transfer_id: String,
    capability: String,
}

pub(super) fn dispatch(
    backend: Arc<dyn ExecutionBackend>,
    stager: Option<&Arc<crate::execution::remote::staging::WorkerStager>>,
    transfers: &mut SystemImageTransfers,
    runtime: &tokio::runtime::Runtime,
    capability_key: &mut Option<zeroize::Zeroizing<[u8; 32]>>,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), crate::execution::errors::ExecutionError> {
    match envelope.kind {
        Kind::InspectSystemPackages => inspect(&backend, runtime, &envelope.payload, envelope),
        Kind::BeginSystemImageTransfer => begin(
            &envelope.payload,
            transfers,
            envelope,
            stager,
            capability_key,
        ),
        Kind::SystemImageTransferChunk => chunk(&envelope.payload, transfers, envelope),
        Kind::FinalizeSystemImageTransfer => {
            finalize(&envelope.payload, transfers, envelope, backend, runtime)
        }
        _ => unreachable!("only system image messages are dispatched here"),
    }
}

pub(super) fn inspect(
    backend: &Arc<dyn ExecutionBackend>,
    runtime: &tokio::runtime::Runtime,
    payload: &Value,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), crate::execution::errors::ExecutionError> {
    super::support::require_control_id(envelope)?;
    let request: InspectRequest = super::support::decode(payload)?;
    let packages = runtime.block_on(backend.inspect_system_package_state(&request.base))?;
    super::support::json_value(packages).map(|value| (Kind::Acknowledge, value))
}

pub(super) fn begin(
    payload: &Value,
    transfers: &mut SystemImageTransfers,
    envelope: &EnvelopeV1,
    stager: Option<&Arc<crate::execution::remote::staging::WorkerStager>>,
    capability_key: &Option<zeroize::Zeroizing<[u8; 32]>>,
) -> Result<(Kind, Value), crate::execution::errors::ExecutionError> {
    super::support::require_control_id(envelope)?;
    let request: BeginRequest = super::support::decode(payload)?;
    let transfer_id = uuid::Uuid::parse_str(&request.transfer_id)
        .map_err(|_| super::super::incompatible("image transfer ID is invalid"))?;
    if transfer_id.hyphenated().to_string() != request.transfer_id
        || transfers.0.contains_key(&request.transfer_id)
        || !transfers.0.is_empty()
    {
        return Err(super::super::incompatible(
            "image transfer is duplicated or capacity is full",
        ));
    }
    request
        .workspace
        .validate()
        .map_err(|_| super::super::incompatible("image workspace identity is invalid"))?;
    if request.workspace.task_id != request.proposal.task_id
        || request.workspace.project_id != request.proposal.project_id
        || request.decision != crate::harness::DependencyDecision::UserAuthorizeForTask
        || !stager.is_some_and(|worker| worker.has_workspace(&request.workspace))
    {
        return Err(super::super::incompatible(
            "image authorization is outside the staged task workspace",
        ));
    }
    let key = capability_key.as_ref().ok_or_else(|| {
        super::super::incompatible("worker session has no user authorization capability")
    })?;
    if !crate::execution::docker::image_extension::verify_system_image_capability(
        key,
        &request.capability,
        &request.transfer_id,
        &request.proposal,
        request.decision,
        &request.workspace,
        &request.manifest,
    ) || transfers.1.contains(&request.capability)
    {
        return Err(super::super::incompatible(
            "system image user authorization capability is invalid, mismatched, or replayed",
        ));
    }
    let base = request
        .proposal
        .resolve_base_selection(&request.workspace.task_id, &request.workspace.project_id)
        .map_err(|_| super::super::incompatible("image proposal failed locked-base validation"))?;
    if request.manifest.is_empty()
        || request.manifest.len() > 256
        || !request
            .manifest
            .windows(2)
            .all(|pair| pair[0].name < pair[1].name)
        || !request
            .proposal
            .packages
            .iter()
            .all(|name| request.manifest.iter().any(|item| &item.name == name))
    {
        return Err(super::super::incompatible(
            "image package manifest is not canonical",
        ));
    }
    for identity in &request.manifest {
        crate::execution::docker::image_extension::builder::validate_identity(
            identity,
            &base.platform,
        )
        .map_err(|_| super::super::incompatible("image package identity was rejected"))?;
    }
    let bytes = vec![Vec::new(); request.manifest.len()];
    transfers.0.insert(
        request.transfer_id,
        Pending {
            capability: request.capability.clone(),
            proposal: request.proposal,
            decision: request.decision,
            workspace: request.workspace,
            identities: request.manifest,
            bytes,
            total_bytes: 0,
        },
    );
    transfers.1.insert(request.capability);
    Ok((Kind::Acknowledge, json!({"accepted":true})))
}

pub(super) fn chunk(
    payload: &Value,
    transfers: &mut SystemImageTransfers,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), crate::execution::errors::ExecutionError> {
    super::support::require_control_id(envelope)?;
    let request: ChunkRequest = super::support::decode(payload)?;
    if request.bytes.is_empty()
        || request.bytes.len() > crate::execution::remote::staging::MAX_TRANSFER_CHUNK_BYTES
    {
        return Err(super::super::incompatible(
            "image package chunk size is invalid",
        ));
    }
    let pending = transfers
        .0
        .get_mut(&request.transfer_id)
        .ok_or_else(|| super::super::incompatible("image transfer is unavailable"))?;
    if !constant_time_eq(&pending.capability, &request.capability) {
        return Err(super::super::incompatible(
            "image package chunk capability does not match its authorized transfer",
        ));
    }
    let bytes = pending
        .bytes
        .get_mut(request.package_index)
        .ok_or_else(|| super::super::incompatible("image package index is invalid"))?;
    if request.offset != bytes.len()
        || pending.total_bytes.saturating_add(request.bytes.len()) > 512 * 1024 * 1024
    {
        return Err(super::super::incompatible(
            "image package chunk sequence or aggregate size is invalid",
        ));
    }
    bytes.extend_from_slice(&request.bytes);
    pending.total_bytes += request.bytes.len();
    Ok((Kind::Acknowledge, json!({"accepted":true})))
}

pub(super) fn finalize(
    payload: &Value,
    transfers: &mut SystemImageTransfers,
    envelope: &EnvelopeV1,
    backend: Arc<dyn ExecutionBackend>,
    runtime: &tokio::runtime::Runtime,
) -> Result<(Kind, Value), crate::execution::errors::ExecutionError> {
    super::support::require_control_id(envelope)?;
    let request: FinalizeRequest = super::support::decode(payload)?;
    let pending = transfers
        .0
        .get(&request.transfer_id)
        .ok_or_else(|| super::super::incompatible("image transfer is unavailable"))?;
    if !constant_time_eq(&pending.capability, &request.capability) {
        return Err(super::super::incompatible(
            "image finalize capability does not match its authorized transfer",
        ));
    }
    let pending = transfers
        .0
        .remove(&request.transfer_id)
        .expect("pending transfer exists");
    let authorized = pending
        .proposal
        .authorize(
            pending.decision,
            &pending.workspace.task_id,
            pending.identities.clone(),
        )
        .map_err(|_| super::super::incompatible("image user authorization failed validation"))?;
    let packages = pending
        .identities
        .into_iter()
        .zip(pending.bytes)
        .map(|(identity, bytes)| {
            crate::execution::docker::image_extension::builder::VerifiedPackageArtifact {
                identity,
                bytes,
            }
        })
        .collect();
    let derived =
        runtime.block_on(backend.derive_authorized_system_image(&authorized, packages))?;
    super::support::json_value(derived).map(|value| (Kind::Acknowledge, value))
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
}

#[cfg(test)]
#[path = "system_image_tests.rs"]
mod tests;
