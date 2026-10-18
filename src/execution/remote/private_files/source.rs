//! Read private bytes only after the persisted receipt and source hash agree.

use std::{fs, io::Read, path::Path};

use super::RemotePrivateFileGrantV1;
use crate::execution::remote::WorkspaceManifestV1;
use zeroize::{Zeroize, Zeroizing};

mod discovery;
pub(crate) use discovery::discover_private_files;

/// Secret-bearing data deliberately has no `Debug` implementation.
pub(crate) struct AuthorizedPrivateFileBytes(Vec<u8>);

impl AuthorizedPrivateFileBytes {
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for AuthorizedPrivateFileBytes {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Validate durable authority and read one exact file without following a
/// symlink. The returned bytes are not logged or persisted by this function.
pub(crate) fn read_authorized_private_file(
    grants_root: &Path,
    source_root: &Path,
    grant: &RemotePrivateFileGrantV1,
    relative_path: &str,
    snapshot: &WorkspaceManifestV1,
    now_unix_ms: u64,
) -> anyhow::Result<AuthorizedPrivateFileBytes> {
    let persisted = crate::persistence::remote_private_file_grants::require_authorized(
        grants_root,
        grant,
        now_unix_ms,
    )?;
    snapshot.validate()?;
    anyhow::ensure!(snapshot.workspace == persisted.workspace);
    crate::execution::remote::workspace::verify_private_file_source(
        &persisted.workspace,
        source_root,
    )?;
    persisted.validate_against_exclusions(&snapshot.excluded_paths)?;
    let spec = persisted
        .files
        .iter()
        .find(|file| file.path == relative_path)
        .ok_or_else(|| anyhow::anyhow!("private file is outside its grant"))?;
    let file = open_source_file(source_root, &spec.path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() == spec.size_bytes,
        "private file changed after confirmation"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(metadata.nlink() == 1 && metadata.mode() & 0o6000 == 0);
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(spec.size_bytes as usize));
    file.take(spec.size_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    persisted.validate_complete_file(relative_path, &bytes)?;
    Ok(AuthorizedPrivateFileBytes(std::mem::take(&mut *bytes)))
}

#[cfg(unix)]
pub(super) fn open_source_file(root: &Path, relative_path: &str) -> anyhow::Result<fs::File> {
    use std::{
        ffi::CString,
        os::fd::{AsRawFd, FromRawFd},
        os::unix::ffi::OsStrExt,
    };

    super::validate_private_path(relative_path)?;
    anyhow::ensure!(root.is_absolute(), "private source root must be absolute");
    let mut directory = fs::OpenOptions::new().read(true).open("/")?;
    for component in root.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(name) => {
                let name = CString::new(name.as_bytes())?;
                let fd = unsafe {
                    libc::openat(
                        directory.as_raw_fd(),
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY,
                    )
                };
                anyhow::ensure!(
                    fd >= 0,
                    "private source root contains a symlink or is unavailable"
                );
                directory = unsafe { fs::File::from_raw_fd(fd) };
            }
            _ => anyhow::bail!("private source root is not canonical"),
        }
    }
    let components = relative_path.split('/').collect::<Vec<_>>();
    for component in &components[..components.len() - 1] {
        let name = CString::new(component.as_bytes())?;
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY,
            )
        };
        anyhow::ensure!(
            fd >= 0,
            "private source path contains a symlink or is unavailable"
        );
        directory = unsafe { fs::File::from_raw_fd(fd) };
    }
    let name = CString::new(components[components.len() - 1].as_bytes())?;
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    anyhow::ensure!(fd >= 0, "private source file is a symlink or unavailable");
    Ok(unsafe { fs::File::from_raw_fd(fd) })
}

#[cfg(not(unix))]
fn open_source_file(_root: &Path, _relative_path: &str) -> anyhow::Result<fs::File> {
    anyhow::bail!("private source transfer is unavailable without secure handle-relative reads")
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;
