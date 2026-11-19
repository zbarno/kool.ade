use std::collections::{BTreeMap, BTreeSet};

use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

use crate::computer_use::{
    backend::ComputerHostPlatformV1, guest_image::ComputerGuestArchitectureV1,
};

use super::encoding::{is_release_tag, is_sha256, is_signature_hex, is_uuid, sha256};

pub const RUNTIME_MANIFEST_SCHEMA_VERSION: u16 = 1;

/// Hard cap on the signed envelope. A runtime manifest lists a handful of
/// artifacts; 1 MiB leaves generous slack while bounding parse cost.
pub const MAX_RUNTIME_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_RUNTIME_ARTIFACT_ENTRIES: usize = 32;
const MAX_BUNDLE_RELATIVE_PATH_BYTES: usize = 256;
/// Aligned with the guest image catalog artifact ceiling.
const MAX_RUNTIME_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_LIFETIME_MS: u64 = 180 * 24 * 60 * 60 * 1000;

#[derive(Debug)]
pub enum RuntimeManifestErrorV1 {
    TooLarge,
    DecodeFailed,
    InvalidHeader,
    InvalidArtifact,
    DuplicateArtifact,
    InvalidLayout,
    EncodingFailed,
    UntrustedTrustStore,
    UnknownSigner,
    BadSignature,
    Expired,
    BelowGenerationFloor,
    Io,
}

impl std::fmt::Display for RuntimeManifestErrorV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "computer-use runtime manifest error: {self:?}")
    }
}

impl std::error::Error for RuntimeManifestErrorV1 {}

/// Host platforms a runtime manifest may address. `Unsupported` is
/// deliberately absent: a manifest that purports to carry an artifact for an
/// unrecognized host is structurally invalid.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeManifestPlatformV1 {
    LinuxX86_64,
    LinuxAarch64,
    MacOsIntel,
    MacOsAppleSilicon,
    WindowsX64,
    WindowsArm64,
}

impl RuntimeManifestPlatformV1 {
    pub const fn host_platform(self) -> ComputerHostPlatformV1 {
        match self {
            Self::LinuxX86_64 => ComputerHostPlatformV1::LinuxX86_64,
            Self::LinuxAarch64 => ComputerHostPlatformV1::LinuxAarch64,
            Self::MacOsIntel => ComputerHostPlatformV1::MacOsIntel,
            Self::MacOsAppleSilicon => ComputerHostPlatformV1::MacOsAppleSilicon,
            Self::WindowsX64 => ComputerHostPlatformV1::WindowsX64,
            Self::WindowsArm64 => ComputerHostPlatformV1::WindowsArm64,
        }
    }

    /// The guest architecture the platform must emulate. Cross-architecture
    /// guests are a separate, future manifest capability.
    pub const fn required_guest_architecture(self) -> ComputerGuestArchitectureV1 {
        match self {
            Self::LinuxX86_64 | Self::MacOsIntel | Self::WindowsX64 => {
                ComputerGuestArchitectureV1::X86_64
            }
            Self::LinuxAarch64 | Self::MacOsAppleSilicon | Self::WindowsArm64 => {
                ComputerGuestArchitectureV1::Aarch64
            }
        }
    }

