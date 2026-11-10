use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::computer_use::{COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1, ComputerSessionClassV1};

use super::enrollment::ComputerRemoteWorkerEnrollmentV1;

const MAX_ADVERTISEMENT_AGE_MS: u64 = 60_000;
const MAX_IMAGE_DIGESTS: usize = 128;

/// Untrusted worker observations received only after the SSH host-key and
/// worker principal have been verified by the controller transport.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ComputerRemoteWorkerAdvertisementV1 {
    pub schema_version: u16,
    pub worker_id: String,
    pub attestation_key_id: String,
    pub signature_hex: String,
    pub ssh_host_key_fingerprint: String,
    pub transport_protocol_version: u16,
    pub guest_protocol_version: u16,
    pub observed_at_unix_ms: u64,
    pub cpu_virtualization_available: bool,
    pub image_digests: Vec<String>,
    pub display_streaming_available: bool,
    pub cpu_millis: u32,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    pub encrypted_personal_storage_available: bool,
    pub parallel_slots: u16,
}

impl ComputerRemoteWorkerAdvertisementV1 {
    pub fn validate(&self) -> Result<(), RemoteComputerWorkerEligibilityV1> {
        if self.schema_version != 1
            || !valid_worker_id(&self.worker_id)
            || !canonical_uuid(&self.attestation_key_id)
            || self.signature_hex.len() != 128
            || !self.signature_hex.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(RemoteComputerWorkerEligibilityV1::InvalidAdvertisement);
        }
        if !valid_fingerprint(&self.ssh_host_key_fingerprint)
            || self.transport_protocol_version != COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1
            || self.guest_protocol_version
                != crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1
        {
            return Err(RemoteComputerWorkerEligibilityV1::ProtocolOrIdentityMismatch);
        }
        if self.observed_at_unix_ms == 0
            || self.memory_bytes == 0
            || self.disk_bytes == 0
            || self.cpu_millis == 0
            || self.cpu_millis > 1_000_000
            || self.parallel_slots == 0
            || self.parallel_slots > 256
            || self.image_digests.is_empty()
            || self.image_digests.len() > MAX_IMAGE_DIGESTS
            || self
                .image_digests
                .iter()
                .any(|digest| !valid_digest(digest))
        {
            return Err(RemoteComputerWorkerEligibilityV1::InvalidAdvertisement);
        }
        Ok(())
    }

    pub fn attest(&mut self, key_id: String, key: &SigningKey) -> Result<(), &'static str> {
        if !canonical_uuid(&key_id) {
            return Err("worker attestation key ID is invalid");
        }
        self.attestation_key_id = key_id;
        self.signature_hex.clear();
        self.signature_hex = encode_hex(&key.sign(&self.signing_payload()?).to_bytes());
        Ok(())
    }

    fn verify_attestation(
        &self,
        enrollment: &ComputerRemoteWorkerEnrollmentV1,
        key: &VerifyingKey,
    ) -> bool {
        if self.attestation_key_id != enrollment.worker_attestation_key_id {
            return false;
        }
        let Ok(payload) = self.signing_payload() else {
            return false;
        };
        let Some(bytes) = decode_hex(&self.signature_hex) else {
            return false;
        };
        let Ok(signature) = Signature::from_slice(&bytes) else {
            return false;
        };
        key.verify(&payload, &signature).is_ok()
    }

    fn signing_payload(&self) -> Result<Vec<u8>, &'static str> {
        serde_json::to_vec(&(
            b"koolade.remote-computer-worker.advertisement.v1".as_slice(),
            self.schema_version,
            &self.worker_id,
            &self.attestation_key_id,
            &self.ssh_host_key_fingerprint,
            self.transport_protocol_version,
            self.guest_protocol_version,
            self.observed_at_unix_ms,
            self.cpu_virtualization_available,
            &self.image_digests,
            self.display_streaming_available,
            self.cpu_millis,
            self.memory_bytes,
            self.disk_bytes,
            self.encrypted_personal_storage_available,
            self.parallel_slots,
        ))
        .map_err(|_| "worker advertisement serialization failed")
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerRemoteWorkerRequirementsV1 {
    pub image_digest: String,
    pub cpu_millis: u32,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    pub require_display_streaming: bool,
    pub session_class: ComputerSessionClassV1,
}

