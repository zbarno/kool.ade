use std::{
    cell::RefCell,
    collections::HashSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DEVICE, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT,
};

use self::metadata::{file_id, metadata};
use super::super::super::archive::{Fingerprint, hash};
use super::native::{
    FILE_OPEN, OPEN_REPARSE_POINT, READ_ATTRIBUTES, READ_DATA, SHARE_READ,
    STATUS_OBJECT_NAME_NOT_FOUND, STATUS_OBJECT_PATH_NOT_FOUND, SYNCHRONIZE,
    SYNCHRONOUS_IO_NONALERT, open_at,
};

mod directories;
mod metadata;
pub(super) use directories::{create_directory, open_root_nofollow, parent, pin_git_metadata};

pub(super) struct PinnedDirectory {
    handles: Vec<File>,
    retained: RefCell<Vec<File>>,
    identities: RefCell<HashSet<([u8; 16], u64)>>,
}

impl PinnedDirectory {
    fn new(handles: Vec<File>) -> Result<Self, String> {
        let identities = handles
            .iter()
            .map(file_id)
            .collect::<Result<HashSet<_>, _>>()?;
        Ok(Self {
            handles,
            retained: RefCell::new(Vec::new()),
            identities: RefCell::new(identities),
        })
    }

    pub(super) fn handle(&self) -> &File {
        self.handles
            .last()
            .expect("a pinned directory has a handle")
    }

    fn duplicate(&self) -> Result<Self, String> {
        let handles = self
            .handles
            .iter()
            .map(|handle| {
                handle
                    .try_clone()
                    .map_err(|_| "Workspace directory handle could not be retained".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(handles)
    }

    fn retain(&self, files: &[File]) -> Result<(), String> {
        let mut identities = self.identities.borrow_mut();
        let mut retained = self.retained.borrow_mut();
        for file in files {
            let identity = file_id(file)?;
            if !identities.contains(&identity) {
                let retained_file = file
                    .try_clone()
                    .map_err(|_| "Workspace directory handle could not be retained")?;
                identities.insert(identity);
                retained.push(retained_file);
            }
        }
        Ok(())
    }
}

pub(super) fn open_target(
    parent: &PinnedDirectory,
    name: &str,
    desired_access: u32,
    share_access: u32,
) -> Result<File, i32> {
    let mut wide = name.encode_utf16().collect::<Vec<_>>();
    open_at(
        parent.handle(),
        &mut wide,
        desired_access,
        share_access,
        FILE_OPEN,
        OPEN_REPARSE_POINT | SYNCHRONOUS_IO_NONALERT,
        0,
    )
}

pub(super) fn fingerprint_at(
    root: &PinnedDirectory,
    relative: &Path,
    byte_limit: u64,
    index_executable: Option<bool>,
) -> Result<Option<Fingerprint>, String> {
    let Some((parent, name)) = parent(root, relative, false)? else {
        return Ok(None);
    };
    let read_access = READ_DATA | READ_ATTRIBUTES | SYNCHRONIZE;
    let mut file = match open_target(&parent, &name, read_access, SHARE_READ) {
        Ok(file) => file,
        Err(STATUS_OBJECT_NAME_NOT_FOUND) | Err(STATUS_OBJECT_PATH_NOT_FOUND) => return Ok(None),
        Err(_) => return Err("Workspace destination could not be opened safely".into()),
    };
    let (attributes, standard, basic) = metadata(&file)?;
    if attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || attributes.FileAttributes & FILE_ATTRIBUTE_DEVICE != 0
    {
        return Err("Workspace destination is a reparse point or special file".into());
    }
    if standard.Directory {
        return Ok(Some(Fingerprint::Directory));
    }
    if standard.EndOfFile < 0 || standard.NumberOfLinks != 1 {
        return Err("Workspace destination is a hard link or special file".into());
    }
    let file_size = u64::try_from(standard.EndOfFile)
        .map_err(|_| "Workspace destination is a hard link or special file")?;
    if file_size > byte_limit {
        return Err("Workspace destination exceeds the synchronization limit".into());
    }
    let initial_id = file_id(&file)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(byte_limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "Workspace destination could not be read")?;
    if bytes.len() as u64 > byte_limit {
        return Err("Workspace destination exceeds the synchronization limit".into());
    }
    let (_, after, basic_after) = metadata(&file)?;
    if after.EndOfFile != standard.EndOfFile
        || basic_after.LastWriteTime != basic.LastWriteTime
        || basic_after.ChangeTime != basic.ChangeTime
        || file_id(&file)? != initial_id
    {
        return Err("Workspace destination changed during synchronization".into());
    }
    let current = open_target(&parent, &name, read_access, SHARE_READ)
        .map_err(|_| "Workspace destination changed during synchronization")?;
    if file_id(&current)? != initial_id {
        return Err("Workspace destination changed during synchronization".into());
    }
    let mode = file_mode(attributes.FileAttributes, index_executable);
    Ok(Some(Fingerprint::File {
        sha256: hash(&bytes),
        size: bytes.len() as u64,
        mode,
    }))
}

pub(super) fn fingerprint_handle(
    file: &mut File,
    byte_limit: u64,
    index_executable: Option<bool>,
) -> Result<Fingerprint, String> {
    let (attributes, before, basic) = metadata(file)?;
    if attributes.FileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DEVICE) != 0
        || before.Directory
        || before.EndOfFile < 0
        || before.NumberOfLinks != 1
    {
        return Err("Workspace destination is not a singly linked regular file".into());
    }
    let file_size = u64::try_from(before.EndOfFile)
        .map_err(|_| "Workspace destination is not a singly linked regular file")?;
    if file_size > byte_limit {
        return Err("Workspace destination exceeds the synchronization limit".into());
    }
    let id = file_id(file)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "Workspace destination could not be rewound")?;
    let mut bytes = Vec::new();
    (&mut *file)
        .take(byte_limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "Workspace destination could not be read")?;
    let (_, after, basic_after) = metadata(file)?;
    if bytes.len() as u64 > byte_limit
        || after.EndOfFile != before.EndOfFile
        || basic_after.LastWriteTime != basic.LastWriteTime
        || basic_after.ChangeTime != basic.ChangeTime
        || file_id(file)? != id
    {
        return Err("Workspace destination changed during synchronization".into());
    }
    let mode = file_mode(attributes.FileAttributes, index_executable);
    Ok(Fingerprint::File {
        sha256: hash(&bytes),
        size: bytes.len() as u64,
        mode,
    })
}

fn file_mode(attributes: u32, index_executable: Option<bool>) -> u32 {
    let mode = if index_executable.unwrap_or(false) {
        0o755
    } else {
        0o644
    };
    if attributes & FILE_ATTRIBUTE_READONLY != 0 {
        mode & !0o222
    } else {
        mode
    }
}

pub(super) fn open_transaction_file(parent: &PinnedDirectory, name: &str) -> Result<File, String> {
    open_target(
        parent,
        name,
        READ_DATA | READ_ATTRIBUTES | super::native::DELETE | SYNCHRONIZE,
        SHARE_READ,
    )
    .map_err(|_| "Workspace file is busy or changed; host data was preserved".into())
}
