use std::{
    fs::{self, File, OpenOptions},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
};

use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    GetFileInformationByHandle, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

use super::super::MAX_AUDIT_BYTES;

pub(super) fn open_directory(path: &Path) -> anyhow::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    Ok(options.open(path)?)
}

pub(super) fn open_or_create_file(path: &Path) -> anyhow::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    Ok(options.open(path)?)
}

pub(super) fn open_existing_file(path: &Path) -> anyhow::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    Ok(options.open(path)?)
}

pub(super) fn open_existing_audit(path: &Path) -> anyhow::Result<Option<File>> {
    match open_existing_file(path) {
        Ok(file) => {
            verify_regular_file(path, &file)?;
            anyhow::ensure!(
                file.metadata()?.len() <= MAX_AUDIT_BYTES,
                "Machine repair audit is oversized"
            );
            Ok(Some(file))
        }
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn open_new_file(path: &Path) -> anyhow::Result<File> {
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    Ok(options.open(path)?)
}

pub(super) fn verify_directory(path: &Path, opened: &File) -> anyhow::Result<()> {
    verify_no_reparse_ancestors(path)?;
    let named = fs::symlink_metadata(path)?;
    let held = file_information(opened)?;
    let current = open_directory(path)?;
    let current = file_information(&current)?;
    anyhow::ensure!(
        named.is_dir()
            && !is_reparse(&named)
            && held.dwFileAttributes
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
                != 0
            && current.dwFileAttributes
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
                != 0
            && !is_reparse_info(&held)
            && !is_reparse_info(&current)
            && same_file(&current, &held),
        "Repair audit directory identity changed or is unsafe"
    );
    crate::persistence::setup_tasks::verify_private_acl_handle(opened.as_raw_handle())
}

pub(super) fn verify_regular_file(path: &Path, opened: &File) -> anyhow::Result<()> {
    verify_no_reparse_ancestors(path)?;
    let named = fs::symlink_metadata(path)?;
    let held = file_information(opened)?;
    let current = open_existing_file(path)?;
    let current = file_information(&current)?;
    anyhow::ensure!(
        named.is_file()
            && !is_reparse(&named)
            && held.dwFileAttributes
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
                == 0
            && current.dwFileAttributes
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
                == 0
            && !is_reparse_info(&held)
            && !is_reparse_info(&current)
            && held.nNumberOfLinks == 1
            && current.nNumberOfLinks == 1
            && same_file(&current, &held),
        "Machine repair audit file must be private, regular, and singly linked"
    );
    crate::persistence::setup_tasks::verify_private_acl_handle(opened.as_raw_handle())
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes()
        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
}

pub(super) fn verify_no_reparse_ancestors(path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        path.is_absolute(),
        "Machine repair audit path must be absolute"
    );
    let mut current = std::path::PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        anyhow::ensure!(
            metadata.is_dir() && !is_reparse(&metadata),
            "Machine repair audit path contains a non-directory or reparse-point ancestor"
        );
        let opened = open_directory(&current)?;
        let info = file_information(&opened)?;
        anyhow::ensure!(
            !is_reparse_info(&info)
                && info.dwFileAttributes
                    & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY
                    != 0,
            "Machine repair audit path contains an unsafe directory ancestor"
        );
        crate::persistence::setup_tasks::verify_no_untrusted_delete_rights(opened.as_raw_handle())?;
    }
    Ok(())
}

fn file_information(file: &File) -> anyhow::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    anyhow::ensure!(
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, info.as_mut_ptr()) } != 0,
        "Machine repair audit file identity could not be inspected"
    );
    Ok(unsafe { info.assume_init() })
}

fn is_reparse_info(info: &BY_HANDLE_FILE_INFORMATION) -> bool {
    info.dwFileAttributes & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
}

fn same_file(left: &BY_HANDLE_FILE_INFORMATION, right: &BY_HANDLE_FILE_INFORMATION) -> bool {
    left.dwVolumeSerialNumber == right.dwVolumeSerialNumber
        && left.nFileIndexHigh == right.nFileIndexHigh
        && left.nFileIndexLow == right.nFileIndexLow
}

pub(super) fn atomic_replace(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let source = wide_path(source)?;
    let destination = wide_path(destination)?;
    anyhow::ensure!(
        unsafe {
            MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } != 0,
        "Machine repair audit could not be replaced atomically"
    );
    Ok(())
}

fn wide_path(path: &Path) -> anyhow::Result<Vec<u16>> {
    let mut value = path.as_os_str().encode_wide().collect::<Vec<_>>();
    anyhow::ensure!(!value.contains(&0), "Machine repair audit path is invalid");
    value.push(0);
    Ok(value)
}