impl ComputerRemoteWorkerRequirementsV1 {
    pub fn validate(&self) -> Result<(), RemoteComputerWorkerEligibilityV1> {
        if !valid_digest(&self.image_digest)
            || self.cpu_millis == 0
            || self.memory_bytes == 0
            || self.disk_bytes == 0
            || self.cpu_millis > 1_000_000
            || self.memory_bytes > 1 << 50
            || self.disk_bytes > 1 << 50
        {
            return Err(RemoteComputerWorkerEligibilityV1::InvalidRequirements);
        }
        if self.session_class != ComputerSessionClassV1::Disposable {
            return Err(RemoteComputerWorkerEligibilityV1::PersonalVmNotInScope);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteComputerWorkerEligibilityV1 {
    Eligible,
    InvalidAdvertisement,
    InvalidRequirements,
    WorkerNotEnrolled,
    WorkerDisabled,
    ProtocolOrIdentityMismatch,
    StaleAdvertisement,
    VirtualizationUnavailable,
    ImageNotApproved,
    DisplayUnavailable,
    MemoryInsufficient,
    DiskInsufficient,
    CapacityUnavailable,
    ReleaseGateClosed,
    TargetApprovalMismatch,
    SessionAlreadyScheduled,
    SchedulerCapacityExceeded,
    PersonalVmNotInScope,
}

/// Compare an authenticated probe against operator enrollment and the exact
/// controller-approved session requirements. Advertised values never widen
/// the trusted image or resource policy. The caller must pass this result to
/// the release gate and explicit target-approval scheduler before allocation.
pub fn assess_remote_computer_worker_v1(
    enrollment: Option<&ComputerRemoteWorkerEnrollmentV1>,
    advertisement: &ComputerRemoteWorkerAdvertisementV1,
    requirements: &ComputerRemoteWorkerRequirementsV1,
    now_unix_ms: u64,
) -> RemoteComputerWorkerEligibilityV1 {
    if let Err(reason) = advertisement.validate() {
        return reason;
    }
    if let Err(reason) = requirements.validate() {
        return reason;
    }
    let Some(enrollment) = enrollment else {
        return RemoteComputerWorkerEligibilityV1::WorkerNotEnrolled;
    };
    let Ok(worker_key) = enrollment.validate() else {
        return RemoteComputerWorkerEligibilityV1::ProtocolOrIdentityMismatch;
    };
    if enrollment.worker_id != advertisement.worker_id
        || enrollment.ssh_host_key_fingerprint != advertisement.ssh_host_key_fingerprint
        || !advertisement.verify_attestation(enrollment, &worker_key)
    {
        return RemoteComputerWorkerEligibilityV1::ProtocolOrIdentityMismatch;
    }
    if !enrollment.enabled {
        return RemoteComputerWorkerEligibilityV1::WorkerDisabled;
    }
    if advertisement.observed_at_unix_ms > now_unix_ms
        || now_unix_ms.saturating_sub(advertisement.observed_at_unix_ms) > MAX_ADVERTISEMENT_AGE_MS
    {
        return RemoteComputerWorkerEligibilityV1::StaleAdvertisement;
    }
    if !advertisement.cpu_virtualization_available {
        return RemoteComputerWorkerEligibilityV1::VirtualizationUnavailable;
    }
    if !enrollment
        .trusted_image_digests
        .iter()
        .any(|digest| digest == &requirements.image_digest)
        || !advertisement
            .image_digests
            .iter()
            .any(|digest| digest == &requirements.image_digest)
    {
        return RemoteComputerWorkerEligibilityV1::ImageNotApproved;
    }
    if requirements.require_display_streaming && !advertisement.display_streaming_available {
        return RemoteComputerWorkerEligibilityV1::DisplayUnavailable;
    }
    if advertisement.memory_bytes < requirements.memory_bytes {
        return RemoteComputerWorkerEligibilityV1::MemoryInsufficient;
    }
    if advertisement.disk_bytes < requirements.disk_bytes {
        return RemoteComputerWorkerEligibilityV1::DiskInsufficient;
    }
    if advertisement.parallel_slots == 0
        || advertisement.parallel_slots > enrollment.max_parallel_sessions
    {
        return RemoteComputerWorkerEligibilityV1::CapacityUnavailable;
    }
    RemoteComputerWorkerEligibilityV1::Eligible
}

fn valid_worker_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_fingerprint(value: &str) -> bool {
    value.starts_with("SHA256:")
        && value.len() == 50
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/'))
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() != 128 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| -> Option<u8> {
                Some(match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    b'A'..=b'F' => byte - b'A' + 10,
                    _ => return None,
                })
            };
            Some(digit(pair[0])? * 16 + digit(pair[1])?)
        })
        .collect()
}

#[cfg(test)]
mod tests;
