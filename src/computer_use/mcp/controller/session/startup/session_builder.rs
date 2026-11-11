use crate::computer_use::mcp::ComputerUseCallerV1;
use crate::computer_use::mcp::controller::types::Runtime;
use crate::computer_use::{
    ComputerControllerProofV1, ComputerLeaseV1, ComputerLifecycleV1, ComputerNetworkProfileV1,
    ComputerPolicyV1, ComputerSessionClassV1, ComputerSessionV1, ComputerTargetV1,
    guest_image::CachedGuestImageV1,
};

pub(super) struct SessionBuild<'a> {
    pub runtime: &'a Runtime,
    pub caller: &'a ComputerUseCallerV1,
    pub policy: ComputerPolicyV1,
    pub network_profile: ComputerNetworkProfileV1,
    pub lease_id: String,
    pub expires_at_unix_ms: u64,
    pub now_unix_ms: u64,
    pub image: &'a CachedGuestImageV1,
    pub session_class: ComputerSessionClassV1,
    pub target: ComputerTargetV1,
}

pub(super) fn build(input: SessionBuild<'_>) -> ComputerSessionV1 {
    let SessionBuild {
        runtime,
        caller,
        policy,
        network_profile,
        lease_id,
        expires_at_unix_ms,
        now_unix_ms,
        image,
        session_class,
        target,
    } = input;
    ComputerSessionV1 {
        schema_version: 1,
        session_started_at_unix_ms: now_unix_ms,
        identity: caller.identity.clone(),
        session_class,
        target,
        image_digest: image.entry().image_digest.clone(),
        image_manifest_digest: image.manifest_digest().to_owned(),
        image_signer_key_id: image.manifest_signer_key_id().to_owned(),
        network_profile,
        allowed_apps: policy.allowed_apps.clone(),
        policy,
        lease: ComputerLeaseV1 {
            schema_version: 1,
            owner_id: caller.identity.user_id.clone(),
            lease_id,
            generation: 1,
            issued_at_unix_ms: now_unix_ms,
            expires_at_unix_ms,
            cancellation_generation: 1,
        },
        lifecycle: ComputerLifecycleV1::Delegated,
        action_sequence: 0,
        approval_generation: 1,
        cancellation_generation: 1,
        evidence_refs: Vec::new(),
        controller_proof: ComputerControllerProofV1 {
            signer_key_id: runtime.controller_key_id.clone(),
            signature_hex: "0".repeat(128),
        },
    }
}
