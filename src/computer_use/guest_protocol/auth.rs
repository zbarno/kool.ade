use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(target_os = "linux")]
use std::{
    fs,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};
use zeroize::{Zeroize, Zeroizing};

use super::types::{GuestEnvelopeV1, GuestProtocolErrorV1, canonical_uuid};

type HmacSha256 = Hmac<Sha256>;
const BOOT_CONFIG_VERSION: u16 = 1;
const CHANNEL_DOMAIN: &[u8] = b"koolade.computer-use.guest-channel.v1\0";

mod handshake;
pub(crate) use handshake::{GuestHandshakeCompleteV1, guest_handshake, host_handshake};

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GuestBootConfigV1 {
    pub schema_version: u16,
    pub session_id: String,
    pub worker_id: String,
    pub lease_generation: u64,
    pub initial_action_sequence: u64,
    pub expires_at_unix_ms: u64,
    pub boot_id: String,
    pub secret_hex: String,
    pub allowed_apps: Vec<String>,
}

impl Drop for GuestBootConfigV1 {
    fn drop(&mut self) {
        self.secret_hex.zeroize();
    }
}

impl std::fmt::Debug for GuestBootConfigV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuestBootConfigV1")
            .field("schema_version", &self.schema_version)
            .field("session_id", &self.session_id)
            .field("worker_id", &self.worker_id)
            .field("lease_generation", &self.lease_generation)
            .field("initial_action_sequence", &self.initial_action_sequence)
            .field("expires_at_unix_ms", &self.expires_at_unix_ms)
            .field("boot_id", &self.boot_id)
            .field("secret_hex", &"[redacted]")
            .field("allowed_apps", &self.allowed_apps)
            .finish()
    }
}

impl GuestBootConfigV1 {
    pub(crate) fn issue(
        session_id: String,
        worker_id: String,
        lease_generation: u64,
        initial_action_sequence: u64,
        expires_at_unix_ms: u64,
        allowed_apps: Vec<String>,
    ) -> Result<Self, GuestHandshakeErrorV1> {
        let mut secret = [0; 32];
        secret[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        secret[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
        let config = Self {
            schema_version: BOOT_CONFIG_VERSION,
            session_id,
            worker_id,
            lease_generation,
            initial_action_sequence,
            expires_at_unix_ms,
            boot_id: uuid::Uuid::new_v4().to_string(),
            secret_hex: encode_hex(&secret),
            allowed_apps,
        };
        secret.zeroize();
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), GuestHandshakeErrorV1> {
        if self.schema_version != BOOT_CONFIG_VERSION
            || !canonical_uuid(&self.session_id)
            || !canonical_uuid(&self.worker_id)
            || self.lease_generation == 0
            || self.initial_action_sequence > 10_000
            || self.expires_at_unix_ms == 0
            || !canonical_uuid(&self.boot_id)
            || self.allowed_apps.len() > 32
            || self.allowed_apps.iter().any(|app| {
                app.is_empty()
                    || app.len() > 64
                    || !app.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.'
                    })
            })
        {
            return Err(GuestHandshakeErrorV1::InvalidBinding);
        }
        decode_hex(&self.secret_hex).map(|_| ())
    }

    pub(crate) fn secret(&self) -> Result<Zeroizing<[u8; 32]>, GuestHandshakeErrorV1> {
        self.validate()?;
        let bytes = decode_hex(&self.secret_hex)?;
        let secret: [u8; 32] = bytes
            .try_into()
            .map_err(|_| GuestHandshakeErrorV1::InvalidBinding)?;
        Ok(Zeroizing::new(secret))
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn read_private_host_file(path: &Path) -> Result<Self, GuestProtocolErrorV1> {
        let metadata = fs::symlink_metadata(path).map_err(|_| GuestProtocolErrorV1::Io)?;
        if !metadata.file_type().is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
            || metadata.len() > 4096
        {
            return Err(GuestProtocolErrorV1::WrongSession);
        }
        let mut options = fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        let mut bytes = Zeroizing::new(Vec::new());
        options
            .open(path)
            .and_then(|file| file.take(4097).read_to_end(&mut bytes))
            .map_err(|_| GuestProtocolErrorV1::Io)?;
        if bytes.is_empty() || bytes.len() > 4096 {
            return Err(GuestProtocolErrorV1::Oversized);
        }
        let config: Self =
            serde_json::from_slice(&bytes).map_err(|_| GuestProtocolErrorV1::Malformed)?;
        config
            .validate()
            .map_err(|_| GuestProtocolErrorV1::WrongSession)?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum GuestHandshakeErrorV1 {
    InvalidBinding,
    InvalidEnvelope,
    InvalidProof,
    Replay,
    UnsupportedVersion,
    Io,
}

impl std::fmt::Display for GuestHandshakeErrorV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "computer-use guest handshake error: {self:?}")
    }
}

