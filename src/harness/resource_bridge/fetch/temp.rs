use std::{fs, path::PathBuf};

pub(super) struct TempResponse {
    pub(super) directory: PathBuf,
    pub(super) headers: PathBuf,
    pub(super) body: PathBuf,
}

impl TempResponse {
    pub(super) fn new() -> anyhow::Result<Self> {
        let directory = std::env::temp_dir().join(format!(
            "koolade-resource-response-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        #[cfg(windows)]
        super::super::set_private_dir(&directory)?;
        Ok(Self {
            headers: directory.join("headers"),
            body: directory.join("body"),
            directory,
        })
    }
}

impl Drop for TempResponse {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
