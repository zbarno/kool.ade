use crate::computer_use::ComputerSessionClassV1;

use super::super::super::catalog::{valid_id, valid_sha256};
use super::super::contracts::{
    GuestAppConsentGrantV1 as Approval, GuestAppOperationRequestV1 as Request,
};

pub(super) fn validate_request(
    request: &Request,
    approval: &Approval,
    expected_catalog: &str,
) -> Result<(), &'static str> {
    request.identity.validate()?;
    if request.schema_version != 1
        || !canonical_uuid(&request.operation_id)
        || !valid_id(&request.app_id)
        || request.catalog_digest != expected_catalog
        || !valid_sha256(&request.image_digest)
        || request.lease_generation == 0
        || request.approval_generation == 0
        || request.now_unix_ms == 0
        || approval.identity != request.identity
        || approval.session_class != request.session_class
        || approval.app_id != request.app_id
        || approval.operation != request.operation
        || approval.catalog_digest != request.catalog_digest
        || approval.image_digest != request.image_digest
        || approval.architecture != request.architecture
        || approval.lease_generation != request.lease_generation
        || approval.approval_generation != request.approval_generation
        || approval.granted_capabilities != request.granted_capabilities
        || approval.network_granted != request.network_granted
        || approval.expires_at_unix_ms <= request.now_unix_ms
        || approval.expires_at_unix_ms - request.now_unix_ms > 15 * 60 * 1000
    {
        return Err("guest app operation authorization is invalid or expired");
    }
    if request.session_class != ComputerSessionClassV1::Disposable {
        return Err("persistent app operations require encrypted personal-session support");
    }
    Ok(())
}

pub(super) fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
