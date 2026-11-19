//! Verification receipt for an app-bundled macOS QEMU executable.
//!
//! The expected digest must come from an independently authenticated runtime
//! manifest. This module does not discover QEMU through `PATH` or establish
//! that manifest trust on its own.

use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use super::{ComputerHostPlatformV1, qemu_plan::MacOsQemuLaunchPlanV1};
use crate::computer_use::runtime_manifest::{
    RuntimeArtifactKindV1, RuntimeManifestPlatformV1, VerifiedRuntimeArtifactV1,
};

const MAX_QEMU_BINARY_BYTES: u64 = 512 * 1024 * 1024;

/// Origin of a receipt's expected digest. Present whenever the receipt was
/// built from an entry of a runtime manifest verified under the pinned
/// application trust; `None` marks receipts minted by the standalone
/// verification entrypoint (no manifest provenance to attest).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MacOsRuntimeManifestProvenanceV1 {
    manifest_digest: String,
    artifact_id: String,
}

impl MacOsRuntimeManifestProvenanceV1 {
    pub(crate) fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    pub(crate) fn artifact_id(&self) -> &str {
        &self.artifact_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MacOsTrustedQemuRuntimeV1 {
    bundle_resources: PathBuf,
    path: PathBuf,
    platform: ComputerHostPlatformV1,
    sha256: String,
    manifest_provenance: Option<MacOsRuntimeManifestProvenanceV1>,
}

impl MacOsTrustedQemuRuntimeV1 {
    pub(crate) fn manifest_provenance(&self) -> Option<&MacOsRuntimeManifestProvenanceV1> {
        self.manifest_provenance.as_ref()
    }

    #[cfg(test)]
    pub(crate) fn fixture(path: PathBuf, platform: ComputerHostPlatformV1) -> Self {
        let bundle_resources = path.parent().expect("fixture has a parent").to_path_buf();
        let path = fs::canonicalize(path).expect("canonicalize QEMU fixture");
        let sha256 = hash_file(&path).expect("hash QEMU fixture");
        Self {
            bundle_resources,
            path,
            platform,
            sha256,
            manifest_provenance: None,
        }
    }

    /// Build a receipt from one entry of a runtime manifest already verified
    /// under the pinned application trust. No filesystem discovery happens
    /// here: the enforcing predicates (presence, symlink refusal, permissions,
    /// size bounds, digest parity with disk) are all re-checked at launch
    /// time by `path_for_plan`.
    pub(crate) fn from_manifest_artifact(
        bundle_resources: &Path,
        manifest_digest: &str,
        artifact: &VerifiedRuntimeArtifactV1,
    ) -> io::Result<Self> {
        let platform = match artifact.platform() {
            RuntimeManifestPlatformV1::MacOsIntel => ComputerHostPlatformV1::MacOsIntel,
            RuntimeManifestPlatformV1::MacOsAppleSilicon => {
                ComputerHostPlatformV1::MacOsAppleSilicon
            }
            _ => return Err(invalid_runtime()),
        };
        if artifact.kind() != RuntimeArtifactKindV1::Executable
            || artifact.bundle_relative_path()
                != artifact.platform().canonical_qemu_executable_path()
            || artifact.size_bytes() == 0
            || artifact.size_bytes() > MAX_QEMU_BINARY_BYTES
            || !is_trusted_digest_spelling(artifact.digest())
            || !is_trusted_digest_spelling(manifest_digest)
        {
            return Err(invalid_runtime());
        }
        let root = fs::canonicalize(bundle_resources)?;
        let path = root.join(artifact.bundle_relative_path());
        Ok(Self {
            bundle_resources: root,
            path,
            platform,
            sha256: artifact.digest().to_owned(),
            manifest_provenance: Some(MacOsRuntimeManifestProvenanceV1 {
                manifest_digest: manifest_digest.to_owned(),
                artifact_id: artifact.artifact_id().to_owned(),
            }),
        })
    }

    /// Create a runtime receipt only for an executable beneath the app's
    /// canonical resource directory with the architecture-specific name and
    /// the digest supplied by a trusted app runtime manifest.
    pub(crate) fn verify(
        bundle_resources: &Path,
        executable: &Path,
        expected_sha256: &str,
        platform: ComputerHostPlatformV1,
    ) -> io::Result<Self> {
        if !matches!(
            platform,
            ComputerHostPlatformV1::MacOsIntel | ComputerHostPlatformV1::MacOsAppleSilicon
        ) || expected_sha256.len() != 71
            || !expected_sha256.starts_with("sha256:")
            || !expected_sha256[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(invalid_runtime());
        }
        let expected_name = match platform {
            ComputerHostPlatformV1::MacOsIntel => "qemu-system-x86_64",
            ComputerHostPlatformV1::MacOsAppleSilicon => "qemu-system-aarch64",
            _ => return Err(invalid_runtime()),
        };
        if executable.file_name().and_then(|name| name.to_str()) != Some(expected_name) {
            return Err(invalid_runtime());
        }
        let root = fs::canonicalize(bundle_resources)?;
        let metadata = fs::symlink_metadata(executable)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_QEMU_BINARY_BYTES
        {
            return Err(invalid_runtime());
        }
        let path = fs::canonicalize(executable)?;
        if !path.starts_with(&root) {
            return Err(invalid_runtime());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                return Err(invalid_runtime());
            }
        }
        let actual = hash_file(&path)?;
        if actual != expected_sha256 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "bundled QEMU digest does not match the trusted runtime manifest",
            ));
        }
        Ok(Self {
            bundle_resources: root,
            path,
            platform,
            sha256: actual,
            manifest_provenance: None,
        })
    }

    pub(crate) fn path_for_plan(&self, plan: &MacOsQemuLaunchPlanV1) -> io::Result<&Path> {
        if self.platform != plan.platform
            || self.path.file_name().and_then(|name| name.to_str()) != Some(plan.executable)
        {
            return Err(invalid_runtime());
        }
        let metadata = fs::symlink_metadata(&self.path)?;
        let current_path = fs::canonicalize(&self.path)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_QEMU_BINARY_BYTES
            || !current_path.starts_with(&self.bundle_resources)
            || current_path != self.path
            || hash_file(&current_path)? != self.sha256
        {
            return Err(invalid_runtime());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0
                || metadata.permissions().mode() & 0o022 != 0
            {
                return Err(invalid_runtime());
            }
        }
        Ok(&self.path)
    }

    #[cfg(test)]
    fn sha256(&self) -> &str {
        &self.sha256
    }
}

