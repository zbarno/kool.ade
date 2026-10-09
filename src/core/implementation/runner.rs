use super::*;
#[cfg(test)]
mod git_trace;
mod large_output;
mod verification;
mod verification_command;
#[cfg(test)]
pub(super) use git_trace::capture_git_worktree_commands;

pub(super) struct Runner {
    pub(super) gh: String,
    pub(super) runtime_config_source: Option<std::path::PathBuf>,
    pub(super) deadline: Instant,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) progress: Sender<LiveProgress>,
}
impl Runner {
    pub(super) fn remaining(&self) -> anyhow::Result<Duration> {
        anyhow::ensure!(
            !self.cancel.load(Ordering::SeqCst),
            "Implementation cancelled. The task repository is preserved; choose Resume implementation to continue."
        );
        self.deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Implementation budget expired. The task repository is preserved for resume."
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
    pub(super) fn git(&self, cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
        #[cfg(test)]
        git_trace::record(args);
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

    pub(super) fn git_to_file(
        &self,
        cwd: &Path,
        args: &[&str],
        output: &Path,
    ) -> anyhow::Result<()> {
        #[cfg(test)]
        git_trace::record(args);
        large_output::git_to_file(self, cwd, args, output)
    }

    /// Some Git operations write reflogs even though they do not create a
    /// commit. Keep them from resolving a fallback email through the hostname.
    pub(super) fn git_with_reflog_identity(
        &self,
        cwd: &Path,
        args: &[&str],
    ) -> anyhow::Result<String> {
        let mut git_args = vec![
            "-c",
            "user.name=Kool.ad/e task repository",
            "-c",
            "user.email=task-repository@koolade.invalid",
        ];
        git_args.extend_from_slice(args);
        self.git(cwd, &git_args)
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
