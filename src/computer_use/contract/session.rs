use super::{
    ComputerIdentityV1, ComputerLeaseV1, ComputerLifecycleV1, ComputerPolicyV1,
    ComputerSessionClassV1,
};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComputerTargetV1 {
    LocalKvm { machine_id: String },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerControllerProofV1 {
    pub signer_key_id: String,
    /// Ed25519 signature over the domain-separated session authority payload.
    pub signature_hex: String,
}

impl ComputerControllerProofV1 {
    fn validate(&self) -> Result<(), &'static str> {
        if !is_uuid(&self.signer_key_id)
            || self.signature_hex.len() != 128
            || !self
                .signature_hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("controller proof is malformed");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerSessionV1 {
    pub schema_version: u16,
    pub identity: ComputerIdentityV1,
    pub session_class: ComputerSessionClassV1,
    pub target: ComputerTargetV1,
    pub image_digest: String,
    pub network_profile: super::ComputerNetworkProfileV1,
    pub allowed_apps: std::collections::BTreeSet<String>,
    pub policy: ComputerPolicyV1,
    pub lease: ComputerLeaseV1,
    pub lifecycle: ComputerLifecycleV1,
    pub action_sequence: u64,
    pub approval_generation: u64,
    pub cancellation_generation: u64,
    pub evidence_refs: Vec<String>,
    pub controller_proof: ComputerControllerProofV1,
}

impl ComputerSessionV1 {
    pub fn validate(&self, now_unix_ms: u64) -> Result<(), &'static str> {
        if self.schema_version != 1 {
            return Err("unsupported session version");
        }
        self.controller_proof.validate()?;
        self.identity.validate()?;
        self.policy.validate()?;
        self.lease.validate(now_unix_ms)?;
        if self.policy.identity != self.identity || self.policy.session_class != self.session_class
        {
            return Err("session policy identity mismatch");
        }
        if self.policy.network_profile != self.network_profile
            || self.policy.allowed_apps != self.allowed_apps
        {
            return Err("session authority differs from its policy");
        }
        if self.lease.owner_id != self.identity.user_id
            || self.lease.cancellation_generation != self.cancellation_generation
        {
            return Err("session lease authority mismatch");
        }
        if self.approval_generation != self.policy.approval_generation
            || self.action_sequence > self.policy.resource_budget.action_limit as u64
        {
            return Err("session generation or action quota mismatch");
        }
        let machine_id = match &self.target {
            ComputerTargetV1::LocalKvm { machine_id } => machine_id,
        };
        if !is_uuid(machine_id) || !valid_digest(&self.image_digest) {
            return Err("invalid target or pinned image digest");
        }
        if self.evidence_refs.len() > 128 || self.evidence_refs.iter().any(|id| !is_uuid(id)) {
            return Err("invalid evidence references");
        }
        Ok(())
    }

    /// Sign only inside the trusted controller. Never send its signing key to
    /// a provider, guest, task repository, or remote VM worker.
    pub fn attest(
        &mut self,
        signer_key_id: String,
        signing_key: &SigningKey,
    ) -> Result<(), &'static str> {
        if !is_uuid(&signer_key_id) {
            return Err("controller signing key identity is invalid");
        }
        let signature = signing_key.sign(&self.signing_payload()?);
        self.controller_proof = ComputerControllerProofV1 {
            signer_key_id,
            signature_hex: encode_hex(&signature.to_bytes()),
        };
        Ok(())
    }

    /// The caller selects this key from the host's trusted key registry.
    pub fn verify_attestation(
        &self,
        expected_key_id: &str,
        verifying_key: &VerifyingKey,
    ) -> Result<(), &'static str> {
        self.controller_proof.validate()?;
        if self.controller_proof.signer_key_id != expected_key_id {
            return Err("controller signing key identity mismatch");
        }
        let signature = Signature::from_slice(&decode_hex(&self.controller_proof.signature_hex)?)
            .map_err(|_| "controller signature is malformed")?;
        verifying_key
            .verify(&self.signing_payload()?, &signature)
            .map_err(|_| "controller session attestation is invalid")
    }

    pub fn validate_for_host(
        &self,
        now_unix_ms: u64,
        expected_key_id: &str,
        verifying_key: &VerifyingKey,
    ) -> Result<(), &'static str> {
        self.validate(now_unix_ms)?;
        self.verify_attestation(expected_key_id, verifying_key)
    }

    fn signing_payload(&self) -> Result<Vec<u8>, &'static str> {
        let mut bytes = b"koolade.computer-session.v1\0".to_vec();
        let value = serde_json::to_value(UnsignedComputerSessionV1::from(self))
            .map_err(|_| "session signing payload could not be encoded")?;
        bytes.extend(
            serde_json::to_vec(&value)
                .map_err(|_| "session signing payload could not be encoded")?,
        );
        Ok(bytes)
    }
}

#[derive(Serialize)]
struct UnsignedComputerSessionV1<'a> {
    schema_version: u16,
    identity: &'a ComputerIdentityV1,
    session_class: ComputerSessionClassV1,
    target: &'a ComputerTargetV1,
    image_digest: &'a str,
    network_profile: super::ComputerNetworkProfileV1,
    allowed_apps: &'a std::collections::BTreeSet<String>,
    policy: &'a ComputerPolicyV1,
    lease: &'a ComputerLeaseV1,
    lifecycle: ComputerLifecycleV1,
    action_sequence: u64,
    approval_generation: u64,
    cancellation_generation: u64,
    evidence_refs: &'a [String],
}

impl<'a> From<&'a ComputerSessionV1> for UnsignedComputerSessionV1<'a> {
    fn from(session: &'a ComputerSessionV1) -> Self {
        Self {
            schema_version: session.schema_version,
            identity: &session.identity,
            session_class: session.session_class,
            target: &session.target,
            image_digest: &session.image_digest,
            network_profile: session.network_profile,
            allowed_apps: &session.allowed_apps,
            policy: &session.policy,
            lease: &session.lease,
            lifecycle: session.lifecycle,
            action_sequence: session.action_sequence,
            approval_generation: session.approval_generation,
            cancellation_generation: session.cancellation_generation,
            evidence_refs: &session.evidence_refs,
        }
    }
}

fn is_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn decode_hex(value: &str) -> Result<Vec<u8>, &'static str> {
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err("controller signature is malformed");
    }
    pairs
        .iter()
        .map(|pair| {
            let digit = |b: u8| match b {
                b'0'..=b'9' => Ok(b - b'0'),
                b'a'..=b'f' => Ok(b - b'a' + 10),
                _ => Err("controller signature is malformed"),
            };
            Ok((digit(pair[0])? << 4) | digit(pair[1])?)
        })
        .collect()
}
