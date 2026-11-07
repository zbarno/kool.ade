use crate::computer_use::ComputerSessionV1;
use std::io::{Read, Write};

use super::{
    AuthenticatedGuestEnvelopeV1, GuestBootConfigV1, GuestChannelKeyV1, decode_hex,
    derive_channel_key, encode_hex, transcript_digest,
};
use crate::computer_use::guest_protocol::{
    GuestCapabilitySetV1, GuestMessageV1, GuestProtocolErrorV1, GuestSenderV1,
    frame::{read_frame, write_frame},
};

#[path = "handshake/context.rs"]
mod context;
use context::{
    envelope, random_nonce, validate_config_for_session, validate_context, validate_hello,
};

pub(crate) struct GuestHandshakeCompleteV1 {
    pub channel_key: GuestChannelKeyV1,
    pub capabilities: GuestCapabilitySetV1,
    pub transcript_sha256: String,
    pub boot_id: String,
    pub request_id: String,
    pub action_id: String,
    pub host_sequence: u64,
    pub guest_sequence: u64,
}

pub(crate) fn host_handshake(
    stream: &mut (impl Read + Write),
    config: &GuestBootConfigV1,
    session: &ComputerSessionV1,
    now_unix_ms: u64,
) -> Result<GuestHandshakeCompleteV1, GuestProtocolErrorV1> {
    config
        .validate()
        .map_err(|_| GuestProtocolErrorV1::Malformed)?;
    validate_config_for_session(config, session, now_unix_ms)?;
    let boot_secret = config
        .secret()
        .map_err(|_| GuestProtocolErrorV1::AuthenticationFailed)?;
    let hello: AuthenticatedGuestEnvelopeV1 = read_frame(stream)?;
    hello.verify(&boot_secret)?;
    let (request_id, action_id, guest_nonce, guest_capabilities) = validate_hello(&hello, config)?;

    let host_nonce = random_nonce();
    let capabilities_json =
        serde_json::to_vec(&guest_capabilities).map_err(|_| GuestProtocolErrorV1::Malformed)?;
    let transcript = transcript_digest(
        config,
        &request_id,
        &action_id,
        &guest_nonce,
        &host_nonce,
        &capabilities_json,
    );
    let deadline = now_unix_ms
        .checked_add(30_000)
        .filter(|deadline| *deadline < session.lease.expires_at_unix_ms)
        .unwrap_or(session.lease.expires_at_unix_ms);
    if deadline <= now_unix_ms {
        return Err(GuestProtocolErrorV1::StaleLease);
    }
    let challenge = envelope(
        GuestSenderV1::Host,
        config,
        1,
        &request_id,
        &action_id,
        deadline,
        GuestMessageV1::HostChallenge {
            host_nonce_hex: encode_hex(&host_nonce),
            transcript_sha256: transcript.clone(),
        },
    );
    write_frame(
        stream,
        &AuthenticatedGuestEnvelopeV1::sign(challenge, &boot_secret)?,
    )?;

    let proof: AuthenticatedGuestEnvelopeV1 = read_frame(stream)?;
    proof.verify(&boot_secret)?;
    validate_context(
        &proof.envelope,
        GuestSenderV1::Guest,
        config,
        2,
        &request_id,
        &action_id,
    )?;
    if proof.envelope.capability_id
        != crate::computer_use::guest_protocol::types::GUEST_CHANNEL_CAPABILITY_V1
        || proof.envelope.deadline_unix_ms != deadline
        || !matches!(&proof.envelope.message, GuestMessageV1::ClientProof { transcript_sha256 } if transcript_sha256 == &transcript)
    {
        return Err(GuestProtocolErrorV1::AuthenticationFailed);
    }

    let channel_key = derive_channel_key(&boot_secret, &guest_nonce, &host_nonce, &transcript)
        .map_err(|_| GuestProtocolErrorV1::AuthenticationFailed)?;
    let ready = envelope(
        GuestSenderV1::Host,
        config,
        2,
        &request_id,
        &action_id,
        deadline,
        GuestMessageV1::HostReady {
            transcript_sha256: transcript.clone(),
            capabilities: guest_capabilities.clone(),
        },
    );
    write_frame(
        stream,
        &AuthenticatedGuestEnvelopeV1::sign(ready, channel_key.expose())?,
    )?;
    Ok(GuestHandshakeCompleteV1 {
        channel_key,
        capabilities: guest_capabilities,
        transcript_sha256: transcript,
        boot_id: config.boot_id.clone(),
        request_id,
        action_id,
        host_sequence: 3,
        guest_sequence: 3,
    })
}

