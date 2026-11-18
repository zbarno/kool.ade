//! Private per-user macOS VM data and short AF_UNIX transport paths.

use std::{
    fs::{self, DirBuilder},
    io,
    os::unix::{fs::DirBuilderExt, fs::MetadataExt, fs::PermissionsExt},
    path::{Component, Path, PathBuf},
};

const VM_STORAGE_PARTS: [&str; 2] = ["macos-vm", "v1"];
const SHORT_TRANSPORT_DIRECTORY: &str = "ku";

pub(crate) struct MacOsVmStorageV1 {
    root: PathBuf,
    sessions: PathBuf,
    overlays: PathBuf,
    transport_sessions: PathBuf,
}

impl MacOsVmStorageV1 {
    pub(crate) fn open() -> io::Result<Self> {
        let root = crate::persistence::setup_tasks::computer_use_storage_dir()
            .map_err(|_| invalid_data("Computer Use storage root is unavailable"))?;
        Self::open_at(&root, &std::env::temp_dir())
    }

    pub(super) fn open_at(app_data_root: &Path, temporary_root: &Path) -> io::Result<Self> {
        if !safe_root_path(app_data_root) || !safe_root_path(temporary_root) {
            return Err(invalid_data("macOS VM storage paths must be absolute"));
        }
        prepare_app_data_root(app_data_root)?;
        ensure_private_directory(temporary_root)?;

        let root = VM_STORAGE_PARTS
            .iter()
            .fold(app_data_root.to_path_buf(), |path, component| {
                path.join(component)
            });
        let mut current = app_data_root.to_path_buf();
        for component in VM_STORAGE_PARTS {
            current.push(component);
            create_private_directory(&current)?;
        }
        let sessions = root.join("sessions");
        let overlays = root.join("overlays");
        let transport_sessions = temporary_root.join(SHORT_TRANSPORT_DIRECTORY);
        for path in [&sessions, &overlays, &transport_sessions] {
            create_private_directory(path)?;
        }
        Ok(Self {
            root,
            sessions,
            overlays,
            transport_sessions,
        })
    }

    pub(crate) fn session_directory(&self, session_id: &str) -> io::Result<PathBuf> {
        if !canonical_session_id(session_id) {
            return Err(invalid_data("session id is not a canonical UUID"));
        }
        let path = self.sessions.join(session_id);
        create_private_directory(&path)?;
        Ok(path)
    }

    pub(crate) fn transport_session_directory(&self, session_id: &str) -> io::Result<PathBuf> {
        if !canonical_session_id(session_id) {
            return Err(invalid_data("session id is not a canonical UUID"));
        }
        let path = self.transport_sessions.join(session_id);
        create_private_directory(&path)?;
        Ok(path)
    }

    pub(crate) fn guest_control_socket_path(&self, session_id: &str) -> io::Result<PathBuf> {
        Ok(self
            .transport_session_directory(session_id)?
            .join("guest-control.sock"))
    }

    pub(crate) fn display_socket_path(&self, session_id: &str) -> io::Result<PathBuf> {
        Ok(self
            .transport_session_directory(session_id)?
            .join("display.sock"))
    }

    pub(crate) fn overlay_root(&self) -> &Path {
        &self.overlays
    }

    #[cfg(test)]
    fn storage_root(&self) -> &Path {
        &self.root
    }
}

fn create_private_directory(path: &Path) -> io::Result<()> {
    match DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    ensure_private_directory(path)
}

fn prepare_app_data_root(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
                return Err(invalid_data("VM storage root is not owned by this user"));
            }
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            ensure_private_directory(path)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(path)?;
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.file_type().is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
                return Err(invalid_data("VM storage root is not owned by this user"));
            }
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            ensure_private_directory(path)
        }
        Err(error) => Err(error),
    }
}

fn ensure_private_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "VM storage directory {:?} is not private (owner {}, current user {}, mode {:o})",
                path,
                metadata.uid(),
                unsafe { libc::geteuid() },
                metadata.permissions().mode() & 0o777,
            ),
        ));
    }
    Ok(())
}

fn canonical_session_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|uuid| uuid.hyphenated().to_string() == value)
}

fn safe_root_path(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use std::{os::unix::fs::PermissionsExt, path::Path};

    use super::*;

    const SESSION_ID: &str = "123e4567-e89b-12d3-a456-426614174000";

    fn private_tempdir() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("temporary directory fixture");
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
            .expect("make fixture directory private");
        directory
    }

    fn private_mode(path: &Path) -> u32 {
        fs::symlink_metadata(path)
            .expect("directory metadata")
            .permissions()
            .mode()
            & 0o777
    }

    #[test]
    fn storage_and_session_directories_are_owned_private_and_transport_paths_are_short() {
        let app_data = private_tempdir();
        let temporary = private_tempdir();
        let storage = MacOsVmStorageV1::open_at(app_data.path(), temporary.path())
            .expect("private macOS storage");
        let session = storage
            .session_directory(SESSION_ID)
            .expect("private session directory");
        let socket = storage
            .guest_control_socket_path(SESSION_ID)
            .expect("short guest control socket");
        let display = storage
            .display_socket_path(SESSION_ID)
            .expect("short display socket");

        for path in [
            storage.storage_root(),
            storage.overlay_root(),
            &session,
            socket.parent().expect("transport session directory"),
            display.parent().expect("transport session directory"),
        ] {
            assert_eq!(private_mode(path), 0o700, "private directory mode");
            assert!(fs::symlink_metadata(path).unwrap().file_type().is_dir());
        }
        assert!(socket.as_os_str().as_encoded_bytes().len() <= 103);
        assert!(display.as_os_str().as_encoded_bytes().len() <= 103);
        assert!(socket.starts_with(temporary.path()));
        assert!(!socket.starts_with(app_data.path()));
    }

    #[test]
    fn storage_rejects_noncanonical_ids_and_symlinked_session_directories() {
        let app_data = private_tempdir();
        let temporary = private_tempdir();
        let storage = MacOsVmStorageV1::open_at(app_data.path(), temporary.path())
            .expect("private macOS storage");
        assert!(storage.session_directory("../session").is_err());

        let target = private_tempdir();
        let session_link = app_data
            .path()
            .join("macos-vm/v1/sessions")
            .join(SESSION_ID);
        std::os::unix::fs::symlink(target.path(), &session_link)
            .expect("create substituted session symlink");
        assert!(storage.session_directory(SESSION_ID).is_err());
    }

    #[test]
    fn storage_repairs_group_or_world_access_on_owned_app_data_root() {
        let app_data = private_tempdir();
        let temporary = private_tempdir();
        fs::set_permissions(app_data.path(), fs::Permissions::from_mode(0o755))
            .expect("make app data fixture permissive");
        MacOsVmStorageV1::open_at(app_data.path(), temporary.path())
            .expect("restrict owned app data root");
        assert_eq!(private_mode(app_data.path()), 0o700);
    }

    #[test]
    fn storage_rejects_relative_and_traversing_roots() {
        let app_data = private_tempdir();
        let temporary = private_tempdir();
        assert!(MacOsVmStorageV1::open_at(Path::new("relative"), temporary.path()).is_err());
        assert!(
            MacOsVmStorageV1::open_at(&app_data.path().join("../outside"), temporary.path())
                .is_err()
        );
    }
}
