use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use uuid::Uuid;

mod modes;

use super::super::super::archive::{Fingerprint, ReturnedEntry};
use super::index::IndexModes;
use super::{
    Checkpoint,
    fs::{PinnedDirectory, fingerprint_at, fingerprint_handle, open_transaction_file, parent},
    native::{
        ATTRIBUTE_NORMAL, DELETE, FILE_CREATE, NON_DIRECTORY_FILE, OPEN_REPARSE_POINT,
        READ_ATTRIBUTES, SHARE_READ, STATUS_OBJECT_NAME_COLLISION, SYNCHRONIZE,
        SYNCHRONOUS_IO_NONALERT, WRITE_ATTRIBUTES, WRITE_DATA, delete_file, open_at,
        rename_noreplace, set_readonly,
    },
};

pub(super) fn write_file(
    git_root: &Path,
    root: &PinnedDirectory,
    relative: &Path,
    entry: &ReturnedEntry,
    expected: Option<&Fingerprint>,
    byte_limit: u64,
    index_executable: Option<bool>,
    hook: &mut impl FnMut(Checkpoint) -> bool,
) -> Result<(), String> {
    let (parent, name) =
        parent(root, relative, true)?.ok_or("Workspace file parent is unavailable")?;
    let Fingerprint::File { sha256, size, .. } = &entry.fingerprint else {
        return Err("Container returned a non-file workspace entry".into());
    };
    if *size != entry.bytes.len() as u64
        || *size > byte_limit
        || super::super::super::archive::hash(&entry.bytes) != *sha256
    {
        return Err("Container workspace file does not match its validated fingerprint".into());
    }
    if fingerprint_at(root, relative, byte_limit, index_executable)?.as_ref() != expected {
        return Err(
            "Task clone changed during synchronization; host changes were preserved".into(),
        );
    }
    let (temporary_name, mut temporary) = create_temporary(parent.handle(), "sync")?;
    let write_result = temporary
        .write_all(&entry.bytes)
        .and_then(|()| temporary.sync_all());
    if write_result.is_err() {
        let cleanup = cleanup_temporary(&parent, &temporary, &temporary_name);
        return Err(
            cleanup.unwrap_or_else(|| "Container workspace file could not be staged safely".into())
        );
    }
    if set_readonly(&temporary, entry.mode & 0o200 == 0).is_err() {
        let cleanup = cleanup_temporary(&parent, &temporary, &temporary_name);
        return Err(cleanup
            .unwrap_or_else(|| "Workspace file permissions could not be applied safely".into()));
    }
    if temporary.sync_all().is_err() {
        let cleanup = cleanup_temporary(&parent, &temporary, &temporary_name);
        return Err(
            cleanup.unwrap_or_else(|| "Workspace file metadata could not be flushed".into())
        );
    }

    if expected.is_none() {
        let injected_failure = hook(Checkpoint::BeforeCreateInstall {
            target: relative.to_path_buf(),
        });
        if injected_failure || rename_noreplace(&temporary, parent.handle(), &name).is_err() {
            let cleanup = cleanup_temporary(&parent, &temporary, &temporary_name);
            return Err(cleanup.unwrap_or_else(|| {
                "Concurrent task-clone change prevented safe synchronization".into()
            }));
        }
        modes::persist_new_file_mode(
            git_root,
            relative,
            &parent,
            &temporary,
            &name,
            index_executable,
            entry.mode & 0o111 != 0,
            hook,
        )?;
        return Ok(());
    }

    let Some(Fingerprint::File { .. }) = expected else {
        let cleanup = cleanup_temporary(&parent, &temporary, &temporary_name);
        return Err(cleanup.unwrap_or_else(|| {
            "Workspace file and directory type changes are not synchronized safely".into()
        }));
    };
    let mut displaced = match open_transaction_file(&parent, &name) {
        Ok(file) => file,
        Err(message) => {
            let cleanup = cleanup_temporary(&parent, &temporary, &temporary_name);
            return Err(cleanup.unwrap_or(message));
        }
    };
    let current = match fingerprint_handle(&mut displaced, byte_limit, index_executable) {
        Ok(current) => current,
        Err(message) => {
            let cleanup = cleanup_temporary(&parent, &temporary, &temporary_name);
            return Err(cleanup.unwrap_or(message));
        }
    };
    if Some(&current) != expected {
        let cleanup = cleanup_temporary(&parent, &temporary, &temporary_name);
        return Err(cleanup.unwrap_or_else(|| {
            "Task clone changed during synchronization; host changes were preserved".into()
        }));
    }
    let backup_name = unique_name("conflict");
    if rename_noreplace(&displaced, parent.handle(), &backup_name).is_err() {
        let cleanup = cleanup_temporary(&parent, &temporary, &temporary_name);
        return Err(cleanup.unwrap_or_else(|| {
            "Concurrent task-clone change prevented safe synchronization".into()
        }));
    }
    if fingerprint_handle(&mut displaced, byte_limit, index_executable)
        .ok()
        .as_ref()
        != expected
    {
        let preserved = restore_or_preserve(&parent, &displaced, &name, &backup_name);
        let cleanup = cleanup_temporary(&parent, &temporary, &temporary_name);
        return Err(format!(
            "Task clone changed during synchronization; {}{}",
            preserved,
            cleanup.unwrap_or_default()
        ));
    }

    let injected_failure = hook(Checkpoint::AfterReplaceAside {
        target: relative.to_path_buf(),
    });
    if injected_failure || rename_noreplace(&temporary, parent.handle(), &name).is_err() {
        let restored = restore_or_preserve(&parent, &displaced, &name, &backup_name);
        let cleanup = cleanup_temporary(&parent, &temporary, &temporary_name);
        return Err(format!(
            "Concurrent task-clone change prevented safe synchronization; {restored}{}",
            cleanup.unwrap_or_default()
        ));
    }
    modes::persist_replacement_file_mode(
        git_root,
        relative,
        &parent,
        &displaced,
        &temporary,
        &name,
        &backup_name,
        index_executable,
        entry.mode & 0o111 != 0,
        hook,
    )?;
    if delete_file(&displaced).is_err() {
        let preserved = preserve_conflict(&parent, &displaced, &backup_name);
        return Err(format!(
            "Previous workspace version could not be removed safely; {preserved}"
        ));
    }
    Ok(())
}

