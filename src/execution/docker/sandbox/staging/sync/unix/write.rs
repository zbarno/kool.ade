use std::{
    ffi::CString,
    fs::File,
    io::Write,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    },
    path::{Path, PathBuf},
};

use super::{
    Fingerprint, ReturnedEntry,
    fs::{fingerprint_at, parent},
};

pub(super) fn write_file(
    root: &File,
    relative: &Path,
    entry: &ReturnedEntry,
    expected: Option<&Fingerprint>,
    byte_limit: u64,
) -> Result<(), String> {
    let (parent, name) = parent(root, relative)?.ok_or("Workspace file parent is unavailable")?;
    if fingerprint_at(root, relative, byte_limit)?.as_ref() != expected {
        return Err(
            "Task clone changed during synchronization; host changes were preserved".into(),
        );
    }
    let temporary = CString::new(format!(".koolade-sync-{}", uuid::Uuid::new_v4())).unwrap();
    let mode = (entry.mode & 0o777) | 0o200;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            temporary.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            mode,
        )
    };
    if fd < 0 {
        return Err("Temporary workspace file could not be created safely".into());
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    if unsafe { libc::fchmod(file.as_raw_fd(), mode) } != 0 {
        drop(file);
        unsafe {
            libc::unlinkat(parent.as_raw_fd(), temporary.as_ptr(), 0);
        }
        return Err("Workspace file permissions could not be applied safely".into());
    }
    let write_result = file.write_all(&entry.bytes).and_then(|()| file.sync_all());
    drop(file);
    if write_result.is_err() {
        unsafe {
            libc::unlinkat(parent.as_raw_fd(), temporary.as_ptr(), 0);
        }
        return Err("Container workspace file could not be staged safely".into());
    }
    let checked = fingerprint_at(root, relative, byte_limit);
    if !matches!(checked, Ok(current) if current.as_ref() == expected) {
        unsafe {
            libc::unlinkat(parent.as_raw_fd(), temporary.as_ptr(), 0);
        }
        return Err(
            "Task clone changed during synchronization; host changes were preserved".into(),
        );
    }
    if expected.is_none() {
        if rename_noreplace(&parent, &temporary, &name) != 0 {
            unlink_if_exists(&parent, &temporary);
            return Err("Concurrent task-clone change prevented safe synchronization".into());
        }
    } else {
        if rename_exchange(&parent, &temporary, &name) != 0 {
            unlink_if_exists(&parent, &temporary);
            return Err(
                "Atomic workspace replacement is unavailable; host changes were preserved".into(),
            );
        }
        let displaced = sibling_path(relative, &temporary);
        let displaced_matches = matches!(
            fingerprint_at(root, &displaced, byte_limit),
            Ok(current) if current.as_ref() == expected
        );
        if !displaced_matches {
            if rename_exchange(&parent, &temporary, &name) == 0 {
                let displaced_after_restore = sibling_path(relative, &temporary);
                let is_container_file = matches!(
                    fingerprint_at(root, &displaced_after_restore, byte_limit),
                    Ok(current) if current.as_ref() == Some(&entry.fingerprint)
                );
                if is_container_file {
                    if unlink_name(&parent, &temporary).is_err() {
                        preserve_conflict(&parent, &temporary);
                    }
                } else {
                    preserve_conflict(&parent, &temporary);
                }
            } else {
                preserve_conflict(&parent, &temporary);
            }
            return Err(
                "Task clone changed during synchronization; concurrent host data was preserved"
                    .into(),
            );
        }
        if unlink_name(&parent, &temporary).is_err() {
            preserve_conflict(&parent, &temporary);
            return Err("Previous workspace version could not be removed safely".into());
        }
    }
    if unsafe { libc::fsync(parent.as_raw_fd()) } != 0 {
        return Err("Workspace directory update could not be flushed".into());
    }
    Ok(())
}

pub(super) fn remove_entry(
    root: &File,
    relative: &Path,
    expected: Option<&Fingerprint>,
    byte_limit: u64,
) -> Result<(), String> {
    if !matches!(expected, Some(Fingerprint::File { .. })) {
        return Err("Workspace directory deletion is not synchronized".into());
    }
    let (parent, name) =
        parent(root, relative)?.ok_or("Workspace deletion parent is unavailable")?;
    if fingerprint_at(root, relative, byte_limit)?.as_ref() != expected {
        return Err(
            "Task clone changed during synchronization; host changes were preserved".into(),
        );
    }
    let temporary = CString::new(format!(".koolade-delete-{}", uuid::Uuid::new_v4())).unwrap();
    if rename_noreplace(&parent, &name, &temporary) != 0 {
        return Err("Concurrent task-clone change prevented safe deletion".into());
    }
    let moved = sibling_path(relative, &temporary);
    let moved_matches = matches!(
        fingerprint_at(root, &moved, byte_limit),
        Ok(current) if current.as_ref() == expected
    );
    if !moved_matches {
        if rename_noreplace(&parent, &temporary, &name) != 0 {
            preserve_conflict(&parent, &temporary);
        }
        return Err(
            "Task clone changed during synchronization; concurrent host data was preserved".into(),
        );
    }
    if unlink_name(&parent, &temporary).is_err() {
        if rename_noreplace(&parent, &temporary, &name) != 0 {
            preserve_conflict(&parent, &temporary);
        }
        return Err("Workspace deletion could not safely remove its staged file".into());
    }
    if unsafe { libc::fsync(parent.as_raw_fd()) } != 0 {
        return Err("Workspace deletion could not be flushed".into());
    }
    Ok(())
}

pub(super) fn rename_noreplace(parent: &File, from: &CString, to: &CString) -> i32 {
    unsafe {
        libc::renameat2(
            parent.as_raw_fd(),
            from.as_ptr(),
            parent.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    }
}

pub(super) fn rename_exchange(parent: &File, left: &CString, right: &CString) -> i32 {
    unsafe {
        libc::renameat2(
            parent.as_raw_fd(),
            left.as_ptr(),
            parent.as_raw_fd(),
            right.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    }
}

pub(super) fn sibling_path(relative: &Path, sibling: &CString) -> PathBuf {
    let mut path = relative
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf();
    path.push(std::ffi::OsStr::from_bytes(sibling.as_bytes()));
    path
}

pub(super) fn unlink_if_exists(parent: &File, name: &CString) {
    if unlink_name(parent, name).is_err() {
        preserve_conflict(parent, name);
    }
}

pub(super) fn unlink_name(parent: &File, name: &CString) -> Result<(), String> {
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err("Temporary workspace file could not be removed safely".into())
    }
}

pub(super) fn preserve_conflict(parent: &File, temporary: &CString) {
    let conflict = CString::new(format!(".koolade-conflict-{}", uuid::Uuid::new_v4())).unwrap();
    let _ = rename_noreplace(parent, temporary, &conflict);
}
