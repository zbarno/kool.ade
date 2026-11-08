use std::path::{Path, PathBuf};

/// Private parent for short lived Kool.ad/e runtime files.
pub fn temporary_root() -> anyhow::Result<PathBuf> {
    temporary_root_at(&super::state_root())
}

fn temporary_root_at(state_root: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(state_root)?;
    let state_root = state_root.canonicalize()?;
    let root = state_root.join("tmp");
    match std::fs::symlink_metadata(&root) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Kool.ad/e temporary path is not a private directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::create_dir(&root) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(error) => return Err(error.into()),
    }
    let metadata = std::fs::symlink_metadata(&root)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Kool.ad/e temporary path is not a directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() },
            "Kool.ad/e temporary directory is not owned by the current user"
        );
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        anyhow::ensure!(
            metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "Kool.ad/e temporary path cannot be a reparse point"
        );
    }
    let resolved = root.canonicalize()?;
    anyhow::ensure!(
        resolved.starts_with(&state_root),
        "Kool.ad/e temporary directory escapes the application state directory"
    );
    Ok(resolved)
}

/// Create a uniquely named, automatically removed runtime directory.
pub fn temporary_directory(prefix: &str) -> anyhow::Result<tempfile::TempDir> {
    Ok(tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(temporary_root()?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_directory_is_created_under_private_application_state_and_removed() {
        let root = temporary_root().unwrap();
        let directory = temporary_directory("runtime-temp-test-").unwrap();
        assert!(directory.path().starts_with(root));
        let path = directory.path().to_path_buf();
        drop(directory);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn long_synthetic_test_home_session_socket_path_fits_unix_domain_socket_limit() {
        let root = PathBuf::from("/home/synthetic-user/koolade-test-state-00000000000000000000");
        let session = root
            .join("tmp/s")
            .join("r-00000000000000000000")
            .join("e.sock");
        assert!(session.as_os_str().as_encoded_bytes().len() <= 107);
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_first_use_rechecks_the_created_directory() {
        let state = tempfile::tempdir().unwrap();
        std::thread::scope(|scope| {
            let handles = (0..8)
                .map(|_| scope.spawn(|| temporary_root_at(state.path()).unwrap()))
                .collect::<Vec<_>>();
            for handle in handles {
                assert_eq!(handle.join().unwrap(), state.path().join("tmp"));
            }
        });
    }

    #[cfg(unix)]
    #[test]
    fn temporary_root_rejects_files_and_symlinks_without_changing_the_target() {
        for use_symlink in [false, true] {
            let state = tempfile::tempdir().unwrap();
            let target = tempfile::tempdir().unwrap();
            let root = state.path().join("tmp");
            if use_symlink {
                std::os::unix::fs::symlink(target.path(), &root).unwrap();
            } else {
                std::fs::write(&root, "synthetic marker").unwrap();
            }
            assert!(temporary_root_at(state.path()).is_err());
            assert!(target.path().exists());
        }
    }
}
