use std::{
    ffi::OsStr,
    fs,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::computer_use::{guest_image::QemuImageToolV1, personal_storage::PersonalVmDiskKeyV1};

use super::process::{run_qemu, run_qemu_with_secret_fd};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PersonalDiskErrorV1 {
    UnsafePath,
    AlreadyExists,
    QemuFailed,
    Io,
}

/// Converts the verified signed base image into a self-contained LUKS qcow2.
/// The key is exposed only through an inherited sealed memfd, never argv or disk.
pub(crate) fn create_encrypted_personal_disk(
    tool: &QemuImageToolV1,
    source_image: &Path,
    destination: &Path,
    key: &PersonalVmDiskKeyV1,
    max_disk_bytes: u64,
) -> Result<(), PersonalDiskErrorV1> {
    validate_source(source_image)?;
    let parent = destination
        .parent()
        .ok_or(PersonalDiskErrorV1::UnsafePath)?;
    validate_private_directory(parent)?;
    match fs::symlink_metadata(destination) {
        Ok(_) => return Err(PersonalDiskErrorV1::AlreadyExists),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(PersonalDiskErrorV1::Io),
    }
    let temp_path = temp_path(parent);
    let mut cleanup = TempDiskCleanup {
        path: temp_path.clone(),
        keep: false,
    };
    let secret = key
        .as_qemu_secret_fd()
        .map_err(|_| PersonalDiskErrorV1::QemuFailed)?;
    let secret_arg = format!(
        "secret,id=personal_vm_key,file=/proc/self/fd/{}",
        secret.as_raw_fd()
    );
    let options = "encrypt.format=luks,encrypt.key-secret=personal_vm_key";
    let args = [
        OsStr::new("convert"),
        OsStr::new("--object"),
        OsStr::new(&secret_arg),
        OsStr::new("-f"),
        OsStr::new("qcow2"),
        OsStr::new("-O"),
        OsStr::new("qcow2"),
        OsStr::new("-o"),
        OsStr::new(options),
        OsStr::new("-m"),
        OsStr::new("1"),
        source_image.as_os_str(),
        temp_path.as_os_str(),
    ];
    let result = run_qemu_with_secret_fd(
        tool,
        &args,
        secret.as_raw_fd(),
        max_disk_bytes.saturating_add(128 * 1024 * 1024),
    );
    if !result.is_ok_and(|output| output.status.success()) {
        return Err(PersonalDiskErrorV1::QemuFailed);
    }
    verify_encrypted_disk(tool, &temp_path, max_disk_bytes)?;
    set_private_file(&temp_path)?;
    fs::rename(&temp_path, destination).map_err(|_| PersonalDiskErrorV1::Io)?;
    cleanup.keep = true;
    FileSync::sync_parent(parent)?;
    Ok(())
}

#[derive(Deserialize)]
struct PersonalDiskInfo {
    format: String,
    #[serde(rename = "virtual-size")]
    virtual_size: u64,
    #[serde(rename = "backing-filename")]
    backing_filename: Option<String>,
    encrypted: Option<bool>,
    #[serde(rename = "format-specific")]
    format_specific: Option<PersonalDiskFormatSpecific>,
}

#[derive(Deserialize)]
struct PersonalDiskFormatSpecific {
    #[serde(rename = "type")]
    format_type: String,
    data: PersonalDiskFormatData,
}

#[derive(Deserialize)]
struct PersonalDiskFormatData {
    encrypt: Option<PersonalDiskEncryption>,
}

#[derive(Deserialize)]
struct PersonalDiskEncryption {
    format: String,
}

fn verify_encrypted_disk(
    tool: &QemuImageToolV1,
    path: &Path,
    expected_virtual_size: u64,
) -> Result<(), PersonalDiskErrorV1> {
    let output = run_qemu(
        tool,
        &[
            OsStr::new("info"),
            OsStr::new("--output=json"),
            path.as_os_str(),
        ],
    )
    .map_err(|_| PersonalDiskErrorV1::QemuFailed)?;
    if !output.status.success() {
        return Err(PersonalDiskErrorV1::QemuFailed);
    }
    let info: PersonalDiskInfo =
        serde_json::from_slice(&output.stdout).map_err(|_| PersonalDiskErrorV1::QemuFailed)?;
    validate_disk_info(&info, expected_virtual_size)
}

fn validate_disk_info(
    info: &PersonalDiskInfo,
    expected_virtual_size: u64,
) -> Result<(), PersonalDiskErrorV1> {
    if info.format != "qcow2"
        || info.virtual_size != expected_virtual_size
        || info.backing_filename.is_some()
        || info.encrypted != Some(true)
        || info.format_specific.as_ref().is_none_or(|specific| {
            specific.format_type != "qcow2"
                || specific
                    .data
                    .encrypt
                    .as_ref()
                    .is_none_or(|encryption| encryption.format != "luks")
        })
    {
        return Err(PersonalDiskErrorV1::QemuFailed);
    }
    Ok(())
}

fn validate_source(path: &Path) -> Result<(), PersonalDiskErrorV1> {
    let metadata = fs::symlink_metadata(path).map_err(|_| PersonalDiskErrorV1::UnsafePath)?;
    if !metadata.file_type().is_file() || !path.is_absolute() {
        return Err(PersonalDiskErrorV1::UnsafePath);
    }
    Ok(())
}

fn validate_private_directory(path: &Path) -> Result<(), PersonalDiskErrorV1> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = fs::symlink_metadata(path).map_err(|_| PersonalDiskErrorV1::UnsafePath)?;
    if !metadata.file_type().is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(PersonalDiskErrorV1::UnsafePath);
    }
    Ok(())
}