impl std::error::Error for GuestHandshakeErrorV1 {}

pub(crate) struct GuestChannelKeyV1(Zeroizing<[u8; 32]>);

impl GuestChannelKeyV1 {
    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub(crate) fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuthenticatedGuestEnvelopeV1 {
    pub envelope: GuestEnvelopeV1,
    pub mac_hex: String,
}

impl AuthenticatedGuestEnvelopeV1 {
    pub(crate) fn sign(
        envelope: GuestEnvelopeV1,
        key: &[u8; 32],
    ) -> Result<Self, GuestProtocolErrorV1> {
        envelope.validate_common()?;
        let bytes = serde_json::to_vec(&envelope).map_err(|_| GuestProtocolErrorV1::Malformed)?;
        let mut mac =
            HmacSha256::new_from_slice(key).map_err(|_| GuestProtocolErrorV1::Malformed)?;
        mac.update(CHANNEL_DOMAIN);
        mac.update(&bytes);
        Ok(Self {
            envelope,
            mac_hex: encode_hex(&mac.finalize().into_bytes()),
        })
    }

    pub(crate) fn verify(&self, key: &[u8; 32]) -> Result<(), GuestProtocolErrorV1> {
        self.envelope.validate_common()?;
        let tag =
            decode_hex(&self.mac_hex).map_err(|_| GuestProtocolErrorV1::AuthenticationFailed)?;
        let bytes =
            serde_json::to_vec(&self.envelope).map_err(|_| GuestProtocolErrorV1::Malformed)?;
        let mut mac =
            HmacSha256::new_from_slice(key).map_err(|_| GuestProtocolErrorV1::Malformed)?;
        mac.update(CHANNEL_DOMAIN);
        mac.update(&bytes);
        mac.verify_slice(&tag)
            .map_err(|_| GuestProtocolErrorV1::AuthenticationFailed)
    }
}

pub(crate) fn derive_channel_key(
    boot_secret: &[u8; 32],
    guest_nonce: &[u8; 32],
    host_nonce: &[u8; 32],
    transcript_sha256: &str,
) -> Result<GuestChannelKeyV1, GuestHandshakeErrorV1> {
    let mut salt = [0; 64];
    salt[..32].copy_from_slice(guest_nonce);
    salt[32..].copy_from_slice(host_nonce);
    let hkdf = Hkdf::<Sha256>::new(Some(&salt), boot_secret);
    let mut info = CHANNEL_DOMAIN.to_vec();
    info.extend_from_slice(transcript_sha256.as_bytes());
    let mut key = [0; 32];
    hkdf.expand(&info, &mut key)
        .map_err(|_| GuestHandshakeErrorV1::InvalidBinding)?;
    salt.zeroize();
    info.zeroize();
    Ok(GuestChannelKeyV1::from_bytes(key))
}

pub(crate) fn transcript_digest(
    config: &GuestBootConfigV1,
    request_id: &str,
    action_id: &str,
    guest_nonce: &[u8; 32],
    host_nonce: &[u8; 32],
    capabilities_json: &[u8],
) -> String {
    let mut digest = Sha256::new();
    digest.update(CHANNEL_DOMAIN);
    digest.update(config.session_id.as_bytes());
    digest.update(config.worker_id.as_bytes());
    digest.update(config.lease_generation.to_be_bytes());
    digest.update(config.initial_action_sequence.to_be_bytes());
    digest.update(config.expires_at_unix_ms.to_be_bytes());
    digest.update(config.boot_id.as_bytes());
    digest.update(request_id.as_bytes());
    digest.update(action_id.as_bytes());
    digest.update(guest_nonce);
    digest.update(host_nonce);
    digest.update(capabilities_json);
    encode_hex(&digest.finalize())
}

pub(crate) fn decode_hex(value: &str) -> Result<Vec<u8>, GuestHandshakeErrorV1> {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return Err(GuestHandshakeErrorV1::InvalidBinding);
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => Ok(byte - b'0'),
                b'a'..=b'f' => Ok(byte - b'a' + 10),
                _ => Err(GuestHandshakeErrorV1::InvalidBinding),
            };
            Ok((digit(pair[0])? << 4) | digit(pair[1])?)
        })
        .collect()
}

pub(crate) fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
