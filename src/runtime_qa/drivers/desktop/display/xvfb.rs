use std::path::Path;

pub(super) fn require(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|_| "pinned QA image does not provide Xvfb".to_owned())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("pinned QA image Xvfb executable is unsafe".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "xvfb/tests.rs"]
mod tests;
