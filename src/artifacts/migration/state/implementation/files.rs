use std::{fs, path::Path};

use super::super::super::plan::Plan;
use crate::core::implementation::Implementation;

pub(super) fn migrated_ticket(
    repo: &Path,
    plan: &Plan,
    ticket: &str,
    text: &str,
) -> anyhow::Result<String> {
    let Some(target) = super::super::super::plan::relocated_ticket_path(ticket) else {
        return Ok(ticket.to_owned());
    };
    validate_task(repo, plan, &target, text, ticket)?;
    Ok(target)
}

pub(super) fn validate_task(
    repo: &Path,
    plan: &Plan,
    ticket: &str,
    expected: &str,
    old_ticket: &str,
) -> anyhow::Result<()> {
    let actual = match plan.planned_content(ticket) {
        Some(bytes) => String::from_utf8(bytes.to_vec())?,
        None => fs::read_to_string(repo.join(ticket))?,
    };
    anyhow::ensure!(
        crate::artifacts::task_docs::visible_content(&actual) == expected,
        "Implementation evidence for {old_ticket} does not match task contents at {ticket}; resolve the ticket identity conflict before connecting"
    );
    Ok(())
}

pub(super) fn hold_run_lock(path: &Path, locks: &mut Vec<fs::File>) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Implementation lock {} must be a regular file",
            path.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    let file = fs::OpenOptions::new().read(true).write(true).open(path)?;
    file.try_lock().map_err(|_| {
        anyhow::anyhow!(
            "Task implementation is active for {}; finish or stop it before migrating project artifacts",
            path.parent().unwrap_or(path).display()
        )
    })?;
    locks.push(file);
    Ok(())
}

pub(super) fn check_parent_chain(repo: &Path, parent: &Path) -> anyhow::Result<()> {
    let relative = parent.strip_prefix(repo)?;
    let mut current = repo.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) => anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "Implementation target parent {} is not a real directory",
                current.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(super) fn same_tree(
    left: &Path,
    right: &Path,
    left_state: &Implementation,
    right_state: &Implementation,
) -> anyhow::Result<bool> {
    if left_state != right_state {
        return Ok(false);
    }
    same_tree_contents(left, right, true)
}

fn same_tree_contents(left: &Path, right: &Path, top_level: bool) -> anyhow::Result<bool> {
    let mut a = fs::read_dir(left)?.collect::<Result<Vec<_>, _>>()?;
    let mut b = fs::read_dir(right)?.collect::<Result<Vec<_>, _>>()?;
    a.sort_by_key(|entry| entry.file_name());
    b.sort_by_key(|entry| entry.file_name());
    if a.len() != b.len() {
        return Ok(false);
    }
    for (one, two) in a.iter().zip(&b) {
        if one.file_name() != two.file_name() {
            return Ok(false);
        }
        let one_meta = fs::symlink_metadata(one.path())?;
        let two_meta = fs::symlink_metadata(two.path())?;
        if one_meta.file_type().is_symlink() || two_meta.file_type().is_symlink() {
            anyhow::bail!("Implementation evidence contains a symbolic link");
        }
        if one_meta.is_dir() != two_meta.is_dir() || one_meta.is_file() != two_meta.is_file() {
            return Ok(false);
        }
        if one_meta.is_dir() {
            if !same_tree_contents(&one.path(), &two.path(), false)? {
                return Ok(false);
            }
        } else if !(top_level && one.file_name() == "state.json")
            && fs::read(one.path())? != fs::read(two.path())?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn copy_tree(source: &Path, target: &Path) -> anyhow::Result<()> {
    fs::create_dir(target)?;
    let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let from = entry.path();
        let to = target.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from)?;
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "Implementation evidence {} is linked and cannot be migrated safely",
            from.display()
        );
        if metadata.is_dir() {
            copy_tree(&from, &to)?;
        } else if metadata.is_file() {
            fs::copy(from, to)?;
        } else {
            anyhow::bail!(
                "Unsupported implementation evidence entry {}",
                from.display()
            );
        }
    }
    Ok(())
}

pub(super) fn validate_tree(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        !metadata.file_type().is_symlink(),
        "Implementation evidence {} is linked and cannot be migrated safely",
        path.display()
    );
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            validate_tree(&entry?.path())?;
        }
    } else {
        anyhow::ensure!(
            metadata.is_file(),
            "Unsupported implementation evidence entry {}",
            path.display()
        );
    }
    Ok(())
}

pub(super) fn remove_safe_tree(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Refusing to remove non-directory migration path {}",
        path.display()
    );
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    for entry in entries.drain(..) {
        let child = entry.path();
        let meta = fs::symlink_metadata(&child)?;
        if meta.file_type().is_symlink() {
            anyhow::bail!(
                "Refusing to remove linked implementation evidence {}",
                child.display()
            );
        } else if meta.is_dir() {
            remove_safe_tree(&child)?;
        } else if meta.is_file() {
            fs::remove_file(child)?;
        } else {
            anyhow::bail!(
                "Unsupported implementation evidence entry {}",
                child.display()
            );
        }
    }
    fs::remove_dir(path)?;
    Ok(())
}

pub(super) fn remove_if_empty(path: &Path) -> std::io::Result<()> {
    if fs::read_dir(path)?.next().is_none() {
        fs::remove_dir(path)?;
    }
    Ok(())
}
