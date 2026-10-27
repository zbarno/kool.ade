use std::{
    fs::File,
    mem::{offset_of, size_of},
    os::windows::io::AsRawHandle,
    path::Path,
};

use anyhow::{Context, bail, ensure};
use windows_sys::Win32::{
    Foundation::HANDLE,
    Storage::FileSystem::{
        FILE_ID_BOTH_DIR_INFO, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
        GetFileInformationByHandleEx,
    },
};

use super::super::super::windows_paths;

const BUFFER_BYTES: usize = 64 * 1024;
const ERROR_NO_MORE_FILES: i32 = 18;

pub(super) struct Child {
    pub name: String,
    pub file_id: u64,
    pub attributes: u32,
}

pub(super) fn read_children(directory: &File) -> anyhow::Result<Vec<Child>> {
    let mut children = Vec::new();
    let mut buffer = vec![0_u64; BUFFER_BYTES / size_of::<u64>()];
    let mut first = true;
    loop {
        let class = if first {
            FileIdBothDirectoryRestartInfo
        } else {
            FileIdBothDirectoryInfo
        };
        first = false;
        let result = unsafe {
            GetFileInformationByHandleEx(
                directory.as_raw_handle() as HANDLE,
                class,
                buffer.as_mut_ptr().cast(),
                BUFFER_BYTES as u32,
            )
        };
        if result == 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(ERROR_NO_MORE_FILES) {
                break;
            }
            bail!("workspace directory could not be enumerated safely");
        }
        let bytes =
            unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), BUFFER_BYTES) };
        let mut offset = 0;
        let mut count = 0;
        loop {
            ensure!(
                offset + size_of::<FILE_ID_BOTH_DIR_INFO>() <= bytes.len(),
                "workspace enumeration data is malformed"
            );
            let entry = unsafe { &*bytes.as_ptr().add(offset).cast::<FILE_ID_BOTH_DIR_INFO>() };
            let name_length = entry.FileNameLength as usize;
            let name_offset = offset + offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
            let end = if entry.NextEntryOffset == 0 {
                bytes.len()
            } else {
                offset
                    .checked_add(entry.NextEntryOffset as usize)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(|| {
                        anyhow::anyhow!("workspace enumeration offset is out of bounds")
                    })?
            };
            ensure!(
                entry.FileNameLength % 2 == 0
                    && name_offset <= end
                    && name_length <= end - name_offset,
                "workspace enumeration name is malformed"
            );
            let wide = unsafe {
                std::slice::from_raw_parts(
                    bytes.as_ptr().add(name_offset).cast::<u16>(),
                    name_length / 2,
                )
            };
            let name = String::from_utf16(wide).context("workspace name is not valid Unicode")?;
            if name != "." && name != ".." {
                ensure!(
                    !name.contains('/') && !name.contains('\\'),
                    "workspace entry is not a single path component"
                );
                windows_paths::validate_path(Path::new(&name))?;
                children.push(Child {
                    name,
                    file_id: entry.FileId as u64,
                    attributes: entry.FileAttributes,
                });
            }
            count += 1;
            if entry.NextEntryOffset == 0 {
                break;
            }
            ensure!(
                entry.NextEntryOffset as usize >= size_of::<FILE_ID_BOTH_DIR_INFO>()
                    && entry.NextEntryOffset % 8 == 0,
                "workspace enumeration offset is malformed"
            );
            offset = offset
                .checked_add(entry.NextEntryOffset as usize)
                .filter(|next| *next < bytes.len())
                .ok_or_else(|| anyhow::anyhow!("workspace enumeration offset is out of bounds"))?;
        }
        if count == 0 {
            break;
        }
    }
    children.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(children)
}
