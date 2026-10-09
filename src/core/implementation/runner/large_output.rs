use super::Runner;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};

const STDERR_TAIL_BYTES: usize = 32 * 1024;

pub(super) fn git_to_file(
    runner: &Runner,
    cwd: &Path,
    args: &[&str],
    output: &Path,
) -> anyhow::Result<()> {
    git_to_file_with_limit(runner, cwd, args, output, None)
}

pub(super) fn git_to_file_bounded(
    runner: &Runner,
    cwd: &Path,
    args: &[&str],
    output: &Path,
    max_bytes: u64,
) -> anyhow::Result<()> {
    git_to_file_with_limit(runner, cwd, args, output, Some(max_bytes))
}

fn git_to_file_with_limit(
    runner: &Runner,
    cwd: &Path,
    args: &[&str],
    output: &Path,
    max_bytes: Option<u64>,
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
    let output_file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(output)?;
    let mut child = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Git stdout pipe was not created"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("Git stderr pipe was not created"))?;
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let mut stdout_thread = Some(thread::spawn(move || {
        let result = copy_stdout_with_limit(stdout, output_file, max_bytes);
        let _ = stdout_tx.send(result);
    }));
    let stderr_thread = thread::spawn(move || capture_stderr_tail(stderr));

    let mut stdout_result: Option<anyhow::Result<()>> = None;
    loop {
        if let Err(error) = runner.remaining() {
            stop_child(&mut child);
            let _ = stdout_thread.take().unwrap().join();
            let _ = stderr_thread.join();
            return Err(error);
        }
        if stdout_result.is_none() {
            match stdout_rx.try_recv() {
                Ok(Err(error)) => {
                    stop_child(&mut child);
                    let _ = stderr_thread.join();
                    stdout_thread
                        .take()
                        .unwrap()
                        .join()
                        .map_err(|_| anyhow::anyhow!("Git output capture thread panicked"))?;
                    return Err(error);
                }
                Ok(Ok(())) => stdout_result = Some(Ok(())),
                Err(mpsc::TryRecvError::Disconnected) => {
                    stop_child(&mut child);
                    let _ = stderr_thread.join();
                    let _ = stdout_thread.take().unwrap().join();
                    anyhow::bail!("Git output capture ended without a result");
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout_capture = if let Some(result) = stdout_result.take() {
                    stdout_thread
                        .take()
                        .unwrap()
                        .join()
                        .map_err(|_| anyhow::anyhow!("Git output capture thread panicked"))
                        .map(|()| result)
                } else {
                    Ok(join_stdout_result(
                        stdout_thread.take().unwrap(),
                        &stdout_rx,
                    ))
                };
                let stderr = stderr_thread
                    .join()
                    .map_err(|_| anyhow::anyhow!("Git error capture thread panicked"))??;
                stdout_capture??;
                if status.success() {
                    return Ok(());
                }
                anyhow::bail!(
                    "git {} failed: {}",
                    args.join(" "),
                    crate::error::redact_secrets(&String::from_utf8_lossy(&stderr))
                );
            }
            Ok(None) => thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                stop_child(&mut child);
                let _ = stdout_thread.take().unwrap().join();
                let _ = stderr_thread.join();
                return Err(error.into());
            }
        }
    }
}

fn join_stdout_result(
    worker: JoinHandle<()>,
    receiver: &Receiver<anyhow::Result<()>>,
) -> anyhow::Result<()> {
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("Git output capture thread panicked"))?;
    receiver
        .recv()
        .map_err(|_| anyhow::anyhow!("Git output capture ended without a result"))?
}

fn copy_stdout_with_limit(
    mut stdout: impl Read,
    mut output: File,
    max_bytes: Option<u64>,
) -> anyhow::Result<()> {
    let mut written = 0_u64;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read_size = max_bytes.map_or(buffer.len(), |limit| {
            limit
                .saturating_sub(written)
                .saturating_add(1)
                .min(buffer.len() as u64) as usize
        });
        let count = stdout.read(&mut buffer[..read_size])?;
        if count == 0 {
            return Ok(());
        }
        let next_size = written
            .checked_add(count as u64)
            .ok_or_else(|| anyhow::anyhow!("Git output size overflowed"))?;
        if let Some(limit) = max_bytes {
            anyhow::ensure!(
                next_size <= limit,
                "Git output exceeded its {limit}-byte capture limit"
            );
        }
        output.write_all(&buffer[..count])?;
        written = next_size;
    }
}

fn capture_stderr_tail(mut stderr: impl Read) -> anyhow::Result<Vec<u8>> {
    let mut tail = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let count = stderr.read(&mut buffer)?;
        if count == 0 {
            return Ok(tail);
        }
        tail.extend_from_slice(&buffer[..count]);
        if tail.len() > STDERR_TAIL_BYTES {
            let excess = tail.len() - STDERR_TAIL_BYTES;
            tail.drain(..excess);
        }
    }
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    };

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TemporaryDirectory(PathBuf);

    impl TemporaryDirectory {
        fn create() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "koolade-large-output-test-{}-{}",
                std::process::id(),
                NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&directory).unwrap();
            Self(directory)
        }
    }

    impl Drop for TemporaryDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn runner() -> Runner {
        let (progress, _) = mpsc::channel();
        Runner {
            gh: "gh".into(),
            runtime_config_source: None,
            deadline: std::time::Instant::now() + Duration::from_secs(10),
            cancel: Arc::new(AtomicBool::new(false)),
            progress,
        }
    }

    #[test]
    fn bounded_git_output_never_writes_past_its_limit() {
        let directory = TemporaryDirectory::create();
        let output = directory.0.join("stdout");
        let error = runner()
            .git_to_file_bounded(
                Path::new(env!("CARGO_MANIFEST_DIR")),
                &["version"],
                &output,
                4,
            )
            .unwrap_err();
        assert!(error.to_string().contains("capture limit"));
        assert!(fs::metadata(output).unwrap().len() <= 4);
    }

    #[test]
    fn bounded_git_output_keeps_complete_output_at_the_limit() {
        let directory = TemporaryDirectory::create();
        let output = directory.0.join("stdout");
        runner()
            .git_to_file_bounded(
                Path::new(env!("CARGO_MANIFEST_DIR")),
                &["rev-parse", "--is-inside-work-tree"],
                &output,
                5,
            )
            .unwrap();
        assert_eq!(fs::read(output).unwrap(), b"true\n");
    }

    #[test]
    fn joining_before_receiving_preserves_the_capture_error() {
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            sender
                .send(Err(anyhow::anyhow!("capture limit exceeded")))
                .unwrap();
        });
        let error = join_stdout_result(worker, &receiver).unwrap_err();
        assert!(error.to_string().contains("capture limit exceeded"));
    }
}