pub(super) fn remove_entry(
    git_root: &Path,
    root: &PinnedDirectory,
    relative: &Path,
    expected: Option<&Fingerprint>,
    byte_limit: u64,
    index_executable: Option<bool>,
    hook: &mut impl FnMut(Checkpoint) -> bool,
) -> Result<(), String> {
    if !matches!(expected, Some(Fingerprint::File { .. })) {
        return Err("Workspace directory deletion is not synchronized".into());
    }
    let (parent, name) =
        parent(root, relative, true)?.ok_or("Workspace deletion parent is unavailable")?;
    let mut displaced = open_transaction_file(&parent, &name)?;
    if Some(fingerprint_handle(
        &mut displaced,
        byte_limit,
        index_executable,
    )?) != expected
    {
        return Err(
            "Task clone changed during synchronization; host changes were preserved".into(),
        );
    }
    IndexModes::ensure_current(git_root, relative, index_executable)?;
    let backup_name = unique_name("conflict");
    if rename_noreplace(&displaced, parent.handle(), &backup_name).is_err() {
        return Err("Concurrent task-clone change prevented safe deletion".into());
    }
    if let Err(error) = IndexModes::ensure_current(git_root, relative, index_executable) {
        return Err(format!(
            "{error}; {}",
            restore_or_preserve(&parent, &displaced, &name, &backup_name)
        ));
    }
    if fingerprint_handle(&mut displaced, byte_limit, index_executable)
        .ok()
        .as_ref()
        != expected
    {
        return Err(format!(
            "Task clone changed during synchronization; {}",
            restore_or_preserve(&parent, &displaced, &name, &backup_name)
        ));
    }
    let _ = hook(Checkpoint::AfterDeleteAside {
        target: relative.to_path_buf(),
    });
    if delete_file(&displaced).is_err() {
        return Err(format!(
            "Workspace deletion could not safely remove its staged file; {}",
            restore_or_preserve(&parent, &displaced, &name, &backup_name)
        ));
    }
    Ok(())
}

fn create_temporary(parent: &File, kind: &str) -> Result<(String, File), String> {
    for _ in 0..4 {
        let name = unique_name(kind);
        let mut wide = name.encode_utf16().collect::<Vec<_>>();
        match open_at(
            parent,
            &mut wide,
            WRITE_DATA | WRITE_ATTRIBUTES | READ_ATTRIBUTES | DELETE | SYNCHRONIZE,
            SHARE_READ,
            FILE_CREATE,
            NON_DIRECTORY_FILE | OPEN_REPARSE_POINT | SYNCHRONOUS_IO_NONALERT,
            ATTRIBUTE_NORMAL,
        ) {
            Ok(file) => return Ok((name, file)),
            Err(STATUS_OBJECT_NAME_COLLISION) => {}
            Err(_) => return Err("Temporary workspace file could not be created safely".into()),
        }
    }
    Err("Temporary workspace file could not be created safely".into())
}

fn unique_name(prefix: &str) -> String {
    format!(".koolade-{prefix}-{}", Uuid::new_v4())
}

fn sibling_path(relative: &Path, name: &str) -> PathBuf {
    relative
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(name)
}

fn restore_or_preserve(
    parent: &PinnedDirectory,
    file: &File,
    target: &str,
    backup: &str,
) -> String {
    if rename_noreplace(file, parent.handle(), target).is_ok() {
        return "the previous file was restored".into();
    }
    preserve_conflict(parent, file, backup)
}

fn preserve_conflict(parent: &PinnedDirectory, file: &File, current_name: &str) -> String {
    for _ in 0..4 {
        let conflict = unique_name("conflict");
        if rename_noreplace(file, parent.handle(), &conflict).is_ok() {
            return format!("displaced data was preserved as {conflict}");
        }
    }
    format!("displaced data remains under recovery name {current_name}")
}

fn cleanup_temporary(parent: &PinnedDirectory, file: &File, name: &str) -> Option<String> {
    if delete_file(file).is_ok() {
        return None;
    }
    Some(preserve_conflict(parent, file, name))
}
