use std::{fs::File, mem::size_of, os::windows::io::AsRawHandle};

use anyhow::ensure;
use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DEVICE, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO, FILE_STANDARD_INFO,
        FileAttributeTagInfo, FileBasicInfo, FileStandardInfo, GetFileInformationByHandle,
        GetFileInformationByHandleEx,
    },
};

pub(super) use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DEVICE as ATTRIBUTE_DEVICE, FILE_ATTRIBUTE_DIRECTORY as ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT as ATTRIBUTE_REPARSE_POINT,
};

#[derive(Clone, Copy)]
pub(super) struct Metadata {
    pub attributes: FILE_ATTRIBUTE_TAG_INFO,
    pub standard: FILE_STANDARD_INFO,
    pub basic: FILE_BASIC_INFO,
}

pub(super) fn metadata(file: &File) -> anyhow::Result<Metadata> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut value = Metadata {
        attributes: FILE_ATTRIBUTE_TAG_INFO::default(),
        standard: FILE_STANDARD_INFO::default(),
        basic: FILE_BASIC_INFO::default(),
    };
    query(handle, FileAttributeTagInfo, &mut value.attributes)?;
    query(handle, FileStandardInfo, &mut value.standard)?;
    query(handle, FileBasicInfo, &mut value.basic)?;
    Ok(value)
}

fn query<T>(handle: HANDLE, class: i32, value: &mut T) -> anyhow::Result<()> {
    ensure!(
        unsafe {
            GetFileInformationByHandleEx(
                handle,
                class,
                (value as *mut T).cast(),
                size_of::<T>() as u32,
            )
        } != 0,
        "workspace entry metadata could not be inspected"
    );
    Ok(())
}

pub(super) fn handle_identity(file: &File) -> anyhow::Result<(u32, u64)> {
    let mut value = BY_HANDLE_FILE_INFORMATION::default();
    ensure!(
        unsafe { GetFileInformationByHandle(file.as_raw_handle() as HANDLE, &mut value) } != 0,
        "workspace entry identity could not be inspected"
    );
    Ok((
        value.dwVolumeSerialNumber,
        (u64::from(value.nFileIndexHigh) << 32) | u64::from(value.nFileIndexLow),
    ))
}

pub(super) fn verify_identity(file: &File) -> anyhow::Result<()> {
    ensure!(
        handle_identity(file)?.1 != 0,
        "workspace root identity is unavailable"
    );
    Ok(())
}

pub(super) fn verify_directory(file: &File) -> anyhow::Result<()> {
    ensure_directory(&metadata(file)?)
}

pub(super) fn ensure_not_reparse(metadata: &Metadata) -> anyhow::Result<()> {
    ensure!(
        metadata.attributes.FileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DEVICE)
            == 0,
        "workspace contains a reparse point or special file"
    );
    Ok(())
}

pub(super) fn ensure_directory(metadata: &Metadata) -> anyhow::Result<()> {
    ensure_not_reparse(metadata)?;
    ensure!(
        metadata.standard.Directory
            && metadata.attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0
            && !metadata.standard.DeletePending,
        "workspace path is not a stable directory"
    );
    Ok(())
}

pub(super) fn ensure_stable_file(
    before: &Metadata,
    after: &Metadata,
    before_id: (u32, u64),
    after_id: (u32, u64),
) -> anyhow::Result<()> {
    ensure_not_reparse(after)?;
    ensure!(
        before_id == after_id
            && !before.standard.Directory
            && !after.standard.Directory
            && before.standard.EndOfFile == after.standard.EndOfFile
            && before.standard.NumberOfLinks == after.standard.NumberOfLinks
            && after.standard.NumberOfLinks == 1
            && before.basic.LastWriteTime == after.basic.LastWriteTime
            && before.basic.ChangeTime == after.basic.ChangeTime
            && before.basic.FileAttributes == after.basic.FileAttributes,
        "workspace file changed while staging"
    );
    Ok(())
}

pub(super) fn ensure_stable_directory(
    before: &Metadata,
    after: &Metadata,
    before_id: (u32, u64),
    after_id: (u32, u64),
) -> anyhow::Result<()> {
    ensure_directory(after)?;
    ensure!(
        before_id == after_id
            && before.standard.NumberOfLinks == after.standard.NumberOfLinks
            && before.basic.LastWriteTime == after.basic.LastWriteTime
            && before.basic.ChangeTime == after.basic.ChangeTime
            && before.basic.FileAttributes == after.basic.FileAttributes,
        "workspace directory changed while staging"
    );
    Ok(())
}