pub(crate) fn guest_handshake(
    stream: &mut (impl Read + Write),
    config: &GuestBootConfigV1,
    capabilities: GuestCapabilitySetV1,
) -> Result<GuestHandshakeCompleteV1, GuestProtocolErrorV1> {
    config
        .validate()
        .map_err(|_| GuestProtocolErrorV1::Malformed)?;
    capabilities.validate()?;
    if capabilities
        .app_registry
        .iter()
        .any(|app| !config.allowed_apps.contains(app))
    {
        return Err(GuestProtocolErrorV1::UnsupportedCapability);
    }
    let boot_secret = config
        .secret()
        .map_err(|_| GuestProtocolErrorV1::AuthenticationFailed)?;
    let guest_nonce = random_nonce();
    let request_id = uuid::Uuid::new_v4().to_string();
    let action_id = uuid::Uuid::new_v4().to_string();
    let hello = envelope(
        GuestSenderV1::Guest,
        config,
        1,
        &request_id,
        &action_id,
        0,
        GuestMessageV1::ClientHello {
            boot_id: config.boot_id.clone(),
            guest_nonce_hex: encode_hex(&guest_nonce),
            capabilities: capabilities.clone(),
        },
    );
    write_frame(
        stream,
        &AuthenticatedGuestEnvelopeV1::sign(hello, &boot_secret)?,
    )?;
    let challenge: AuthenticatedGuestEnvelopeV1 = read_frame(stream)?;
    challenge.verify(&boot_secret)?;
    validate_context(
        &challenge.envelope,
        GuestSenderV1::Host,
        config,
        1,
        &request_id,
        &action_id,
    )?;
    let host_nonce = match &challenge.envelope.message {
        GuestMessageV1::HostChallenge {
            host_nonce_hex,
            transcript_sha256,
        } => {
            let nonce = decode_hex(host_nonce_hex)
                .map_err(|_| GuestProtocolErrorV1::AuthenticationFailed)?;
            let nonce: [u8; 32] = nonce
                .try_into()
                .map_err(|_| GuestProtocolErrorV1::AuthenticationFailed)?;
            let capabilities_json =
                serde_json::to_vec(&capabilities).map_err(|_| GuestProtocolErrorV1::Malformed)?;
            let expected = transcript_digest(
                config,
                &request_id,
                &action_id,
                &guest_nonce,
                &nonce,
                &capabilities_json,
            );
            if transcript_sha256 != &expected || challenge.envelope.deadline_unix_ms == 0 {
                return Err(GuestProtocolErrorV1::AuthenticationFailed);
            }
            (nonce, expected, challenge.envelope.deadline_unix_ms)
        }
        _ => return Err(GuestProtocolErrorV1::Malformed),
    };
    let (host_nonce, transcript, deadline) = host_nonce;
    let proof = envelope(
        GuestSenderV1::Guest,
        config,
        2,
        &request_id,
        &action_id,
        deadline,
        GuestMessageV1::ClientProof {
            transcript_sha256: transcript.clone(),
        },
    );
    write_frame(
        stream,
        &AuthenticatedGuestEnvelopeV1::sign(proof, &boot_secret)?,
    )?;
    let channel_key = derive_channel_key(&boot_secret, &guest_nonce, &host_nonce, &transcript)
        .map_err(|_| GuestProtocolErrorV1::AuthenticationFailed)?;
    let ready: AuthenticatedGuestEnvelopeV1 = read_frame(stream)?;
    ready.verify(channel_key.expose())?;
    validate_context(
        &ready.envelope,
        GuestSenderV1::Host,
        config,
        2,
        &request_id,
        &action_id,
    )?;
    if ready.envelope.deadline_unix_ms != deadline
        || !matches!(&ready.envelope.message, GuestMessageV1::HostReady { transcript_sha256, capabilities: accepted } if transcript_sha256 == &transcript && accepted == &capabilities)
    {
        return Err(GuestProtocolErrorV1::AuthenticationFailed);
    }
    Ok(GuestHandshakeCompleteV1 {
        channel_key,
        capabilities,
        transcript_sha256: transcript,
        boot_id: config.boot_id.clone(),
        request_id,
        action_id,
        host_sequence: 3,
        guest_sequence: 3,
    })
}
