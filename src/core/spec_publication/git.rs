use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use super::MAX_OUTPUT_BYTES;
use super::github::PublicCommitIdentity;

pub(super) fn ensure_existing_path_is_safe(
    repo: &Path,
    commit: &str,
    path: &str,
) -> anyhow::Result<()> {
    let components = path.split('/').collect::<Vec<_>>();
    for index in 1..components.len() {
        let prefix = components[..index].join("/");
        let output = git_at(repo, &["ls-tree", "-z", commit, "--", &prefix])?;
        if output.is_empty() {
            continue;
        }
        let (mode, kind, _) = parse_tree_entry(output.as_bytes())?;
        anyhow::ensure!(
            kind == "tree" && mode == "040000",
            "publication path contains a non-directory or symbolic-link parent"
        );
    }
    let output = git_at(repo, &["ls-tree", "-z", commit, "--", path])?;
    if output.is_empty() {
        return Ok(());
    }
    let (mode, kind, found_path) = parse_tree_entry(output.as_bytes())?;
    anyhow::ensure!(
        found_path == path && kind == "blob" && mode == "100644",
        "publication may replace only an existing regular Markdown file"
    );
    Ok(())
}

pub(super) fn parse_tree_entry(bytes: &[u8]) -> anyhow::Result<(String, String, String)> {
    let entry = bytes
        .split(|byte| *byte == 0)
        .find(|entry| !entry.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Git tree entry is malformed"))?;
    let separator = entry
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| anyhow::anyhow!("Git tree entry is malformed"))?;
    let (metadata, path_with_tab) = entry.split_at(separator);
    let path = &path_with_tab[1..];
    let mut fields = std::str::from_utf8(metadata)?.split_ascii_whitespace();
    let mode = fields.next().unwrap_or_default().to_owned();
    let kind = fields.next().unwrap_or_default().to_owned();
    let path = std::str::from_utf8(path)?.to_owned();
    Ok((mode, kind, path))
}

pub(super) fn git_commit_tree(
    repo: &Path,
    tree: &str,
    parent: &str,
    identity: &PublicCommitIdentity,
) -> anyhow::Result<String> {
    let mut command = git_command(repo)?;
    command
        .args([
            "commit-tree",
            tree,
            "-p",
            parent,
            "-m",
            "Publish approved Kool.ad/e documentation",
        ])
        .env("GIT_AUTHOR_NAME", &identity.name)
        .env("GIT_AUTHOR_EMAIL", &identity.email)
        .env("GIT_COMMITTER_NAME", &identity.name)
        .env("GIT_COMMITTER_EMAIL", &identity.email);
    checked_stdout(command, "could not create documentation commit")
}

pub(super) fn git_input_at(repo: &Path, args: &[&str], input: &[u8]) -> anyhow::Result<String> {
    let mut command = git_command(repo)?;
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(input)?;
    let output = child.wait_with_output()?;
    checked_output(output, "Git could not prepare a documentation blob")
}

pub(super) fn git_input_at_env(
    repo: &Path,
    args: &[&str],
    input: &[u8],
    index: Option<&PathBuf>,
) -> anyhow::Result<String> {
    let mut command = git_command(repo)?;
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let mut child = command.spawn()?;
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(input)?;
    let output = child.wait_with_output()?;
    checked_output(output, "Git publication operation failed")
}

pub(super) fn git_at(repo: &Path, args: &[&str]) -> anyhow::Result<String> {
    let mut command = git_command(repo)?;
    command.args(args);
    checked_stdout(command, "Git publication operation failed")
}

pub(super) fn git_at_env(
    repo: &Path,
    args: &[&str],
    index: Option<&PathBuf>,
) -> anyhow::Result<String> {
    let mut command = git_command(repo)?;
    command.args(args);
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    checked_stdout(command, "Git publication operation failed")
}

pub(super) fn git_standalone(args: &[&str]) -> anyhow::Result<String> {
    let mut command = git_standalone_command();
    command.args(args);
    checked_stdout(command, "Git publication setup failed")
}

pub(super) fn git_standalone_output(args: &[&str]) -> anyhow::Result<Output> {
    Ok(git_standalone_command().args(args).output()?)
}

pub(super) fn git_command(repo: &Path) -> anyhow::Result<Command> {
    crate::core::git_commands::validated_git_command(repo)
}

pub(super) fn git_standalone_command() -> Command {
    crate::core::git_commands::standalone_git_command()
}

pub(super) fn gh_output(gh: &std::ffi::OsStr, cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let mut command = Command::new(gh);
    command.current_dir(cwd).args(args).env_clear();
    for name in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "LANG",
        "LC_ALL",
        "SYSTEMROOT",
        "WINDIR",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GH_HOST",
        "GH_CONFIG_DIR",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.env("GH_PROMPT_DISABLED", "1");
    let output = run_bounded(&mut command)?;
    checked_output(
        output,
        "GitHub CLI could not create the documentation pull request",
    )
}

pub(super) fn run_bounded(command: &mut Command) -> anyhow::Result<Output> {
    const TIMEOUT: Duration = Duration::from_secs(45);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let stdout_reader = std::thread::spawn(move || read_limited(stdout));
    let stderr_reader = std::thread::spawn(move || read_limited(stderr));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("GitHub CLI timed out during documentation publication");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| anyhow::anyhow!("GitHub CLI stdout reader failed"))??;
    let stderr = stderr_reader
        .join()
        .map_err(|_| anyhow::anyhow!("GitHub CLI stderr reader failed"))??;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

pub(super) fn read_limited(reader: impl std::io::Read) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub(super) fn checked_stdout(mut command: Command, detail: &str) -> anyhow::Result<String> {
    let output = command.output()?;
    checked_output(output, detail)
}

pub(super) fn checked_output(output: Output, detail: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        output.stdout.len() <= MAX_OUTPUT_BYTES && output.stderr.len() <= MAX_OUTPUT_BYTES,
        "publication command returned too much output"
    );
    if !output.status.success() {
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        let detail = if diagnostic.trim().is_empty() {
            detail.to_owned()
        } else {
            format!("{detail}: {}", diagnostic.trim())
        };
        anyhow::bail!("{}", crate::error::redact_secrets(&detail));
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim().to_owned())
        .map_err(|_| anyhow::anyhow!("publication command returned non-UTF-8 output"))
}

pub(super) fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
