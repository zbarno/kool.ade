use crate::computer_use::{
    ComputerActionV1,
    guest_protocol::{GuestBootConfigV1, GuestProtocolErrorV1},
};
use sha2::{Digest, Sha256};

pub(super) fn request_id_for_action(
    config: &GuestBootConfigV1,
    action: &ComputerActionV1,
) -> String {
    let namespace = uuid::Uuid::parse_str(&config.session_id).expect("validated boot config UUID");
    uuid::Uuid::new_v5(
        &namespace,
        format!(
            "koolade.computer-use.request.v1:{}:{}:{}",
            config.lease_generation, action.action_sequence, action.idempotency_key
        )
        .as_bytes(),
    )
    .to_string()
}

pub(super) fn action_digest(action: &ComputerActionV1) -> Result<String, GuestProtocolErrorV1> {
    let bytes = serde_json::to_vec(action).map_err(|_| GuestProtocolErrorV1::Malformed)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
