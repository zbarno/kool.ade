use sha2::Digest;

use crate::computer_use::{
    ComputerActionActorV1, ComputerActionPayloadV1, ComputerActionV1, ComputerOperationV1,
};

use super::super::VerifiedGuestApplicationCatalogV1;
use super::contracts::{GuestAppConsentGrantV1 as Approval, GuestAppOperationRequestV1 as Request};
mod validation;
use self::validation::validate_request;

pub struct GuestAppProvisionerV1 {
    catalog: VerifiedGuestApplicationCatalogV1,
}

impl GuestAppProvisionerV1 {
    pub fn new(catalog: VerifiedGuestApplicationCatalogV1) -> Self {
        Self { catalog }
    }

    /// Build an authenticated guest action for an approved catalog request.
    /// The operation ID is the protocol idempotency key; the existing session
    /// action journal owns replay and uncertain outcomes.
    pub fn computer_action(
        &self,
        request: &Request,
        approval: &Approval,
        action_sequence: u64,
        cancellation_generation: u64,
    ) -> Result<ComputerActionV1, &'static str> {
        validate_request(request, approval, self.catalog.digest())?;
        if action_sequence == 0 || cancellation_generation == 0 {
            return Err("guest app action generation is invalid");
        }
        let manifest = self
            .catalog
            .app(&request.app_id)
            .ok_or("app is not in the trusted catalog")?;
        if request.image_digest != self.catalog.catalog().image_digest
            || request.architecture != self.catalog.catalog().architecture
            || !manifest
                .supported_image_digests
                .contains(&request.image_digest)
            || !manifest
                .supported_architectures
                .contains(&request.architecture)
            || !manifest
                .required_capabilities
                .is_subset(&request.granted_capabilities)
            || (manifest
                .required_capabilities
                .contains(&super::super::GuestAppCapabilityV1::GuestNetwork)
                && !request.network_granted)
        {
            return Err("app exceeds the approved image or capability grant");
        }
        let manifest_bytes =
            serde_json::to_vec(manifest).map_err(|_| "guest app manifest could not be encoded")?;
        let manifest_sha256 = format!("sha256:{:x}", sha2::Sha256::digest(&manifest_bytes));
        Ok(ComputerActionV1 {
            schema_version: 1,
            session_id: request.identity.session_id.clone(),
            action_sequence,
            idempotency_key: request.operation_id.clone(),
            actor: ComputerActionActorV1::Human,
            lease_generation: request.lease_generation,
            approval_generation: request.approval_generation,
            cancellation_generation,
            operation: ComputerOperationV1::ManageApp,
            payload: ComputerActionPayloadV1::ManageApp {
                app_id: manifest.app_id.clone(),
                operation: request.operation,
                catalog_digest: request.catalog_digest.clone(),
                image_digest: request.image_digest.clone(),
                manifest_sha256,
                package_sha256: manifest.package.package_sha256.clone(),
            },
        })
    }
}
