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
        let path = self.path.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            set_mode(parent, 0o700)?;
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        set_private_file_mode(&mut options);
        let mut file = options.open(&path)?;
        file.write_all(contents)?;
        set_mode(&path, 0o600)?;
        Ok(path)
    }

    pub(super) fn create_directory(&self, name: &str) -> anyhow::Result<PathBuf> {
        let path = self.path.join(name);
        fs::create_dir_all(&path)?;
        set_mode(&path, 0o700)?;
        Ok(path)
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
