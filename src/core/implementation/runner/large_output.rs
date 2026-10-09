use super::Runner;
use std::{
    fs::{self, OpenOptions},
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

pub(super) fn git_to_file(
    runner: &Runner,
    cwd: &Path,
    args: &[&str],
    output: &Path,
) -> anyhow::Result<()> {
    let parent = output
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Git output file has no parent"))?;
    let parent_meta = fs::symlink_metadata(parent)?;
    anyhow::ensure!(
        parent_meta.is_dir() && !parent_meta.file_type().is_symlink(),
        "Git output directory is not a real directory"
    );
    if let Ok(metadata) = fs::symlink_metadata(output) {
        anyhow::ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Git output path is not a regular file"
        );
    }
    let stdout = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(output)?;
    let mut child = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(stdout)
        .stderr(Stdio::piped())
        .spawn()?;
    loop {
        if let Err(error) = runner.remaining() {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("{error}");
        }
        match child.try_wait()? {
            Some(status) => {
                let output = child.wait_with_output()?;
                if status.success() {
                    return Ok(());
                }
                anyhow::bail!(
                    "git {} failed: {}",
                    args.join(" "),
                    crate::error::redact_secrets(&String::from_utf8_lossy(&output.stderr))
                );
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}
