use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    process::Command,
};

use crate::computer_use::apps::GuestAppHelperConfigV1;
use crate::computer_use::apps::{
    GuestAppCapabilityV1, GuestApplicationCatalogTrustV1, GuestApplicationCatalogV1,
    VerifiedGuestApplicationCatalogV1,
};

use super::super::protocol::ReplyV1;

const HELPER_CONFIG: &str =
    "/sys/firmware/qemu_fw_cfg/by_name/opt/org.koolade.computer-use/app-helper-config/raw";
const APP_CATALOG: &str =
    "/sys/firmware/qemu_fw_cfg/by_name/opt/org.koolade.computer-use/app-catalog/raw";
const BOOT_MARKER: &str = "/var/lib/koolade-app-helper/boot-state.json";
const MAX_CONFIG_BYTES: u64 = 4096;
const MAX_CATALOG_BYTES: u64 = 256 * 1024;

pub(super) fn begin_guest_helper_boot() -> Result<(), &'static str> {
    let config = helper_config()?;
    write_boot_marker(Path::new(BOOT_MARKER), &config)
}

fn write_boot_marker(marker: &Path, config: &GuestAppHelperConfigV1) -> Result<(), &'static str> {
    if fs::symlink_metadata(marker).is_ok() {
        return Err("app helper cannot resume an existing guest session");
    }
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options
        .open(marker)
        .map_err(|_| "app helper boot marker unavailable")?;
    let bytes = serde_json::to_vec(&config).map_err(|_| "app helper context is invalid")?;
    file.write_all(&bytes)
        .map_err(|_| "app helper boot marker cannot be written")?;
    file.sync_all()
        .map_err(|_| "app helper boot marker cannot be synced")
}

pub(super) fn helper_config() -> Result<GuestAppHelperConfigV1, &'static str> {
    let bytes = read_fw_cfg(HELPER_CONFIG, MAX_CONFIG_BYTES)?;
    let config: GuestAppHelperConfigV1 =
        serde_json::from_slice(&bytes).map_err(|_| "app helper context is malformed")?;
    config.validate()?;
    Ok(config)
}

pub(super) fn verified_catalog() -> Result<VerifiedGuestApplicationCatalogV1, &'static str> {
    let config = helper_config()?;
    let bytes = read_fw_cfg(APP_CATALOG, MAX_CATALOG_BYTES)?;
    let catalog: GuestApplicationCatalogV1 =
        serde_json::from_slice(&bytes).map_err(|_| "app catalog is malformed")?;
    let trust = GuestApplicationCatalogTrustV1::application_pinned()?;
    let verified = VerifiedGuestApplicationCatalogV1::verify(catalog, &trust)?;
    if verified.catalog().image_digest != config.image_digest
        || verified.catalog().architecture
            != crate::computer_use::apps::GuestAppArchitectureV1::X86_64
    {
        return Err("app catalog does not match the active guest image");
    }
    Ok(verified)
}

pub(super) fn status() -> ReplyV1 {
    let Ok(config) = helper_config() else {
        return ReplyV1::Status {
            app_management: false,
            staged_task_app_launch: super::task_stage::available(),
            installed_apps: Vec::new(),
        };
    };
    let Ok(catalog) = verified_catalog() else {
        return ReplyV1::Status {
            app_management: false,
            staged_task_app_launch: super::task_stage::available(),
            installed_apps: Vec::new(),
        };
    };
    let mut installed_apps = Vec::new();
    let mut package_available = false;
    for manifest in &catalog.catalog().apps {
        if !config.allowed_apps.contains(&manifest.app_id)
            || !manifest
                .required_capabilities
                .iter()
                .all(|cap| *cap == GuestAppCapabilityV1::GraphicalDisplay)
        {
            continue;
        }
        if super::package::cached_package_is_valid(&manifest.package.package_sha256) {
            package_available = true;
        }
        if package_installed(&manifest.package.package_id, &manifest.package.version)
            || manifest
                .package
                .rollback_version
                .as_deref()
                .is_some_and(|version| package_installed(&manifest.package.package_id, version))
        {
            installed_apps.push(manifest.app_id.clone());
        }
    }
    ReplyV1::Status {
        app_management: package_available,
        staged_task_app_launch: super::task_stage::available(),
        installed_apps,
    }
}

pub(super) fn read_fw_cfg(path: &str, max_bytes: u64) -> Result<Vec<u8>, &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "app helper fw_cfg input unavailable")?;
    if !metadata.file_type().is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
        || metadata.len() == 0
        || metadata.len() > max_bytes
    {
        return Err("app helper fw_cfg input is unsafe or oversized");
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    options
        .open(path)
        .and_then(|file| file.take(max_bytes + 1).read_to_end(&mut bytes))
        .map_err(|_| "app helper fw_cfg input cannot be read")?;
    if bytes.is_empty() || bytes.len() as u64 > max_bytes {
        return Err("app helper fw_cfg input exceeds its limit");
    }
    Ok(bytes)
}

fn package_installed(package_id: &str, version: &str) -> bool {
    let Ok(output) = Command::new("/usr/bin/dpkg-query")
        .args(["-W", "-f=${Status}\n${Version}", package_id])
        .env_clear()
        .output()
    else {
        return false;
    };
    output.status.success()
        && output.stdout == format!("install ok installed\n{version}").as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_restart_denies_reusing_the_session_boot_marker() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("boot-state.json");
        let config = GuestAppHelperConfigV1 {
            schema_version: 1,
            session_id: "00000000-0000-4000-8000-000000000001".into(),
            worker_id: "00000000-0000-4000-8000-000000000002".into(),
            image_digest: format!("sha256:{}", "a".repeat(64)),
            session_class: crate::computer_use::ComputerSessionClassV1::Disposable,
            lease_generation: 1,
            approval_generation: 1,
            cancellation_generation: 1,
            initial_action_sequence: 0,
            expires_at_unix_ms: 60_000,
            boot_id: "00000000-0000-4000-8000-000000000003".into(),
            allowed_apps: vec!["text-editor".into()],
        };
        assert!(write_boot_marker(&marker, &config).is_ok());
        assert!(write_boot_marker(&marker, &config).is_err());
    }
}
