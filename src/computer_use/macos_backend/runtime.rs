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

const MAX_QEMU_BINARY_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MacOsTrustedQemuRuntimeV1 {
    bundle_resources: PathBuf,
    path: PathBuf,
    platform: ComputerHostPlatformV1,
    sha256: String,
}

impl MacOsTrustedQemuRuntimeV1 {
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
        }
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
}