fn set_private_file(path: &Path) -> Result<(), PersonalDiskErrorV1> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = fs::symlink_metadata(path).map_err(|_| PersonalDiskErrorV1::UnsafePath)?;
    if !metadata.file_type().is_file() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(PersonalDiskErrorV1::UnsafePath);
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|_| PersonalDiskErrorV1::Io)
}

fn temp_path(parent: &Path) -> PathBuf {
    parent.join(format!(".personal-{}.tmp.qcow2", uuid::Uuid::new_v4()))
}

struct FileSync;

struct TempDiskCleanup {
    path: PathBuf,
    keep: bool,
}

impl Drop for TempDiskCleanup {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl FileSync {
    fn sync_parent(path: &Path) -> Result<(), PersonalDiskErrorV1> {
        fs::File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| PersonalDiskErrorV1::Io)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    use crate::computer_use::{
        guest_image::QemuImageToolV1, personal_storage::PersonalVmDiskKeyV1,
    };

    use super::{
        PersonalDiskErrorV1, PersonalDiskInfo, create_encrypted_personal_disk, validate_disk_info,
    };

    #[test]
    fn encrypted_personal_disk_requires_self_contained_luks_qcow2() {
        let luks: PersonalDiskInfo = serde_json::from_str(
            r#"{"format":"qcow2","virtual-size":2147483648,"encrypted":true,"format-specific":{"type":"qcow2","data":{"encrypt":{"format":"luks"}}}}"#,
        )
        .unwrap();
        assert_eq!(validate_disk_info(&luks, 2 * 1024 * 1024 * 1024), Ok(()));

        let aes: PersonalDiskInfo = serde_json::from_str(
            r#"{"format":"qcow2","virtual-size":2147483648,"encrypted":true,"format-specific":{"type":"qcow2","data":{"encrypt":{"format":"aes"}}}}"#,
        )
        .unwrap();
        assert_eq!(
            validate_disk_info(&aes, 2 * 1024 * 1024 * 1024),
            Err(PersonalDiskErrorV1::QemuFailed)
        );
    }

    #[test]
    fn disk_conversion_uses_secret_fd_and_keeps_key_out_of_arguments() {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let source = temp.path().join("signed-base.qcow2");
        let destination = temp.path().join("profile.qcow2");
        fs::write(&source, b"verified source fixture").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
        let args_log = temp.path().join("args.txt");
        let script = temp.path().join("qemu-img-fixture");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"convert\" ]; then\n  printf '%s\\n' \"$@\" > '{}';\n  for arg do output=\"$arg\"; done\n  printf 'fixture ciphertext' > \"$output\"\nelse\n  printf '{{\"format\":\"qcow2\",\"virtual-size\":2147483648,\"encrypted\":true,\"format-specific\":{{\"type\":\"qcow2\",\"data\":{{\"encrypt\":{{\"format\":\"luks\"}}}}}}}}'\nfi\n",
                args_log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        let key = PersonalVmDiskKeyV1::for_test([0x5a; 32]);

        create_encrypted_personal_disk(
            &QemuImageToolV1::from_test_path(script),
            &source,
            &destination,
            &key,
            2 * 1024 * 1024 * 1024,
        )
        .unwrap();

        let args = fs::read_to_string(args_log).unwrap();
        assert!(args.contains("secret,id=personal_vm_key,file=/proc/self/fd/"));
        assert!(args.contains("encrypt.format=luks,encrypt.key-secret=personal_vm_key"));
        assert!(!args.contains(&"5a".repeat(32)));
        assert!(destination.is_file());
        assert_eq!(
            fs::symlink_metadata(&destination)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(Path::new(&destination).is_absolute());
    }
}
