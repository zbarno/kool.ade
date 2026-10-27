use std::path::{Path, PathBuf};

use core_foundation::url::CFURL;
use security_framework::os::macos::code_signing::{Flags, SecRequirement, SecStaticCode};

use crate::execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure};

use super::super::failure;

const APP_BUNDLE_ID: &str = "com.docker.docker";
const TEAM_ID: &str = "9BNSXJN65R";
// Docker's support repository records this bundle ID in 4.30.0 metadata
// (docker/for-mac#7302) and the same Team ID on a 4.78.0 shipped binary
// (docker/desktop-feedback#471). Verify this pin against current official Mac
// releases before making a platform support claim.

fn app_requirement() -> String {
    format!(
        "anchor apple generic and identifier \"{APP_BUNDLE_ID}\" and certificate leaf[subject.OU] = \"{TEAM_ID}\" and certificate leaf[field.1.2.840.113635.100.6.1.13] exists"
    )
}

/// Verifies the installed bundle as separate setup evidence. Runtime trust
/// still depends on the live `com.docker.backend` socket peer checks.
pub(super) fn verify_installed_docker_desktop() -> Result<(), PrerequisiteFailure> {
    let candidate = app_bundle_candidates()
        .into_iter()
        .find(|path| is_direct_app_bundle(path))
        .ok_or_else(|| {
            failure(
                PrerequisiteCode::DockerMissing,
                "Install Docker Desktop from Docker's official macOS guide",
                false,
            )
        })?;
    verify_bundle(&candidate).map(|_version| ()).map_err(|()| {
        failure(
            PrerequisiteCode::UntrustedEndpoint,
            "Docker Desktop's macOS app signature or publisher is not trusted",
            false,
        )
    })
}

fn app_bundle_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from("/Applications/Docker.app")];
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join("Applications/Docker.app"));
    }
    candidates
}

fn is_direct_app_bundle(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn verify_bundle(path: &Path) -> Result<String, ()> {
    let url = CFURL::from_path(path, true).ok_or(())?;
    let code = SecStaticCode::from_path(&url, Flags::NONE).map_err(|_| ())?;
    let requirement: SecRequirement = app_requirement().parse().map_err(|_| ())?;
    code.check_validity(
        Flags::CHECK_ALL_ARCHITECTURES | Flags::STRICT_VALIDATE,
        &requirement,
    )
    .map_err(|_| ())?;

    let info_path = path.join("Contents/Info.plist");
    let info = plist::Value::from_file(&info_path).map_err(|_| ())?;
    let dictionary = info.as_dictionary().ok_or(())?;
    let bundle_id = dictionary
        .get("CFBundleIdentifier")
        .and_then(plist::Value::as_string)
        .ok_or(())?;
    if bundle_id != APP_BUNDLE_ID {
        return Err(());
    }
    let version = dictionary
        .get("CFBundleShortVersionString")
        .and_then(plist::Value::as_string)
        .filter(|version| {
            !version.is_empty()
                && version.len() <= 64
                && version
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        })
        .ok_or(())?
        .to_owned();

    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirement_pins_the_desktop_bundle_and_developer_id_team() {
        let requirement = app_requirement();
        assert!(requirement.contains("identifier \"com.docker.docker\""));
        assert!(requirement.contains("subject.OU] = \"9BNSXJN65R\""));
        assert!(requirement.contains("1.2.840.113635.100.6.1.13"));
        assert_eq!(TEAM_ID, "9BNSXJN65R");
    }

    #[test]
    fn rejects_symlinked_application_bundle_candidates() {
        let directory = tempfile::tempdir().unwrap();
        let real_app = directory.path().join("Docker.app");
        std::fs::create_dir(&real_app).unwrap();
        let link = directory.path().join("Linked.app");
        std::os::unix::fs::symlink(&real_app, &link).unwrap();
        assert!(is_direct_app_bundle(&real_app));
        assert!(!is_direct_app_bundle(&link));
    }

    #[test]
    fn rejects_an_unsigned_application_bundle() {
        let directory = tempfile::tempdir().unwrap();
        assert!(verify_bundle(directory.path()).is_err());
    }
}
