use std::{
    fs::{File, Metadata},
    path::Path,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct WorkspaceObjectId {
    volume: u64,
    index: u64,
}

#[cfg(unix)]
pub(super) fn from_metadata(metadata: &Metadata, _path: &Path) -> Option<WorkspaceObjectId> {
    use std::os::unix::fs::MetadataExt;

    Some(WorkspaceObjectId {
        volume: metadata.dev(),
        index: metadata.ino(),
    })
}

#[cfg(not(unix))]
pub(super) fn from_metadata(_metadata: &Metadata, path: &Path) -> Option<WorkspaceObjectId> {
    #[cfg(windows)]
    {
        windows::from_path(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

#[cfg(windows)]
pub(super) fn from_handle(file: &File) -> Option<WorkspaceObjectId> {
    windows::from_handle(file)
}

pub(super) fn is_reparse_point(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(unix)]
pub(super) fn matches_file_identity(file: &File, expected: &WorkspaceObjectId) -> bool {
    use std::path::Path;

    file.metadata()
        .ok()
        .and_then(|metadata| from_metadata(&metadata, Path::new(".")))
        .as_ref()
        == Some(expected)
}

#[cfg(all(test, windows))]
#[path = "workspace_identity/tests.rs"]
mod tests;

#[cfg(windows)]
mod windows {
    use super::WorkspaceObjectId;
    use std::{
        fs::{File, OpenOptions},
        os::windows::fs::OpenOptionsExt,
        path::Path,
    };
    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, GetFileInformationByHandle,
        },
    };

    pub(super) fn from_path(path: &Path) -> Option<WorkspaceObjectId> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .ok()?;
        from_handle(&file)
    }

    pub(super) fn from_handle(file: &File) -> Option<WorkspaceObjectId> {
        let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
        if unsafe {
            GetFileInformationByHandle(
                std::os::windows::io::AsRawHandle::as_raw_handle(file) as HANDLE,
                information.as_mut_ptr(),
            )
        } == 0
        {
            return None;
        }
        let information = unsafe { information.assume_init() };
        if information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
            || information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        {
            return None;
        }
        Some(WorkspaceObjectId {
            volume: u64::from(information.dwVolumeSerialNumber),
            index: (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow),
        })
    }
}