    /// Bundles must publish QEMU under this canonical relative path so
    /// lookup is exact equality — no path resolution is delegated to the
    /// manifest author. Empty strings denote platforms whose adapters have
    /// not adopted this manifest yet.
    pub const fn canonical_qemu_executable_path(self) -> &'static str {
        match self {
            Self::MacOsIntel => "qemu/qemu-system-x86_64",
            Self::MacOsAppleSilicon => "qemu/qemu-system-aarch64",
            Self::WindowsX64 => "qemu/qemu-system-x86_64.exe",
            Self::WindowsArm64 => "qemu/qemu-system-aarch64.exe",
            Self::LinuxX86_64 | Self::LinuxAarch64 => "",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeArtifactKindV1 {
    /// Native executable launched by a host adapter (e.g. QEMU).
    Executable,
    /// Passive data file (configs, licenses, certificates).
    File,
    /// Bundled guest disk image consumed by an adapter.
    DiskImage,
}

impl std::fmt::Display for RuntimeArtifactKindV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Executable => "executable",
            Self::File => "file",
            Self::DiskImage => "disk_image",
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeArtifactV1 {
    pub artifact_id: String,
    pub kind: RuntimeArtifactKindV1,
    pub platform: RuntimeManifestPlatformV1,
    pub guest_architecture: ComputerGuestArchitectureV1,
    /// Canonical POSIX path relative to the bundle resource root.
    pub bundle_relative_path: String,
    /// Lowercase `sha256:<hex>` digest of the exact shipped bytes.
    pub sha256: String,
    pub size_bytes: u64,
    /// Unix mode for `Executable` entries; must be zero for the other kinds.
    pub permission_bits: u32,
}

impl RuntimeArtifactV1 {
    pub(super) fn validate(&self) -> Result<(), RuntimeManifestErrorV1> {
        let invalid = || RuntimeManifestErrorV1::InvalidArtifact;
        if !is_uuid(&self.artifact_id) {
            return Err(invalid());
        }
        if self.guest_architecture != self.platform.required_guest_architecture() {
            return Err(invalid());
        }
        if !is_valid_bundle_relative_path(&self.bundle_relative_path) {
            return Err(invalid());
        }
        if !is_sha256(&self.sha256)
            || self.size_bytes == 0
            || self.size_bytes > MAX_RUNTIME_ARTIFACT_BYTES
        {
            return Err(invalid());
        }
        match self.kind {
            RuntimeArtifactKindV1::Executable => {
                // Reject setuid/setgid/sticky, require owner execute, and
                // forbid group/other write. We deliberately do not demand
                // group/other execute: legitimate bundles commonly ship
                // stricter modes such as 0700. The on-disk permission
                // predicates in each platform receipt remain authoritative.
                if self.permission_bits & 0o7000 != 0
                    || self.permission_bits & 0o100 == 0
                    || self.permission_bits & 0o022 != 0
                {
                    return Err(invalid());
                }
            }
            RuntimeArtifactKindV1::File | RuntimeArtifactKindV1::DiskImage => {
                if self.permission_bits != 0 {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}

fn is_valid_bundle_relative_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_BUNDLE_RELATIVE_PATH_BYTES {
        return false;
    }
    for &byte in bytes {
        let acceptable = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/');
        if !acceptable {
            return false;
        }
    }
    let mut segments = 0_usize;
    for segment in path.split('/') {
        segments += 1;
        if segments > 8 || segment.is_empty() || segment == "." || segment == ".." {
            return false;
        }
    }
    true
}

/// Signed manifest shipping alongside an application release.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RuntimeManifestV1 {
    pub schema_version: u16,
    /// Version of the application release this manifest was built for.
    pub release_version: String,
    pub generation: u64,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub signer_key_id: String,
    pub artifacts: Vec<RuntimeArtifactV1>,
    /// Lowercase hex Ed25519 signature over the domain-tagged payload.
    pub signature_hex: String,
}

impl RuntimeManifestV1 {
    pub fn validate(&self) -> Result<(), RuntimeManifestErrorV1> {
        if self.schema_version != RUNTIME_MANIFEST_SCHEMA_VERSION
            || !is_release_tag(&self.release_version)
            || self.generation == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self.expires_at_unix_ms - self.issued_at_unix_ms > MAX_LIFETIME_MS
            || !is_uuid(&self.signer_key_id)
            || self.artifacts.is_empty()
            || self.artifacts.len() > MAX_RUNTIME_ARTIFACT_ENTRIES
            || !is_signature_hex(&self.signature_hex)
        {
            return Err(RuntimeManifestErrorV1::InvalidHeader);
        }
        let mut ids = BTreeSet::new();
        let mut layouts = BTreeSet::new();
        for artifact in &self.artifacts {
            artifact.validate()?;
            if !ids.insert(artifact.artifact_id.as_str())
                || !layouts.insert((artifact.platform, artifact.bundle_relative_path.clone()))
            {
                return Err(RuntimeManifestErrorV1::DuplicateArtifact);
            }
        }
        Ok(())
    }

    /// Canonical-envelope digest; bind it wherever a downstream session
    /// record must prove which manifest it relied on.
    pub fn digest(&self) -> Result<String, RuntimeManifestErrorV1> {
        Ok(sha256(&super::encoding::canonical_manifest_bytes(self)?))
    }

    /// Bytes an offline release signer must sign for this manifest.
    pub fn signing_payload_bytes(&self) -> Result<Vec<u8>, RuntimeManifestErrorV1> {
        super::encoding::signing_payload_bytes(self)
    }
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeManifestTrustV1 {
    keys: BTreeMap<String, VerifyingKey>,
    minimum_generation: u64,
}

impl RuntimeManifestTrustV1 {
    /// Keys must come from app-pinned trust; the generation floor is
    /// durable host state advanced after each successful verification.
    pub fn from_application_pinned_keys(
        keys: impl IntoIterator<Item = (String, VerifyingKey)>,
        minimum_generation: u64,
    ) -> Result<Self, RuntimeManifestErrorV1> {
        let mut trusted = BTreeMap::new();
        for (key_id, key) in keys {
            if !is_uuid(&key_id) || trusted.insert(key_id, key).is_some() {
                return Err(RuntimeManifestErrorV1::UntrustedTrustStore);
            }
        }
        if trusted.is_empty() || minimum_generation == 0 {
            return Err(RuntimeManifestErrorV1::UntrustedTrustStore);
        }
        Ok(Self {
            keys: trusted,
            minimum_generation,
        })
    }

    pub(super) fn key(&self, id: &str) -> Option<&VerifyingKey> {
        self.keys.get(id)
    }

    pub fn minimum_generation(&self) -> u64 {
        self.minimum_generation
    }
}
