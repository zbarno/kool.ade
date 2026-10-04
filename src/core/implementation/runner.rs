use super::*;

pub(super) struct Runner {
    pub(super) gh: String,
    pub(super) deadline: Instant,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) progress: Sender<LiveProgress>,
}
impl Runner {
    pub(super) fn remaining(&self) -> anyhow::Result<Duration> {
        anyhow::ensure!(
            !self.cancel.load(Ordering::SeqCst),
            "Implementation cancelled. The worktree is preserved; choose Resume implementation to continue."
        );
        self.deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Implementation budget expired. The worktree is preserved for resume."
                )
            })
    }
    pub(super) fn update(&self, text: impl Into<String>) {
        let _ = self.progress.send(LiveProgress {
            activity: Some(text.into()),
            ..Default::default()
        });
    }
    pub(super) fn command(
        &self,
        cwd: &Path,
        program: &str,
        args: &[&str],
    ) -> anyhow::Result<String> {
        self.command_with_env_policy(cwd, program, args, false)
    }
    pub(super) fn command_clean_env(
        &self,
        cwd: &Path,
        program: &str,
        args: &[&str],
    ) -> anyhow::Result<String> {
        self.command_with_env_policy(cwd, program, args, true)
    }
    pub(super) fn command_with_env_policy(
        &self,
        cwd: &Path,
        program: &str,
        args: &[&str],
        clean_env: bool,
    ) -> anyhow::Result<String> {
        self.remaining()?;
        let argv = std::iter::once(program.to_owned())
            .chain(args.iter().map(|s| s.to_string()))
            .collect::<Vec<_>>();
        let child = if clean_env {
            crate::harness::pi_proc::spawn_with_input_clear_env(&argv, cwd, None, &[])?
        } else {
            crate::harness::pi_proc::spawn(&argv, cwd)?
        };
        let mut output = String::new();
        let mut error = String::new();
        loop {
            if let Err(e) = self.remaining() {
                child.kill();
                let detail = crate::error::redact_secrets(&format!(
                    "{e}\nCommand: {program} {}\nWorking directory: {}\nstdout:\n{output}\nstderr:\n{error}",
                    args.join(" "),
                    cwd.display()
                ));
                anyhow::bail!("{detail}");
            }
            use crate::harness::pi_proc::{PollState, StreamEvt};
            match child.poll_next(Duration::from_millis(100)) {
                Ok(StreamEvt::Stdout(line)) => append_tail(&mut output, &line),
                Ok(StreamEvt::Stderr(line)) => append_tail(&mut error, &line),
                Ok(StreamEvt::Exited(ok)) => {
                    if !ok {
                        let detail = crate::error::redact_secrets(&format!(
                            "{program} failed:\nstdout:\n{output}\nstderr:\n{error}"
                        ));
                        anyhow::bail!("{detail}");
                    }
                    return Ok(output.trim().into());
                }
                Err(PollState::Closed) => anyhow::bail!("{program} closed without an exit result"),
                Err(PollState::Pending) => {}
            }
        }
    }
    pub(super) fn check_storage(&self, _cwd: &Path) -> anyhow::Result<()> {
        #[cfg(unix)]
        {
            let existing = _cwd.ancestors().find(|path| path.is_dir()).ok_or_else(|| {
                anyhow::anyhow!("Cannot locate filesystem for {}", _cwd.display())
            })?;
            let output = self.command(existing, "df", &["-Pk", "."])?;
            let available = output
                .lines()
                .last()
                .and_then(|line| line.split_whitespace().nth(3))
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Cannot determine available disk space for {}",
                        _cwd.display()
                    )
                })?;
            anyhow::ensure!(
                available >= 1024 * 1024,
                "Insufficient disk space at {}: {} MiB available; at least 1 GiB is required to start implementation or verification. Free rebuildable build caches, then Resume implementation. Existing work is preserved.",
                _cwd.display(),
                available / 1024
            );
        }
        Ok(())
    }
    pub(super) fn verify(&self, cwd: &Path, command: &str) -> anyhow::Result<String> {
        self.check_storage(cwd)?;
        cwd.to_str()
            .ok_or_else(|| anyhow::anyhow!("Non-UTF8 worktree path"))?;
        let mut sandbox = crate::harness::pi_sandbox::Sandbox::new(cwd)?;
        let npm_cache = crate::harness::prepared_npm_cache_path()?;
        sandbox.mount_npm_cache(&npm_cache, false)?;
        let args = sandbox.command_args("/bin/sh", command);
        let args = args.iter().map(String::as_str).collect::<Vec<_>>();
        self.command_clean_env(
            cwd,
            sandbox
                .bwrap
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Non-UTF8 bubblewrap path"))?,
            &args,
        )
    }
    pub(super) fn git(&self, cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
        let retries = if matches!(args.first(), Some(&"fetch" | &"ls-remote")) {
            3
        } else {
            1
        };
        let mut last = None;
        for attempt in 0..retries {
            match self.command(cwd, "git", args) {
                Ok(output) => return Ok(output),
                Err(error) => {
                    self.remaining()?;
                    last = Some(error);
                }
            }
            if attempt + 1 < retries {
                self.update("Retrying temporary Git connection failure…");
            }
        }
        Err(last.unwrap())
    }

    pub(super) fn merge_base(
        &self,
        cwd: &Path,
        left: &str,
        right: &str,
    ) -> anyhow::Result<Option<String>> {
        self.remaining()?;
        let child = crate::harness::pi_proc::spawn(
            &["git".into(), "merge-base".into(), left.into(), right.into()],
            cwd,
        )?;
        let mut stdout = String::new();
        let mut stderr = String::new();
        loop {
            if let Err(error) = self.remaining() {
                child.kill();
                anyhow::bail!("{error}");
            }
            use crate::harness::pi_proc::{PollState, StreamEvt};
            match child.poll_next(Duration::from_millis(100)) {
                Ok(StreamEvt::Stdout(line)) => append_tail(&mut stdout, &line),
                Ok(StreamEvt::Stderr(line)) => append_tail(&mut stderr, &line),
                Ok(StreamEvt::Exited(true)) => return Ok(Some(stdout.trim().into())),
                Ok(StreamEvt::Exited(false)) => {
                    if child.exit_code() == Some(1)
                        && stdout.trim().is_empty()
                        && (stderr.trim().is_empty() || stderr.contains("no merge base"))
                    {
                        return Ok(None);
                    }
                    anyhow::bail!(
                        "git merge-base failed with exit code {:?}:\nstdout:\n{stdout}\nstderr:\n{stderr}",
                        child.exit_code()
                    );
                }
                Err(PollState::Closed) => {
                    anyhow::bail!("git merge-base closed without an exit result")
                }
                Err(PollState::Pending) => {}
            }
        }
    }
}
pub(super) fn append_tail(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
    if out.len() > 32_000 {
        let boundary = out
            .char_indices()
            .map(|(i, _)| i)
            .find(|i| *i >= out.len() - 24_000)
            .unwrap_or(0);
        out.drain(..boundary);
    }
}
