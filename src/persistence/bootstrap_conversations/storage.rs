use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

#[cfg(test)]
use std::cell::RefCell;

use fs2::FileExt;

#[cfg(test)]
thread_local! {
    static TEST_DIRECTORY: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub(super) fn conversation_dir() -> anyhow::Result<PathBuf> {
    #[cfg(test)]
    if let Some(directory) = TEST_DIRECTORY.with(|current| current.borrow().clone()) {
        ensure_private_directory(&directory)?;
        return Ok(directory);
    }
    // The setup task loader establishes and verifies the existing machine-local
    // state directory using its platform-specific private ACL rules.
    crate::persistence::setup_tasks::load()?;
    let root = crate::persistence::setup_tasks::machine_setup_state_dir()?;
    ensure_private_directory(&root)?;
    let directory = root.join("bootstrap-conversations");
    ensure_private_directory(&directory)?;
    Ok(directory)
}

#[cfg(test)]
pub(super) fn with_test_directory<T>(path: &Path, action: impl FnOnce() -> T) -> T {
    struct Restore(Option<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TEST_DIRECTORY.with(|current| *current.borrow_mut() = self.0.take());
        }
    }
    let previous = TEST_DIRECTORY.with(|current| current.replace(Some(path.to_owned())));
    let _restore = Restore(previous);
    action()
}

pub(super) fn ensure_private_directory(path: &Path) -> anyhow::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        anyhow::ensure!(
            metadata.is_dir() && !is_reparse_point(&metadata),
            "unsafe bootstrap conversation directory"
        );
    } else {
        match fs::create_dir(path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !is_reparse_point(&metadata),
        "unsafe bootstrap conversation directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() },
            "bootstrap conversation directory is not owned by the current user"
        );
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        let secured = fs::symlink_metadata(path)?;
        anyhow::ensure!(
            secured.uid() == unsafe { libc::geteuid() }
                && secured.permissions().mode() & 0o077 == 0,
            "bootstrap conversation directory permissions are not private"
        );
    }
    crate::persistence::setup_tasks::verify_private_acl(path)?;
    Ok(())
}

pub(super) fn lock(directory: &Path, digest: &str) -> anyhow::Result<File> {
    let path = directory.join(format!("{digest}.lock"));
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    }
    let file = options.open(&path)?;
    verify_regular_file(&path)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => return Ok(file),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(20));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                anyhow::bail!("timed out waiting for the bootstrap conversation lock")
            }
            Err(error) => return Err(error.into()),
        }
    }
}

pub(super) fn verify_regular_file(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !is_reparse_point(&metadata),
        "unsafe bootstrap conversation state entry"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        anyhow::ensure!(
            metadata.uid() == unsafe { libc::geteuid() } && metadata.nlink() == 1,
            "bootstrap conversation state file is not privately owned"
        );
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        let secured = fs::symlink_metadata(path)?;
        anyhow::ensure!(
            secured.uid() == unsafe { libc::geteuid() }
                && secured.nlink() == 1
                && secured.permissions().mode() & 0o077 == 0,
            "bootstrap conversation state file permissions are not private"
        );
    }
    crate::persistence::setup_tasks::verify_private_acl(path)?;
    Ok(())
}

pub(super) fn secure_file(path: &Path) -> anyhow::Result<()> {
    verify_regular_file(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    verify_regular_file(path)
}

fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}
