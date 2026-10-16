use super::super::*;
use crate::execution::{
    backend::BackendFuture,
    docker::image_extension::{
        AuthorizedImageExtensionRequest, BaseImageSelection, DerivedImage, LinuxDistribution,
        PackageTrust, SystemImageExtensionRequest,
        builder::{PackageArtifactIdentity, VerifiedPackageArtifact},
    },
    remote::{protocol::EnvelopeKindV1 as Kind, staging::MAX_TRANSFER_CHUNK_BYTES},
};
use sha2::Digest;

pub(super) fn inspect<'a>(
    backend: &'a RemoteExecutionBackend,
    base: &'a BaseImageSelection,
) -> BackendFuture<
    'a,
    std::collections::BTreeMap<String, crate::harness::resource_bridge::InstalledPackage>,
> {
    Box::pin(async move {
        backend.request(
            Kind::InspectSystemPackages,
            CONTROL_EXECUTION_ID,
            serde_json::json!({"base": base}),
        )
    })
}

pub(super) fn derive<'a>(
    backend: &'a RemoteExecutionBackend,
    request: &'a AuthorizedImageExtensionRequest,
    packages: Vec<VerifiedPackageArtifact>,
) -> BackendFuture<'a, DerivedImage> {
    Box::pin(async move {
        if packages.len() != request.package_manifest.len()
            || packages.is_empty()
            || packages.len() > 256
            || !packages
                .iter()
                .map(|package| &package.identity)
                .eq(request.package_manifest.iter())
        {
            return Err(incompatible());
        }
        for package in &packages {
            crate::execution::docker::image_extension::builder::validate_identity(
                &package.identity,
                &request.target_platform,
            )
            .map_err(|_| incompatible())?;
            let digest = format!("{:x}", sha2::Sha256::digest(&package.bytes));
            if digest != package.identity.sha256 {
                return Err(incompatible());
            }
        }
        let workspace = backend
            .snapshots
            .lock()
            .map_err(|_| disconnected())?
            .keys()
            .find(|workspace| {
                workspace.task_id == request.task_id && workspace.project_id == request.project_id
            })
            .cloned()
            .ok_or_else(incompatible)?;
        let architecture = request
            .target_platform
            .strip_prefix("linux/")
            .ok_or_else(incompatible)?;
        let proposal = SystemImageExtensionRequest {
            task_id: request.task_id.clone(),
            project_id: request.project_id.clone(),
            distribution: LinuxDistribution::DebianBookworm,
            base_profile: request.base_profile,
            base_image_digest: request.base_digest.clone(),
            target_arch: architecture.to_owned(),
            packages: request.requested_packages.clone(),
            trust: PackageTrust::ApprovedDistributionRepository,
            purpose: request.purpose.clone(),
        };
        let transfer_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let manifest = packages
            .iter()
            .map(|package| package.identity.clone())
            .collect::<Vec<PackageArtifactIdentity>>();
        let mut client = backend.client.lock().map_err(|_| disconnected())?;
        let decision = crate::harness::DependencyDecision::UserAuthorizeForTask;
        let capability = client.system_image_capability(
            &transfer_id,
            &proposal,
            decision,
            &workspace,
            &manifest,
        )?;
        let _: crate::execution::remote::protocol::EnvelopeV1 = client.request(
            Kind::BeginSystemImageTransfer,
            CONTROL_EXECUTION_ID,
            serde_json::json!({
                "transfer_id": transfer_id,
                "capability": capability,
                "proposal": proposal,
                "decision": decision,
                "workspace": workspace,
                "manifest": manifest
            }),
        )?;
        for (package_index, package) in packages.iter().enumerate() {
            for (chunk_index, bytes) in package.bytes.chunks(MAX_TRANSFER_CHUNK_BYTES).enumerate() {
                let _: crate::execution::remote::protocol::EnvelopeV1 = client.request(
                    Kind::SystemImageTransferChunk,
                    CONTROL_EXECUTION_ID,
                    serde_json::json!({"transfer_id":transfer_id,"package_index":package_index,
                        "capability":capability,
                        "offset":chunk_index * MAX_TRANSFER_CHUNK_BYTES,"bytes":bytes}),
                )?;
            }
        }
        let envelope: crate::execution::remote::protocol::EnvelopeV1 = client.request(
            Kind::FinalizeSystemImageTransfer,
            CONTROL_EXECUTION_ID,
            serde_json::json!({"transfer_id":transfer_id,"capability":capability}),
        )?;
        serde_json::from_value(envelope.payload).map_err(|_| incompatible())
    })
}
