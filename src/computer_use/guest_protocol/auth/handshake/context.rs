use super::super::AuthenticatedGuestEnvelopeV1;
use crate::computer_use::{
    ComputerSessionV1,
    guest_protocol::{
        GuestBootConfigV1, GuestEnvelopeV1, GuestMessageV1, GuestProtocolErrorV1, GuestSenderV1,
        types::GUEST_CHANNEL_CAPABILITY_V1,
    },
};

pub(super) fn validate_config_for_session(
    config: &GuestBootConfigV1,
    session: &ComputerSessionV1,
    now_unix_ms: u64,
) -> Result<(), GuestProtocolErrorV1> {
    session
        .lease
        .validate(now_unix_ms)
        .map_err(|_| GuestProtocolErrorV1::StaleLease)?;
    let worker_id = session.target.target_machine_id();
    let mut allowed_apps = session.allowed_apps.iter().cloned().collect::<Vec<_>>();
    allowed_apps.sort();
    if config.session_id != session.identity.session_id
        || config.worker_id != *worker_id
        || config.lease_generation != session.lease.generation
        || config.expires_at_unix_ms != session.lease.expires_at_unix_ms
        || config.initial_action_sequence > session.action_sequence
        || config.allowed_apps != allowed_apps
    {
        return Err(GuestProtocolErrorV1::WrongSession);
    }
    Ok(())
}

pub(super) fn validate_hello(
    hello: &AuthenticatedGuestEnvelopeV1,
    config: &GuestBootConfigV1,
) -> Result<
    (
        String,
        String,
        [u8; 32],
        crate::computer_use::guest_protocol::GuestCapabilitySetV1,
    ),
    GuestProtocolErrorV1,
> {
    let envelope = &hello.envelope;
    if envelope.sender != GuestSenderV1::Guest
        || envelope.sequence != 1
        || envelope.deadline_unix_ms != 0
        || envelope.session_id != config.session_id
        || envelope.worker_id != config.worker_id
        || envelope.lease_generation != config.lease_generation
        || envelope.capability_id != GUEST_CHANNEL_CAPABILITY_V1
    {
        return Err(GuestProtocolErrorV1::WrongSession);
    }
    let GuestMessageV1::ClientHello {
        boot_id,
        guest_nonce_hex,
        capabilities,
    } = &envelope.message
    else {
        return Err(GuestProtocolErrorV1::Malformed);
    };
    if boot_id != &config.boot_id {
        return Err(GuestProtocolErrorV1::AuthenticationFailed);
    }
    let nonce =
        super::super::decode_hex(guest_nonce_hex).map_err(|_| GuestProtocolErrorV1::Malformed)?;
    let nonce: [u8; 32] = nonce
        .try_into()
        .map_err(|_| GuestProtocolErrorV1::Malformed)?;
    capabilities.validate()?;
    if capabilities
        .app_registry
        .iter()
        .any(|app| !config.allowed_apps.contains(app))
    {
        return Err(GuestProtocolErrorV1::UnsupportedCapability);
    }
    Ok((
        envelope.request_id.clone(),
        envelope.action_id.clone(),
        nonce,
        capabilities.clone(),
    ))
}

pub(super) fn validate_context(
    envelope: &GuestEnvelopeV1,
    sender: GuestSenderV1,
    config: &GuestBootConfigV1,
    sequence: u64,
    request_id: &str,
    action_id: &str,
) -> Result<(), GuestProtocolErrorV1> {
    if envelope.sender != sender
        || envelope.sequence != sequence
        || envelope.session_id != config.session_id
        || envelope.worker_id != config.worker_id
        || envelope.lease_generation != config.lease_generation
        || envelope.request_id != request_id
        || envelope.action_id != action_id
        || envelope.capability_id != GUEST_CHANNEL_CAPABILITY_V1
    {
        return Err(GuestProtocolErrorV1::WrongSession);
    }
    Ok(())
}

pub(super) fn envelope(
    sender: GuestSenderV1,
    config: &GuestBootConfigV1,
    sequence: u64,
    request_id: &str,
    action_id: &str,
    deadline_unix_ms: u64,
    message: GuestMessageV1,
) -> GuestEnvelopeV1 {
    GuestEnvelopeV1 {
        protocol_version: crate::computer_use::guest_protocol::types::GUEST_PROTOCOL_VERSION_V1,
        sender,
        session_id: config.session_id.clone(),
        worker_id: config.worker_id.clone(),
        lease_generation: config.lease_generation,
        sequence,
        request_id: request_id.to_owned(),
        action_id: action_id.to_owned(),
        deadline_unix_ms,
        capability_id: GUEST_CHANNEL_CAPABILITY_V1.to_owned(),
        message,
    }
}

pub(super) fn random_nonce() -> [u8; 32] {
    let mut bytes = [0; 32];
    bytes[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    bytes
}