pub(super) fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn is_trusted_digest_spelling(digest: &str) -> bool {
    digest.len() == 71
        && digest.starts_with("sha256:")
        && digest[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn invalid_runtime() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "macOS QEMU runtime is not a verified app-bundled executable for this launch plan",
    )
}

#[cfg(test)]
fn other_platform(platform: ComputerHostPlatformV1) -> ComputerHostPlatformV1 {
    match platform {
        ComputerHostPlatformV1::MacOsIntel => ComputerHostPlatformV1::MacOsAppleSilicon,
        ComputerHostPlatformV1::MacOsAppleSilicon => ComputerHostPlatformV1::MacOsIntel,
        _ => ComputerHostPlatformV1::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use std::{os::unix::fs::PermissionsExt, path::Path};

    use super::*;
    use crate::computer_use::macos_backend::qemu_plan::{
        MacOsQemuLaunchRequestV1, build_macos_qemu_launch_plan,
    };

    fn fixture(platform: ComputerHostPlatformV1) -> (tempfile::TempDir, PathBuf, String) {
        let directory = tempfile::tempdir().expect("bundle fixture");
        let resources = directory.path().join("Resources");
        fs::create_dir(&resources).expect("create resources");
        let name = match platform {
            ComputerHostPlatformV1::MacOsIntel => "qemu-system-x86_64",
            ComputerHostPlatformV1::MacOsAppleSilicon => "qemu-system-aarch64",
            _ => panic!("macOS platform expected"),
        };
        let executable = resources.join(name);
        fs::write(&executable, b"trusted qemu fixture").expect("write executable");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
            .expect("make executable");
        let digest = hash_file(&executable).expect("hash fixture");
        (directory, executable, digest)
    }

    fn plan(platform: ComputerHostPlatformV1) -> MacOsQemuLaunchPlanV1 {
        let executable = match platform {
            ComputerHostPlatformV1::MacOsIntel => "qemu-system-x86_64",
            ComputerHostPlatformV1::MacOsAppleSilicon => "qemu-system-aarch64",
            _ => panic!("macOS platform expected"),
        };
        let plan = build_macos_qemu_launch_plan(
            platform,
            MacOsQemuLaunchRequestV1 {
                vcpus: 2,
                memory_bytes: 1024 * 1024 * 1024,
                disk_bytes: 1024 * 1024 * 1024,
                overlay_virtual_size_bytes: 1024 * 1024 * 1024,
                overlay: Path::new("/private/overlay.qcow2"),
                control_socket: Path::new("/private/control.sock"),
                qmp_socket: Path::new("/private/qmp.sock"),
                display_socket: Path::new("/private/display.sock"),
                boot_config: Path::new("/private/boot.json"),
                app_helper_config: Path::new("/private/helper.json"),
            },
        )
        .expect("valid bounded plan");
        assert_eq!(plan.executable, executable);
        plan
    }

    #[test]
    fn runtime_receipt_requires_canonical_bundle_path_architecture_and_digest() {
        for platform in [
            ComputerHostPlatformV1::MacOsIntel,
            ComputerHostPlatformV1::MacOsAppleSilicon,
        ] {
            let (directory, executable, digest) = fixture(platform);
            let resources = directory.path().join("Resources");
            let receipt =
                MacOsTrustedQemuRuntimeV1::verify(&resources, &executable, &digest, platform)
                    .expect("valid bundled executable");
            assert_eq!(receipt.sha256(), digest);
            assert_eq!(receipt.path_for_plan(&plan(platform)).unwrap(), executable);
            assert!(
                MacOsTrustedQemuRuntimeV1::verify(
                    &resources,
                    &executable,
                    &format!("sha256:{}", "0".repeat(64)),
                    platform,
                )
                .is_err()
            );
            assert!(
                receipt
                    .path_for_plan(&plan(other_platform(platform)))
                    .is_err()
            );
            fs::write(&executable, b"replaced QEMU fixture").expect("tamper with fixture");
            assert!(receipt.path_for_plan(&plan(platform)).is_err());
        }
    }

    #[test]
    fn runtime_receipt_rejects_symlinks_outside_bundle_and_ambient_paths() {
        let (directory, executable, digest) = fixture(ComputerHostPlatformV1::MacOsIntel);
        let resources = directory.path().join("Resources");
        let symlink_directory = resources.join("alias");
        fs::create_dir(&symlink_directory).expect("create alias directory");
        let symlink = symlink_directory.join("qemu-system-x86_64");
        std::os::unix::fs::symlink(&executable, &symlink).expect("create symlink");
        assert!(
            MacOsTrustedQemuRuntimeV1::verify(
                &resources,
                &symlink,
                &digest,
                ComputerHostPlatformV1::MacOsIntel,
            )
            .is_err()
        );

        let outside = directory.path().join("qemu-system-x86_64");
        fs::write(&outside, b"outside").expect("write outside fixture");
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o700))
            .expect("make outside executable");
        let outside_digest = hash_file(&outside).expect("hash outside fixture");
        assert!(
            MacOsTrustedQemuRuntimeV1::verify(
                &resources,
                &outside,
                &outside_digest,
                ComputerHostPlatformV1::MacOsIntel,
            )
            .is_err()
        );
        assert!(
            MacOsTrustedQemuRuntimeV1::verify(
                &resources,
                &executable,
                &digest,
                ComputerHostPlatformV1::LinuxX86_64,
            )
            .is_err()
        );
    }

    // --- Receipts derived from authenticated runtime manifests -----------

    const MANIFEST_KEY_ID: &str = "11111111-2222-4333-8444-555555555555";
    const CLOCK_NOW_MS: u64 = 1_760_000_000_000;

    fn e2e_now(ms_offset: i64) -> u64 {
        (CLOCK_NOW_MS as i128 + ms_offset as i128) as u64
    }

    fn e2e_signer() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&[71; 32])
    }

    fn e2e_sign(manifest: &mut crate::computer_use::runtime_manifest::RuntimeManifestV1) {
        use ed25519_dalek::Signer;
        let key = e2e_signer();
        let signature = key.sign(&manifest.signing_payload_bytes().expect("signing payload"));
        manifest.signature_hex = e2e_hex(&signature.to_bytes());
    }

    fn e2e_hex(bytes: &[u8]) -> String {
        let mut out = String::new();
        for byte in bytes {
            out.push_str(&format!("{byte:02x}"));
        }
        out
    }

    fn e2e_manifest(
        mutate: impl FnOnce(&mut crate::computer_use::runtime_manifest::RuntimeManifestV1),
    ) -> crate::computer_use::runtime_manifest::RuntimeManifestV1 {
        let guest_architecture =
            crate::computer_use::runtime_manifest::RuntimeManifestPlatformV1::MacOsIntel
                .required_guest_architecture();
        let artifact = crate::computer_use::runtime_manifest::RuntimeArtifactV1 {
            artifact_id: "aaaaaaaa-bbbb-4ccc-8ddd-000000000001".to_owned(),
            kind: crate::computer_use::runtime_manifest::RuntimeArtifactKindV1::Executable,
            platform: crate::computer_use::runtime_manifest::RuntimeManifestPlatformV1::MacOsIntel,
            guest_architecture,
            bundle_relative_path: "qemu/qemu-system-x86_64".to_owned(),
            sha256: format!("sha256:{}", "0".repeat(64)),
            size_bytes: 1,
            permission_bits: 0o700,
        };
        let mut manifest = crate::computer_use::runtime_manifest::RuntimeManifestV1 {
            schema_version: crate::computer_use::runtime_manifest::RUNTIME_MANIFEST_SCHEMA_VERSION,
            release_version: "e2e-1".to_owned(),
            generation: 3,
            issued_at_unix_ms: e2e_now(-60_000),
            expires_at_unix_ms: e2e_now(3_600_000),
            signer_key_id: MANIFEST_KEY_ID.to_owned(),
            artifacts: vec![artifact],
            signature_hex: "0".repeat(128),
        };
        mutate(&mut manifest);
        e2e_sign(&mut manifest);
        manifest
    }

    fn e2e_load(
        resources: &std::path::Path,
        manifest: &crate::computer_use::runtime_manifest::RuntimeManifestV1,
    ) -> crate::computer_use::runtime_manifest::VerifiedRuntimeManifestV1 {
        std::fs::write(
            resources
                .join(crate::computer_use::runtime_manifest::BUNDLED_RUNTIME_MANIFEST_FILENAME),
            serde_json::to_vec(manifest).expect("encode manifest"),
        )
        .expect("stage manifest");
        let trust = crate::computer_use::runtime_manifest::RuntimeManifestTrustV1::from_application_pinned_keys(
            [(MANIFEST_KEY_ID.to_owned(), e2e_signer().verifying_key())],
            1,
        )
        .expect("test trust");
        crate::computer_use::runtime_manifest::load_bundled_runtime_manifest(
            resources,
            &trust,
            CLOCK_NOW_MS,
        )
        .expect("load staged manifest")
        .expect("manifest present")
    }

    #[test]
    fn manifest_derived_receipt_authorizes_the_bundled_executable_and_detects_swaps() {
        use crate::computer_use::runtime_manifest::RuntimeManifestPlatformV1;
        let platform = ComputerHostPlatformV1::MacOsIntel;
        let directory = tempfile::tempdir().expect("e2e bundle");
        let resources = directory.path().join("Resources");
        let executable = resources.join("qemu").join("qemu-system-x86_64");
        fs::create_dir_all(executable.parent().expect("qemu dir parent")).expect("qemu dir");
        let bytes = b"stub qemu standing in for the bundled binary";
        fs::write(&executable, bytes).expect("stage executable");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).expect("chmod");
        let digest = hash_file(&executable).expect("hash stub");

        let manifest = e2e_manifest(|manifest| {
            manifest
                .artifacts
                .first_mut()
                .expect("single artifact")
                .sha256 = digest.clone();
            manifest
                .artifacts
                .first_mut()
                .expect("single artifact")
                .size_bytes = bytes.len() as u64;
        });
        let verified = e2e_load(&resources, &manifest);
        let artifact = verified
            .qemu_executable_for(RuntimeManifestPlatformV1::MacOsIntel)
            .expect("canonical lookup");

        let receipt = MacOsTrustedQemuRuntimeV1::from_manifest_artifact(
            &resources,
            verified.digest(),
            &artifact,
        )
        .expect("receipt from verified artifact");
        assert_eq!(
            receipt.path_for_plan(&plan(platform)).unwrap(),
            fs::canonicalize(&executable).expect("canonicalize stub")
        );
        let provenance = receipt.manifest_provenance().expect("provenance recorded");
        assert_eq!(provenance.manifest_digest(), verified.digest());
        assert_eq!(
            provenance.artifact_id(),
            "aaaaaaaa-bbbb-4ccc-8ddd-000000000001"
        );

        // Standalone-verified receipts carry no manifest provenance.
        let standalone =
            MacOsTrustedQemuRuntimeV1::verify(&resources, &executable, &digest, platform)
                .expect("standalone verification");
        assert!(standalone.manifest_provenance().is_none());

        // Swapping the backing file defeats the receipt at re-check time.
        fs::write(&executable, b"swapped stub").expect("swap executable");
        assert!(matches!(
            receipt.path_for_plan(&plan(platform)),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied
        ));

        // Entries that do not describe this platform's canonical executable
        // never become receipts, even inside a perfectly signed envelope.
        let alien = e2e_manifest(|manifest| {
            let artifact = manifest.artifacts.first_mut().expect("single artifact");
            artifact.platform = RuntimeManifestPlatformV1::WindowsX64;
            artifact.guest_architecture =
                crate::computer_use::guest_image::ComputerGuestArchitectureV1::X86_64;
            artifact.bundle_relative_path = "qemu/qemu-system-x86_64.exe".to_owned();
        });
        let verified = e2e_load(&resources, &alien);
        let artifact = verified
            .qemu_executable_for(RuntimeManifestPlatformV1::WindowsX64)
            .expect("windows entry present");
        assert!(
            MacOsTrustedQemuRuntimeV1::from_manifest_artifact(
                &resources,
                verified.digest(),
                &artifact,
            )
            .is_err()
        );

        let inflated = e2e_manifest(|manifest| {
            manifest
                .artifacts
                .first_mut()
                .expect("single artifact")
                .size_bytes = 600 * 1024 * 1024;
        });
        let verified = e2e_load(&resources, &inflated);
        let artifact = verified
            .qemu_executable_for(RuntimeManifestPlatformV1::MacOsIntel)
            .expect("entry present");
        assert!(
            MacOsTrustedQemuRuntimeV1::from_manifest_artifact(
                &resources,
                verified.digest(),
                &artifact,
            )
            .is_err()
        );
        assert!(
            MacOsTrustedQemuRuntimeV1::from_manifest_artifact(
                &resources,
                "not-a-digest",
                &artifact,
            )
            .is_err()
        );
    }
}
