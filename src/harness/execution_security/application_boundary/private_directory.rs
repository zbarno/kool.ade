use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) struct PrivateDirectory {
    pub(crate) path: PathBuf,
}

impl PrivateDirectory {
    pub(crate) fn create(prefix: &str) -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        create_private_directory(&path)?;
        if let Err(error) = verify_private_acl(&path) {
            let _ = fs::remove_dir_all(&path);
            return Err(error);
        }
        Ok(Self { path })
    }

    pub(super) fn write_json<T: serde::Serialize>(
        &self,
        name: &str,
        value: &T,
    ) -> anyhow::Result<PathBuf> {
        self.write_file(name, &serde_json::to_vec(value)?)
    }

    pub(super) fn write_file(&self, name: &str, contents: &[u8]) -> anyhow::Result<PathBuf> {
        let path = self.path_for(name)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            secure_directory_chain(&self.path, parent)?;
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        set_private_file_mode(&mut options);
        let mut file = options.open(&path)?;
        if let Err(error) = verify_private_acl(&path) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        file.write_all(contents)?;
        set_mode(&path, 0o600)?;
        if let Err(error) = verify_private_acl(&path) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        Ok(path)
    }

    pub(super) fn create_directory(&self, name: &str) -> anyhow::Result<PathBuf> {
        let path = self.path_for(name)?;
        fs::create_dir_all(&path)?;
        secure_directory_chain(&self.path, &path)?;
        Ok(path)
    }

    fn path_for(&self, name: &str) -> anyhow::Result<PathBuf> {
        let relative = Path::new(name);
        anyhow::ensure!(
            !relative.as_os_str().is_empty()
                && relative
                    .components()
                    .all(|component| matches!(component, std::path::Component::Normal(_))),
            "private path contains an invalid component"
        );
        Ok(self.path.join(relative))
    }
}

impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).create(path)?;
    Ok(())
}

#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> anyhow::Result<()> {
    fs::create_dir(path)?;
    Ok(())
}

#[cfg(unix)]
fn set_private_file_mode(options: &mut fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_private_file_mode(_options: &mut fs::OpenOptions) {}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> anyhow::Result<()> {
    Ok(())
}

fn secure_directory_chain(root: &Path, directory: &Path) -> anyhow::Result<()> {
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| anyhow::anyhow!("private directory path escaped its root"))?;
    verify_private_acl(root)?;

    let mut current = root.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(name) => current.push(name),
            _ => anyhow::bail!("private directory path contains an invalid component"),
        }
        let metadata = fs::symlink_metadata(&current)?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink() && !is_reparse_point(&metadata),
            "private directory path is not a regular directory"
        );
        set_mode(&current, 0o700)?;
        verify_private_acl(&current)?;
    }
    Ok(())
}

fn verify_private_acl(path: &Path) -> anyhow::Result<()> {
    crate::persistence::setup_tasks::verify_private_acl(path)
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}
