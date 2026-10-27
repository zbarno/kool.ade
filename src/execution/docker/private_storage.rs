use std::path::Path;

pub(crate) fn verify_private_root(path: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        crate::persistence::setup_tasks::verify_private_acl(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Ok(())
    }
}
