use std::{
    fs::File,
    mem::{offset_of, size_of},
    os::windows::io::{AsRawHandle, FromRawHandle},
    ptr,
};

use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE as WDK_FILE_CREATE, FILE_DIRECTORY_FILE as WDK_FILE_DIRECTORY_FILE,
            FILE_NON_DIRECTORY_FILE as WDK_FILE_NON_DIRECTORY_FILE, FILE_OPEN as WDK_FILE_OPEN,
            FILE_OPEN_REPARSE_POINT as WDK_FILE_OPEN_REPARSE_POINT,
            FILE_SYNCHRONOUS_IO_NONALERT as WDK_FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::{
        Foundation::{OBJ_CASE_INSENSITIVE, UNICODE_STRING},
        Storage::FileSystem::{
            DELETE as W_DELETE, FILE_APPEND_DATA as W_APPEND_DATA, FILE_ATTRIBUTE_DIRECTORY,
            FILE_ATTRIBUTE_NORMAL, FILE_BASIC_INFO, FILE_DISPOSITION_FLAG_DELETE,
            FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_INFO_EX,
            FILE_READ_ATTRIBUTES as W_READ_ATTRIBUTES, FILE_READ_DATA as W_READ_DATA,
            FILE_RENAME_INFO, FILE_RENAME_INFO_0, FILE_SHARE_READ, FILE_SHARE_WRITE,
            FILE_TRAVERSE as W_FILE_TRAVERSE, FILE_WRITE_ATTRIBUTES as W_WRITE_ATTRIBUTES,
            FILE_WRITE_DATA as W_WRITE_DATA, FileBasicInfo, FileDispositionInfoEx, FileRenameInfo,
            SYNCHRONIZE as W_SYNCHRONIZE, SetFileInformationByHandle,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

pub(super) const STATUS_OBJECT_NAME_NOT_FOUND: i32 = -1_073_741_772;
pub(super) const STATUS_OBJECT_NAME_COLLISION: i32 = -1_073_741_771;
pub(super) const STATUS_OBJECT_PATH_NOT_FOUND: i32 = -1_073_741_766;
pub(super) const SHARE_READ: u32 = FILE_SHARE_READ;
pub(super) const SHARE_WRITE: u32 = FILE_SHARE_WRITE;
pub(super) const READ_DATA: u32 = W_READ_DATA;
pub(super) const WRITE_DATA: u32 = W_WRITE_DATA;
pub(super) const WRITE_ATTRIBUTES: u32 = W_WRITE_ATTRIBUTES;
pub(super) const APPEND_DATA: u32 = W_APPEND_DATA;
pub(super) const READ_ATTRIBUTES: u32 = W_READ_ATTRIBUTES;
pub(super) const TRAVERSE: u32 = W_FILE_TRAVERSE;
pub(super) const DELETE: u32 = W_DELETE;
pub(super) const SYNCHRONIZE: u32 = W_SYNCHRONIZE;
pub(super) const DIRECTORY_FILE: u32 = WDK_FILE_DIRECTORY_FILE;
pub(super) const NON_DIRECTORY_FILE: u32 = WDK_FILE_NON_DIRECTORY_FILE;
pub(super) const SYNCHRONOUS_IO_NONALERT: u32 = WDK_FILE_SYNCHRONOUS_IO_NONALERT;
pub(super) const OPEN_REPARSE_POINT: u32 = WDK_FILE_OPEN_REPARSE_POINT;
pub(super) const FILE_OPEN: u32 = WDK_FILE_OPEN;
pub(super) const FILE_CREATE: u32 = WDK_FILE_CREATE;
pub(super) const ATTRIBUTE_NORMAL: u32 = FILE_ATTRIBUTE_NORMAL;
pub(super) const ATTRIBUTE_DIRECTORY: u32 = FILE_ATTRIBUTE_DIRECTORY;

pub(super) fn open_at(
    parent: &File,
    name: &mut [u16],
    desired_access: u32,
    share_access: u32,
    disposition: u32,
    create_options: u32,
    file_attributes: u32,
) -> Result<File, i32> {
    let byte_length = name.len().checked_mul(size_of::<u16>()).ok_or(-1)?;
    if byte_length == 0 || byte_length > u16::MAX as usize {
        return Err(-1);
    }
    let byte_length = u16::try_from(byte_length).map_err(|_| -1)?;
    let unicode = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: name.as_mut_ptr(),
    };
    let mut object_attributes = OBJECT_ATTRIBUTES::default();
    object_attributes.Length = u32::try_from(size_of::<OBJECT_ATTRIBUTES>()).map_err(|_| -1)?;
    object_attributes.RootDirectory = parent.as_raw_handle();
    object_attributes.ObjectName = &unicode;
    object_attributes.Attributes = OBJ_CASE_INSENSITIVE;
    let mut io_status = IO_STATUS_BLOCK::default();
    let mut handle = ptr::null_mut();
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            desired_access,
            &object_attributes,
            &mut io_status,
            None,
            file_attributes,
            share_access,
            disposition,
            create_options,
            None,
            0,
        )
    };
    if status < 0 {
        return Err(status);
    }
    if handle.is_null() {
        return Err(-1);
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

pub(super) fn rename_noreplace(file: &File, parent: &File, name: &str) -> Result<(), ()> {
    let encoded = name.encode_utf16().collect::<Vec<_>>();
    let name_bytes = encoded.len().checked_mul(size_of::<u16>()).ok_or(())?;
    let length = offset_of!(FILE_RENAME_INFO, FileName)
        .checked_add(name_bytes)
        .ok_or(())?;
    let information_length = u32::try_from(length).map_err(|_| ())?;
    let file_name_length = u32::try_from(name_bytes).map_err(|_| ())?;
    let mut storage = vec![0_usize; length.div_ceil(size_of::<usize>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        ptr::write(
            info,
            FILE_RENAME_INFO {
                Anonymous: FILE_RENAME_INFO_0 {
                    ReplaceIfExists: false,
                },
                RootDirectory: parent.as_raw_handle(),
                FileNameLength: file_name_length,
                FileName: [0],
            },
        );
        ptr::copy_nonoverlapping(
            encoded.as_ptr(),
            (info
                .cast::<u8>()
                .add(offset_of!(FILE_RENAME_INFO, FileName)))
            .cast::<u16>(),
            encoded.len(),
        );
    }
    let result = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfo,
            info.cast(),
            information_length,
        )
    };
    if result == 0 { Err(()) } else { Ok(()) }
}

pub(super) fn delete_file(file: &File) -> Result<(), ()> {
    let info_size = u32::try_from(size_of::<FILE_DISPOSITION_INFO_EX>()).map_err(|_| ())?;
    let mut disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    let result = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfoEx,
            (&mut disposition as *mut FILE_DISPOSITION_INFO_EX).cast(),
            info_size,
        )
    };
    if result == 0 { Err(()) } else { Ok(()) }
}

pub(super) fn set_readonly(file: &File, readonly: bool) -> Result<(), ()> {
    let info_size = u32::try_from(size_of::<FILE_BASIC_INFO>()).map_err(|_| ())?;
    let mut info = FILE_BASIC_INFO {
        CreationTime: -1,
        LastAccessTime: -1,
        LastWriteTime: -1,
        ChangeTime: -1,
        FileAttributes: if readonly { 1 } else { ATTRIBUTE_NORMAL },
    };
    let result = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileBasicInfo,
            (&mut info as *mut FILE_BASIC_INFO).cast(),
            info_size,
        )
    };
    if result == 0 { Err(()) } else { Ok(()) }
}
