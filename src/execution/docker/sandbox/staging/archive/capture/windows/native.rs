use std::{
    fs::{File, OpenOptions},
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path},
    ptr,
};

use anyhow::{Context, bail, ensure};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_OPEN, FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::{
        Foundation::{HANDLE, UNICODE_STRING},
        Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

use super::entries;

const OBJ_CASE_INSENSITIVE: u32 = 0x0000_0040;
const SYNCHRONIZE: u32 = 0x0010_0000;
const SHARE_READ: u32 = FILE_SHARE_READ;
const SHARE_READ_WRITE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE;
pub(super) const OPEN_REPARSE_ATTRIBUTES: u32 =
    FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE;

pub(super) struct RootHandle {
    pub directory: File,
    _ancestors: Vec<File>,
}

pub(super) struct ScopeHandle {
    pub directory: File,
    _ancestors: Vec<File>,
}

pub(super) fn open_root(path: &Path) -> anyhow::Result<RootHandle> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        bail!("task-clone root is not an absolute Windows path");
    };
    if !matches!(
        prefix.kind(),
        std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
    ) {
        bail!("task-clone root is not on a local Windows drive");
    }
    let volume_root = format!("{}\\", prefix.as_os_str().to_string_lossy());
    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(SHARE_READ_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    let mut current = options
        .open(volume_root)
        .context("Windows volume root could not be opened safely")?;
    entries::verify_directory(&current)?;
    let mut rest = components.collect::<Vec<_>>();
    let mut ancestors = Vec::new();
    let mut saw_root = false;
    let last = rest.len().saturating_sub(1);
    for (index, component) in rest.drain(..).enumerate() {
        match component {
            Component::RootDir => saw_root = true,
            Component::Normal(name) => {
                ensure!(saw_root, "task-clone root path is not canonical");
                let share = if index == last {
                    SHARE_READ
                } else {
                    SHARE_READ_WRITE
                };
                let child = open_directory_at(&current, name, share)?;
                ancestors.push(current);
                current = child;
            }
            _ => bail!("task-clone root path is not canonical"),
        }
    }
    ensure!(saw_root, "task-clone root is not absolute");
    Ok(RootHandle {
        directory: current,
        _ancestors: ancestors,
    })
}

pub(super) fn open_scope(root: &File, scope: &Path) -> anyhow::Result<ScopeHandle> {
    if scope == Path::new(".") {
        return Ok(ScopeHandle {
            directory: root.try_clone()?,
            _ancestors: Vec::new(),
        });
    }
    super::super::super::validate_relative(scope)?;
    ensure!(
        !scope.components().any(|component| {
            matches!(component, Component::Normal(name) if name.to_string_lossy().eq_ignore_ascii_case(".git"))
        }),
        "scoped workspace path cannot target Git metadata"
    );
    let mut current = root.try_clone()?;
    let mut ancestors = Vec::new();
    for component in scope.components() {
        let Component::Normal(name) = component else {
            bail!("scoped workspace path is invalid");
        };
        let child = open_directory_at(&current, name, SHARE_READ)?;
        ancestors.push(current);
        current = child;
    }
    Ok(ScopeHandle {
        directory: current,
        _ancestors: ancestors,
    })
}

fn open_directory_at(
    parent: &File,
    name: &std::ffi::OsStr,
    share_access: u32,
) -> anyhow::Result<File> {
    let directory = open_at_with_share(parent, name, OPEN_REPARSE_ATTRIBUTES, share_access)?;
    entries::verify_directory(&directory)?;
    Ok(directory)
}

pub(super) fn open_at(
    parent: &File,
    name: &std::ffi::OsStr,
    desired_access: u32,
) -> anyhow::Result<File> {
    open_at_with_share(parent, name, desired_access, SHARE_READ)
}

fn open_at_with_share(
    parent: &File,
    name: &std::ffi::OsStr,
    desired_access: u32,
    share_access: u32,
) -> anyhow::Result<File> {
    let mut wide = name.encode_wide().collect::<Vec<_>>();
    let byte_length = wide
        .len()
        .checked_mul(size_of::<u16>())
        .filter(|length| *length > 0 && *length <= u16::MAX as usize)
        .ok_or_else(|| anyhow::anyhow!("workspace component is too long"))?;
    ensure!(
        !wide.contains(&0),
        "workspace component contains a null character"
    );
    let name = UNICODE_STRING {
        Length: byte_length as u16,
        MaximumLength: byte_length as u16,
        Buffer: wide.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle() as HANDLE,
        ObjectName: &name,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: ptr::null(),
        SecurityQualityOfService: ptr::null(),
    };
    let mut status_block = IO_STATUS_BLOCK::default();
    let mut handle = ptr::null_mut();
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            desired_access,
            &attributes,
            &mut status_block,
            ptr::null(),
            0,
            share_access,
            FILE_OPEN,
            FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            ptr::null(),
            0,
        )
    };
    ensure!(
        status >= 0 && !handle.is_null(),
        "workspace entry could not be opened safely"
    );
    Ok(unsafe { File::from_raw_handle(handle) })
}
