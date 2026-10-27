use std::{fs::File, mem::size_of, os::windows::io::AsRawHandle};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO, FILE_ID_INFO,
    FILE_STANDARD_INFO, FileAttributeTagInfo, FileBasicInfo, FileIdInfo, FileStandardInfo,
    GetFileInformationByHandleEx,
};

pub(super) fn verify_directory(file: &File) -> Result<(), String> {
    let (attributes, standard, _) = metadata(file)?;
    if !standard.Directory || attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("Workspace path traverses a non-directory or reparse point".into());
    }
    Ok(())
}

pub(super) fn metadata(
    file: &File,
) -> Result<(FILE_ATTRIBUTE_TAG_INFO, FILE_STANDARD_INFO, FILE_BASIC_INFO), String> {
    let handle = file.as_raw_handle();
    let mut attributes = FILE_ATTRIBUTE_TAG_INFO::default();
    let mut standard = FILE_STANDARD_INFO::default();
    let mut basic = FILE_BASIC_INFO::default();
    let attributes_size = u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>())
        .map_err(|_| "Workspace entry metadata size is unsupported")?;
    let standard_size = u32::try_from(size_of::<FILE_STANDARD_INFO>())
        .map_err(|_| "Workspace entry metadata size is unsupported")?;
    let basic_size = u32::try_from(size_of::<FILE_BASIC_INFO>())
        .map_err(|_| "Workspace entry metadata size is unsupported")?;
    let attributes_ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileAttributeTagInfo,
            (&mut attributes as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            attributes_size,
        )
    };
    let standard_ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            standard_size,
        )
    };
    let basic_ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileBasicInfo,
            (&mut basic as *mut FILE_BASIC_INFO).cast(),
            basic_size,
        )
    };
    if attributes_ok == 0 || standard_ok == 0 || basic_ok == 0 {
        return Err("Workspace entry metadata could not be inspected safely".into());
    }
    Ok((attributes, standard, basic))
}

pub(super) fn file_id(file: &File) -> Result<([u8; 16], u64), String> {
    let mut info = FILE_ID_INFO::default();
    let info_size = u32::try_from(size_of::<FILE_ID_INFO>())
        .map_err(|_| "Workspace entry identity size is unsupported")?;
    let result = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            info_size,
        )
    };
    if result == 0 {
        return Err("Workspace entry identity could not be verified".into());
    }
    Ok((info.FileId.Identifier, info.VolumeSerialNumber))
}
