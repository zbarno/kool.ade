use serde::{Deserialize, Serialize};

use crate::computer_use::{ComputerIdentityV1, ComputerSessionClassV1};

use super::super::{GuestAppArchitectureV1, GuestAppCapabilityV1};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestAppOperationKindV1 {
    Install,
    Update,
    Remove,
    Launch,
}

/// Controller-created authorization for one exact app operation. Provider
/// arguments cannot mint or expand this record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestAppOperationRequestV1 {
    pub schema_version: u16,
    pub operation_id: String,
    pub identity: ComputerIdentityV1,
    pub session_class: ComputerSessionClassV1,
    pub app_id: String,
    pub operation: GuestAppOperationKindV1,
    pub catalog_digest: String,
    pub image_digest: String,
    pub architecture: GuestAppArchitectureV1,
    pub lease_generation: u64,
    pub approval_generation: u64,
    pub now_unix_ms: u64,
    pub granted_capabilities: std::collections::BTreeSet<GuestAppCapabilityV1>,
    pub network_granted: bool,
}

/// Only trusted controller code can mint this token after showing and
/// accepting the exact app effects in the approval UI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuestAppConsentGrantV1 {
    pub(super) identity: ComputerIdentityV1,
    pub(super) session_class: ComputerSessionClassV1,
    pub(super) app_id: String,
    pub(super) operation: GuestAppOperationKindV1,
    pub(super) catalog_digest: String,
    pub(super) image_digest: String,
    pub(super) architecture: GuestAppArchitectureV1,
    pub(super) lease_generation: u64,
    pub(super) approval_generation: u64,
    pub(super) expires_at_unix_ms: u64,
    pub(super) granted_capabilities: std::collections::BTreeSet<GuestAppCapabilityV1>,
    pub(super) network_granted: bool,
}

impl GuestAppConsentGrantV1 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn approved_by_controller(
        identity: ComputerIdentityV1,
        session_class: ComputerSessionClassV1,
        app_id: String,
        operation: GuestAppOperationKindV1,
        catalog_digest: String,
        image_digest: String,
        architecture: GuestAppArchitectureV1,
        lease_generation: u64,
        approval_generation: u64,
        expires_at_unix_ms: u64,
        granted_capabilities: std::collections::BTreeSet<GuestAppCapabilityV1>,
        network_granted: bool,
    ) -> Self {
        Self {
            identity,
            session_class,
            app_id,
            operation,
            catalog_digest,
            image_digest,
            architecture,
            lease_generation,
            approval_generation,
            expires_at_unix_ms,
            granted_capabilities,
            network_granted,
        }
    }
}
