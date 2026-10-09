use super::Runner;
use std::{
    fs::{self, DirBuilder},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_CAPTURE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_RECORDS: usize = 100_000;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

struct TemporaryOutput {
    directory: PathBuf,
    output: PathBuf,
}

impl TemporaryOutput {
    fn create() -> anyhow::Result<Self> {
        let directory = std::env::temp_dir();
        let metadata = fs::symlink_metadata(&directory)?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Temporary output directory is not a real directory"
        );
        for _ in 0..32 {
            let sequence = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
            let private_directory = directory.join(format!(
                ".koolade-git-records-{}-{sequence:016x}",
                std::process::id()
            ));
            match create_private_directory(&private_directory) {
                Ok(()) => {
                    return Ok(Self {
                        output: private_directory.join("stdout"),
                        directory: private_directory,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        anyhow::bail!("Could not allocate a temporary Git inventory file")
    }
}

impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = DirBuilder::new();
    builder.mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> std::io::Result<()> {
    DirBuilder::new().create(path)
}

pub(super) fn capture(runner: &Runner, cwd: &Path, args: &[&str]) -> anyhow::Result<Vec<String>> {
    parse(&capture_bytes(runner, cwd, args, MAX_CAPTURE_BYTES)?)
}

pub(super) fn capture_bytes(
    runner: &Runner,
    cwd: &Path,
    args: &[&str],
    max_bytes: u64,
) -> anyhow::Result<Vec<u8>> {
    let temporary = TemporaryOutput::create()?;
    runner.git_to_file_bounded(cwd, args, &temporary.output, max_bytes)?;
    let metadata = fs::metadata(&temporary.output)?;
    anyhow::ensure!(
        metadata.len() <= max_bytes,
        "Git output exceeded its bounded capture limit"
    );
    Ok(fs::read(&temporary.output)?)
}

fn parse(bytes: &[u8]) -> anyhow::Result<Vec<String>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    anyhow::ensure!(
        bytes.last() == Some(&0),
        "Git NUL-delimited inventory is incomplete"
    );
    let mut records = Vec::new();
    for bytes in bytes[..bytes.len() - 1].split(|byte| *byte == 0) {
        anyhow::ensure!(
            !bytes.is_empty(),
            "Git NUL-delimited inventory contains an empty record"
        );
        anyhow::ensure!(
            records.len() < MAX_RECORDS,
            "Git NUL-delimited inventory exceeded the 100,000 record limit"
        );
        records.push(
            std::str::from_utf8(bytes)
                .map_err(|_| anyhow::anyhow!("Git inventory contains a non-UTF8 path"))?
                .to_owned(),
        );
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nul_records_preserve_embedded_newlines_and_tabs() {
        assert_eq!(
            parse(b"regular\0name\twith\ncharacters\0").unwrap(),
            ["regular", "name\twith\ncharacters"]
        );
    }

    #[test]
    fn nul_records_reject_truncation_and_non_utf8_paths() {
        assert!(parse(b"truncated").is_err());
        assert!(parse(b"bad\xffpath\0").is_err());
    }

    #[test]
    fn nul_records_enforce_record_count_limit() {
        let mut bytes = Vec::with_capacity((MAX_RECORDS + 1) * 2);
        for _ in 0..=MAX_RECORDS {
            bytes.extend_from_slice(b"x\0");
        }
        assert!(parse(&bytes).is_err());
    }
}
