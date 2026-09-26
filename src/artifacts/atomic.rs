//! Durable sibling-file writes with collision-safe staging and crash recovery.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Atomically write `text` to `path` (write-to-temp + rename) so a crash
/// mid-write can never truncate a planning artifact. Parent dirs are created.
pub fn atomic_write(path: &Path, text: &str) -> anyhow::Result<()> {
    atomic_write_bytes(path, text.as_bytes())
}

/// Byte-oriented version of [`atomic_write`] for serialized records.
pub fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    remove_old_temps(path);
    let (tmp, mut file) = create_temp(path)?;
    let result = (|| -> anyhow::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        replace_file(&tmp, path)?;
        sync_parent_directory(path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(|error| anyhow::anyhow!("cannot atomically write {}: {error}", path.display()))
}

/// Atomically install a new file without replacing a concurrent writer's
/// destination. Used for imported evidence whose selected name must not be
/// overwritten if another import wins the same name at the same time.
pub fn atomic_create_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    create_parent(path)?;
    remove_old_temps(path);
    let (temp, mut file) = create_temp(path)?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::hard_link(&temp, path)?;
        fs::remove_file(&temp)?;
        sync_parent_directory(path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Streaming counterpart to [`atomic_create_bytes`] for large imported files.
pub fn atomic_copy_new(source: &Path, destination: &Path) -> std::io::Result<()> {
    create_parent(destination)?;
    remove_old_temps(destination);
    let (temp, mut output) = create_temp(destination)?;
    let result = (|| -> std::io::Result<()> {
        let mut input = File::open(source)?;
        std::io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        fs::hard_link(&temp, destination)?;
        fs::remove_file(&temp)?;
        sync_parent_directory(destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn create_parent(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(parent_dir(path))
}

fn create_temp(path: &Path) -> std::io::Result<(PathBuf, File)> {
    let parent = parent_dir(path);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "packet".into());
    for _ in 0..128 {
        let sequence = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!(
            ".{name}.{}-{sequence}.packet.tmp",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => return Ok((temp, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::other(format!(
        "could not allocate a unique temporary file for {}",
        path.display()
    )))
}

fn remove_old_temps(path: &Path) {
    let parent = parent_dir(path);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "packet".into());
    let prefix = format!(".{name}.");
    let cutoff = SystemTime::now() - Duration::from_secs(24 * 60 * 60);
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.starts_with(&prefix) || !file_name.ends_with(".packet.tmp") {
            continue;
        }
        if entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .is_some_and(|modified| modified < cutoff)
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let source = source
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let target = target
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            #[link_name = "MoveFileExW"]
            fn move_file_ex_w(existing: *const u16, new: *const u16, flags: u32) -> i32;
        }
        // SAFETY: both paths are NUL-terminated UTF-16 buffers that remain
        // alive for the call; MoveFileExW does not retain either pointer.
        if unsafe {
            move_file_ex_w(
                source.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(source, target)
    }
}

pub(crate) fn sync_parent_directory(_path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    File::open(parent_dir(_path))?.sync_all()?;
    Ok(())
}

fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn concurrent_atomic_writes_use_unique_synced_temporaries() {
        let root = std::env::temp_dir().join(format!(
            "packet-atomic-write-race-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("state.json");
        let values = (0..12)
            .map(|index| format!("complete-payload-{index}-{}", "x".repeat(8_000)))
            .collect::<Vec<_>>();
        let workers = values
            .iter()
            .cloned()
            .map(|value| {
                let target = target.clone();
                thread::spawn(move || atomic_write(&target, &value).unwrap())
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        let written = fs::read_to_string(&target).unwrap();
        assert!(values.iter().any(|expected| expected == &written));
        assert!(fs::read_dir(&root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".packet.tmp")
        }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_replace_preserves_target_and_removes_its_temporary() {
        let root = std::env::temp_dir().join(format!(
            "packet-atomic-write-failure-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("state.json");
        fs::create_dir(&target).unwrap();
        assert!(atomic_write(&target, "new state").is_err());
        assert!(target.is_dir());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn next_write_removes_only_old_matching_orphan_temps() {
        let root = std::env::temp_dir().join(format!(
            "packet-atomic-write-recovery-{}-{}",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("state.json");
        let orphan = root.join(".state.json.abandoned.packet.tmp");
        let other = root.join(".other.json.abandoned.packet.tmp");
        File::create(&orphan)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(SystemTime::now() - Duration::from_secs(48 * 60 * 60)),
            )
            .unwrap();
        File::create(&other)
            .unwrap()
            .set_times(
                std::fs::FileTimes::new()
                    .set_modified(SystemTime::now() - Duration::from_secs(48 * 60 * 60)),
            )
            .unwrap();
        atomic_write(&target, "recovered state").unwrap();
        assert!(!orphan.exists());
        assert!(other.exists());
        assert_eq!(fs::read_to_string(target).unwrap(), "recovered state");
        let _ = fs::remove_dir_all(root);
    }
}
