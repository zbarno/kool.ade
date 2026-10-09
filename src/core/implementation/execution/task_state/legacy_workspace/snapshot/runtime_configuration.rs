use super::super::copy;
use super::{RUNTIME_CONFIG, ensure_real_directory};
use std::{
    collections::BTreeSet,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

pub(super) fn capture(source: &Path, snapshot: &Path) -> anyhow::Result<()> {
    capture_paths(
        source,
        snapshot,
        crate::harness::pi_sandbox::runtime_config::paths(source)?,
    )
}

fn capture_paths(source: &Path, snapshot: &Path, paths: BTreeSet<String>) -> anyhow::Result<()> {
    let list = snapshot.join(RUNTIME_CONFIG);
    let mut bytes = Vec::new();
    for path in paths {
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(0);
    }
    write_list(&list, &bytes)?;
    let root = snapshot.join("runtime-config");
    ensure_real_directory(&root)?;
    copy::copy_untracked(source, &root, &list, true)
}

fn write_list(list: &Path, expected: &[u8]) -> anyhow::Result<()> {
    let temporary = list.with_file_name(".runtime-config.list.tmp");
    ensure_temporary_absent_or_remove(&temporary)?;
    match fs::symlink_metadata(list) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Migration runtime configuration list is not a regular file"
            );
            anyhow::ensure!(
                fs::read(list)? == expected,
                "Original granted runtime configuration changed during migration"
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            output.write_all(expected)?;
            output.sync_all()?;
            drop(output);
            match fs::hard_link(&temporary, list) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let metadata = fs::symlink_metadata(list)?;
                    anyhow::ensure!(
                        metadata.is_file()
                            && !metadata.file_type().is_symlink()
                            && fs::read(list)? == expected,
                        "Original granted runtime configuration changed during migration"
                    );
                }
                Err(error) => return Err(error.into()),
            }
            fs::remove_file(&temporary)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn ensure_temporary_absent_or_remove(temporary: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(temporary) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Migration runtime configuration temporary path is not a regular file"
            );
            fs::remove_file(temporary)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

pub(super) fn matches(source: &Path, snapshot: &Path) -> anyhow::Result<bool> {
    let list = snapshot.join(RUNTIME_CONFIG);
    let metadata = match fs::symlink_metadata(&list) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(crate::harness::pi_sandbox::runtime_config::paths(source)?.is_empty());
        }
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Migration runtime configuration list is not a regular file"
    );
    let mut expected = Vec::new();
    for path in crate::harness::pi_sandbox::runtime_config::paths(source)? {
        expected.extend_from_slice(path.as_bytes());
        expected.push(0);
    }
    Ok(fs::read(&list)? == expected
        && copy::untracked_match(source, &snapshot.join("runtime-config"), &list)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_runtime_config_snapshot_can_be_retried() {
        let root = std::env::temp_dir().join(format!(
            "koolade-runtime-config-snapshot-{}",
            uuid::Uuid::new_v4()
        ));
        let source = root.join("source");
        let snapshot = root.join("snapshot");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(snapshot.join("runtime-config")).unwrap();
        fs::write(source.join(".env"), "SYNTHETIC_CONFIG=preserved\n").unwrap();
        fs::write(snapshot.join("runtime-config/.env"), "partial copy\n").unwrap();
        fs::write(snapshot.join(".runtime-config.list.tmp"), "partial list\0").unwrap();

        let paths = BTreeSet::from([".env".to_owned()]);
        capture_paths(&source, &snapshot, paths.clone()).unwrap();
        capture_paths(&source, &snapshot, paths).unwrap();

        assert_eq!(
            fs::read_to_string(snapshot.join("runtime-config/.env")).unwrap(),
            "SYNTHETIC_CONFIG=preserved\n"
        );
        assert_eq!(fs::read(snapshot.join(RUNTIME_CONFIG)).unwrap(), b".env\0");
        assert!(!snapshot.join(".runtime-config.list.tmp").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
