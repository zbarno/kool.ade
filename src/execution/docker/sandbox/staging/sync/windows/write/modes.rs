use std::path::Path;

use super::super::{Checkpoint, fs::PinnedDirectory, index::IndexModes};

pub(super) fn persist_new_file_mode(
    git_root: &Path,
    relative: &Path,
    parent: &PinnedDirectory,
    file: &std::fs::File,
    current_name: &str,
    index_executable: Option<bool>,
    target_executable: bool,
    hook: &mut impl FnMut(Checkpoint) -> bool,
) -> Result<(), String> {
    if let Err(error) = IndexModes::ensure_current(git_root, relative, index_executable) {
        let preserved = super::preserve_conflict(parent, file, current_name);
        return Err(format!("{error}; {preserved}"));
    }
    let Some(original) = index_executable else {
        return Ok(());
    };
    if original == target_executable {
        return Ok(());
    }
    if let Err(error) = IndexModes::set_if_current(git_root, relative, original, target_executable)
    {
        let index_rollback =
            IndexModes::restore_if_current(git_root, relative, target_executable, original);
        let preserved = super::preserve_conflict(parent, file, current_name);
        return Err(format!(
            "{error}; {}{}",
            preserved,
            rollback_detail(index_rollback)
        ));
    }
    if hook(Checkpoint::AfterIndexModeUpdate {
        target: relative.to_path_buf(),
    }) {
        let index_rollback =
            IndexModes::restore_if_current(git_root, relative, target_executable, original);
        let preserved = super::preserve_conflict(parent, file, current_name);
        return Err(format!(
            "Git index mode update was rolled back; {}{}",
            preserved,
            rollback_detail(index_rollback)
        ));
    }
    Ok(())
}

pub(super) fn persist_replacement_file_mode(
    git_root: &Path,
    relative: &Path,
    parent: &PinnedDirectory,
    previous_file: &std::fs::File,
    new_file: &std::fs::File,
    target_name: &str,
    backup_name: &str,
    index_executable: Option<bool>,
    target_executable: bool,
    hook: &mut impl FnMut(Checkpoint) -> bool,
) -> Result<(), String> {
    if let Err(error) = IndexModes::ensure_current(git_root, relative, index_executable) {
        let file_rollback =
            rollback_replacement(parent, previous_file, new_file, target_name, backup_name);
        return Err(format!("{error}; {file_rollback}"));
    }
    let Some(original) = index_executable else {
        return Ok(());
    };
    if original == target_executable {
        return Ok(());
    }
    let update = IndexModes::set_if_current(git_root, relative, original, target_executable);
    let failure = match update {
        Ok(())
            if !hook(Checkpoint::AfterIndexModeUpdate {
                target: relative.to_path_buf(),
            }) =>
        {
            return Ok(());
        }
        Ok(()) => "injected failure after Git index mode update".to_string(),
        Err(error) => error,
    };
    let index_rollback =
        IndexModes::restore_if_current(git_root, relative, target_executable, original);
    let file_rollback =
        rollback_replacement(parent, previous_file, new_file, target_name, backup_name);
    Err(format!(
        "{failure}; {file_rollback}{}",
        rollback_detail(index_rollback)
    ))
}

fn rollback_replacement(
    parent: &PinnedDirectory,
    previous_file: &std::fs::File,
    new_file: &std::fs::File,
    target_name: &str,
    backup_name: &str,
) -> String {
    let new_version = super::unique_name("conflict");
    if super::rename_noreplace(new_file, parent.handle(), &new_version).is_err() {
        let previous = super::preserve_conflict(parent, previous_file, backup_name);
        return format!("container version remains at destination; {previous}");
    }
    if super::rename_noreplace(previous_file, parent.handle(), target_name).is_err() {
        let previous = super::preserve_conflict(parent, previous_file, backup_name);
        return format!("container version is preserved as {new_version}; {previous}");
    }
    if super::delete_file(new_file).is_err() {
        let new_version = super::preserve_conflict(parent, new_file, &new_version);
        return format!("previous version restored; {new_version}");
    }
    "previous version restored and container replacement removed".into()
}

fn rollback_detail(result: Result<(), String>) -> String {
    match result {
        Ok(()) => String::new(),
        Err(error) => format!("; {error}"),
    }
}
