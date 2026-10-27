use std::{
    fs::{File, OpenOptions},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Component, Path},
};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
};

use super::{PinnedDirectory, metadata::verify_directory};
use crate::execution::docker::sandbox::staging::archive::validate_relative;
use crate::execution::docker::sandbox::staging::sync::windows::native::{
    APPEND_DATA, ATTRIBUTE_DIRECTORY, DIRECTORY_FILE, FILE_CREATE, FILE_OPEN, OPEN_REPARSE_POINT,
    READ_ATTRIBUTES, SHARE_READ, SHARE_WRITE, STATUS_OBJECT_NAME_NOT_FOUND,
    STATUS_OBJECT_PATH_NOT_FOUND, SYNCHRONIZE, SYNCHRONOUS_IO_NONALERT, TRAVERSE, WRITE_DATA,
    open_at,
};
use crate::execution::docker::sandbox::workspace_identity::WorkspaceObjectId;

const SHARE_DIRECTORIES: u32 = SHARE_READ | SHARE_WRITE;
const MISSING_DIRECTORY: &str = "missing-directory";

pub(in crate::execution::docker::sandbox::staging::sync::windows) fn open_root_nofollow(
    path: &Path,
    expected_identity: &WorkspaceObjectId,
) -> Result<PinnedDirectory, String> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return Err("Task-clone root is not an absolute Windows path".into());
    };
    if !matches!(
        prefix.kind(),
        std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
    ) {
        return Err("Task-clone root is not on a local Windows drive".into());
    }
    let components = components.collect::<Vec<_>>();
    let root_is_volume = !components
        .iter()
        .any(|component| matches!(component, Component::Normal(_)));
    let volume_root = format!("{}\\", prefix.as_os_str().to_string_lossy());
    let volume = OpenOptions::new()
        .access_mode(directory_access(root_is_volume))
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .share_mode(SHARE_DIRECTORIES)
        .open(volume_root)
        .map_err(|_| "Windows volume root could not be opened safely")?;
    verify_directory(&volume)?;
    let mut handles = vec![volume];

    let last_normal = components
        .iter()
        .rposition(|component| matches!(component, Component::Normal(_)));
    let mut saw_root = false;
    for (index, component) in components.into_iter().enumerate() {
        match component {
            Component::RootDir => saw_root = true,
            Component::Normal(name) => {
                let child = open_directory(
                    handles.last().expect("volume handle exists"),
                    name,
                    Some(index) == last_normal,
                )?;
                handles.push(child);
            }
            _ => return Err("Task-clone root path is not canonical".into()),
        }
    }
    if !saw_root {
        return Err("Task-clone root is not an absolute Windows path".into());
    }
    let root = PinnedDirectory::new(handles)?;
    let actual_identity =
        crate::execution::docker::sandbox::workspace_identity::from_handle(root.handle())
            .ok_or("Task-clone root identity could not be read from its open handle")?;
    if &actual_identity != expected_identity {
        return Err("Task-clone root changed before stage-out synchronization".into());
    }
    Ok(root)
}

pub(in crate::execution::docker::sandbox::staging::sync::windows) fn parent(
    root: &PinnedDirectory,
    relative: &Path,
    for_update: bool,
) -> Result<Option<(PinnedDirectory, String)>, String> {
    validate_relative(relative).map_err(|_| "Workspace path is unsafe")?;
    let components = relative.components().collect::<Vec<_>>();
    let mut current = root.duplicate()?;
    let root_depth = root.handles.len();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err("Workspace path is unsafe".into());
        };
        if index + 1 == components.len() {
            root.retain(&current.handles[root_depth..])?;
            return Ok(Some((current, name.to_string_lossy().into_owned())));
        }
        let needs_update = for_update && index + 2 == components.len();
        match open_directory(current.handle(), name, needs_update) {
            Ok(directory) => current.handles.push(directory),
            Err(message) if message == MISSING_DIRECTORY => {
                root.retain(&current.handles[root_depth..])?;
                return Ok(None);
            }
            Err(message) => return Err(message.into()),
        }
    }
    Err("Workspace path is empty".into())
}

pub(in crate::execution::docker::sandbox::staging::sync::windows) fn pin_git_metadata(
    root: &PinnedDirectory,
    expected_identity: &WorkspaceObjectId,
) -> Result<File, String> {
    let directory = open_directory(root.handle(), std::ffi::OsStr::new(".git"), false)
        .map_err(|_| "Task-clone Git metadata could not be opened safely")?;
    let actual_identity =
        crate::execution::docker::sandbox::workspace_identity::from_handle(&directory)
            .ok_or("Task-clone Git metadata identity could not be read from its open handle")?;
    if &actual_identity != expected_identity {
        return Err("Task-clone Git metadata changed during synchronization".into());
    }
    Ok(directory)
}

pub(in crate::execution::docker::sandbox::staging::sync::windows) fn create_directory(
    root: &PinnedDirectory,
    relative: &Path,
) -> Result<(), String> {
    let (parent, name) =
        parent(root, relative, true)?.ok_or("Workspace directory parent is unavailable")?;
    let mut wide = name.encode_utf16().collect::<Vec<_>>();
    let directory = open_at(
        parent.handle(),
        &mut wide,
        READ_ATTRIBUTES | SYNCHRONIZE | TRAVERSE,
        SHARE_DIRECTORIES,
        FILE_CREATE,
        DIRECTORY_FILE | SYNCHRONOUS_IO_NONALERT,
        ATTRIBUTE_DIRECTORY,
    )
    .map_err(|_| "Workspace directory could not be created without replacing a concurrent entry")?;
    verify_directory(&directory)?;
    root.retain(std::slice::from_ref(&directory))
}

fn open_directory(
    parent: &File,
    name: &std::ffi::OsStr,
    for_update: bool,
) -> Result<File, &'static str> {
    let mut wide = name.encode_wide().collect::<Vec<_>>();
    let directory = match open_at(
        parent,
        &mut wide,
        directory_access(for_update),
        SHARE_DIRECTORIES,
        FILE_OPEN,
        OPEN_REPARSE_POINT | SYNCHRONOUS_IO_NONALERT,
        0,
    ) {
        Ok(directory) => directory,
        Err(STATUS_OBJECT_NAME_NOT_FOUND) | Err(STATUS_OBJECT_PATH_NOT_FOUND) => {
            return Err(MISSING_DIRECTORY);
        }
        Err(_) => return Err("Workspace path traverses a non-directory or reparse point"),
    };
    verify_directory(&directory).map_err(|_| "Workspace path traverses a reparse point")?;
    Ok(directory)
}

fn directory_access(for_update: bool) -> u32 {
    let base = READ_ATTRIBUTES | SYNCHRONIZE | TRAVERSE;
    if for_update {
        base | WRITE_DATA | APPEND_DATA
    } else {
        base
    }
}
