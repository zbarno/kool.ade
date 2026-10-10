use std::{
    io,
    path::{Component, Path, PathBuf},
};

use super::{PlanningStore, StoreError, StoreFile};

impl PlanningStore {
    pub fn read(&self, relative: impl AsRef<Path>) -> Result<Vec<u8>, StoreError> {
        let path = self.resolve(relative.as_ref())?;
        std::fs::read(&path).map_err(|source| StoreError::Io { path, source })
    }

    pub fn read_planning_path(&self, path: &Path) -> Result<Vec<u8>, StoreError> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| StoreError::InvalidPath(path.display().to_string()))?;
        self.read(relative)
    }

    /// List regular files in one store directory without following symlinks.
    /// Missing directories are empty; a symlink anywhere in the requested
    /// path or among its direct entries is rejected.
    pub fn list_files(&self, relative: impl AsRef<Path>) -> Result<Vec<StoreFile>, StoreError> {
        let path = self.resolve(relative.as_ref())?;
        let entries = match std::fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(StoreError::Io { path, source });
            }
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
            let entry_path = entry.path();
            let file_type = entry.file_type().map_err(|source| StoreError::Io {
                path: entry_path.clone(),
                source,
            })?;
            if file_type.is_symlink() {
                return Err(StoreError::InvalidPath(entry_path.display().to_string()));
            }
            if !file_type.is_file() {
                continue;
            }
            let metadata =
                std::fs::symlink_metadata(&entry_path).map_err(|source| StoreError::Io {
                    path: entry_path.clone(),
                    source,
                })?;
            if metadata.file_type().is_symlink() {
                return Err(StoreError::InvalidPath(entry_path.display().to_string()));
            }
            files.push(StoreFile {
                name: entry.file_name().to_string_lossy().into_owned(),
                bytes: metadata.len(),
            });
        }
        files.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(files)
    }

    pub fn atomic_write(&self, relative: impl AsRef<Path>, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.resolve(relative.as_ref())?;
        crate::artifacts::atomic_write_bytes(&path, bytes).map_err(|source| StoreError::Io {
            path,
            source: io::Error::other(source),
        })
    }

    pub fn write_planning_path(&self, path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| StoreError::InvalidPath(path.display().to_string()))?;
        self.atomic_write(relative, bytes)
    }

    pub fn atomic_create(
        &self,
        relative: impl AsRef<Path>,
        bytes: &[u8],
    ) -> Result<(), StoreError> {
        let path = self.resolve(relative.as_ref())?;
        crate::artifacts::atomic_create_bytes(&path, bytes)
            .map_err(|source| StoreError::Io { path, source })
    }

    pub fn atomic_copy_new(
        &self,
        relative: impl AsRef<Path>,
        source: &Path,
    ) -> Result<(), StoreError> {
        let path = self.resolve(relative.as_ref())?;
        crate::artifacts::atomic_copy_new(source, &path)
            .map_err(|source| StoreError::Io { path, source })
    }

    pub fn remove(&self, relative: impl AsRef<Path>) -> Result<bool, StoreError> {
        let path = self.resolve(relative.as_ref())?;
        match std::fs::remove_file(&path) {
            Ok(()) => {
                crate::artifacts::sync_parent_directory(&path)
                    .map_err(|source| StoreError::Io { path, source })?;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(StoreError::Io { path, source }),
        }
    }

    pub(super) fn validate_root(&self) -> Result<(), StoreError> {
        match std::fs::symlink_metadata(&self.root) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                Err(StoreError::InvalidPath(self.root.display().to_string()))
            }
            Ok(_) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(StoreError::Io {
                path: self.root.clone(),
                source,
            }),
        }
    }

    pub(crate) fn resolve(&self, relative: &Path) -> Result<PathBuf, StoreError> {
        let display = relative.to_string_lossy().into_owned();
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || display.contains('\\')
            || display.contains(':')
            || !relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
            || is_local_only(relative)
        {
            return Err(StoreError::InvalidPath(display));
        }

        self.validate_root()?;

        let mut candidate = self.root.clone();
        for part in relative.components() {
            candidate.push(part.as_os_str());
            match std::fs::symlink_metadata(&candidate) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(StoreError::InvalidPath(display));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(StoreError::Io {
                        path: candidate,
                        source,
                    });
                }
            }
        }
        Ok(candidate)
    }
}

fn is_local_only(relative: &Path) -> bool {
    relative == Path::new("config/mcp.json")
        || relative.starts_with("implementation")
        || relative == Path::new("state/time-ledger.log")
}
