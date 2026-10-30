use std::{
    path::Path,
    process::{Command, Output},
};

const REJECTED_LOCAL_CONFIG: &[&str] = &[
    "include.",
    "core.hookspath",
    "core.sshcommand",
    "core.askpass",
    "core.fsmonitor",
    "core.worktree",
    "core.gitproxy",
    "gpg.",
    "extensions.worktreeconfig",
    "diff.external",
    "filter.",
    "protocol.",
    "url.",
];

pub(in crate::core::planning_sync) fn output(repo: &Path, args: &[&str]) -> anyhow::Result<Output> {
    audit_local_config(repo)?;
    Ok(safe_command(Some(repo)).args(args).output()?)
}

pub(super) fn safe_command(repo: Option<&Path>) -> Command {
    let mut command = Command::new("git");
    if let Some(repo) = repo {
        command.arg("-C").arg(repo);
    }
    command
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_EXEC_PATH")
        .env_remove("GIT_PROXY_COMMAND")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_SYSTEM")
        .env_remove("GIT_CEILING_DIRECTORIES")
        .env_remove("GIT_DISCOVERY_ACROSS_FILESYSTEM")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_PREFIX")
        .env_remove("GIT_IMPLICIT_WORK_TREE")
        .env_remove("GIT_SHALLOW_FILE")
        .env_remove("GIT_REPLACE_REF_BASE")
        .env_remove("GIT_QUARANTINE_PATH")
        .env("GIT_ALLOW_PROTOCOL", "http:https:ssh:git:file")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_TRACE")
        .env_remove("GIT_TRACE2")
        .env_remove("GIT_TRACE2_EVENT")
        .env_remove("GIT_TRACE2_PERF")
        .env_remove("GIT_TRACE2_CONFIG_PARAMS")
        .env_remove("GIT_TRACE2_PARENT_SID")
        .env_remove("GIT_TRACE2_PERF_BRIEF")
        .env_remove("GIT_TRACE2_EVENT_NESTING")
        .env_remove("GIT_TRACE2_ENV_VARS")
        .env_remove("GIT_TRACE2_DST_DEBUG")
        .env_remove("GIT_TRACE_CURL")
        .env_remove("GIT_TRACE_CURL_REDIRECT")
        .env_remove("GIT_TRACE_CURL_NO_DATA")
        .env_remove("GIT_TRACE_PACK_ACCESS")
        .env_remove("GIT_TRACE_PACKFILE")
        .env_remove("GIT_TRACE_SHALLOW")
        .env_remove("GIT_TRACE_FSMONITOR")
        .env_remove("GIT_TRACE_REFS")
        .env_remove("GIT_TRACE2")
        .env_remove("GIT_TRACE_REDACT")
        .env_remove("GIT_CURL_VERBOSE")
        .env_remove("GIT_TRACE_SETUP")
        .env_remove("GIT_TRACE_PACKET")
        .env_remove("GIT_TRACE_PERFORMANCE")
        .env_remove("GIT_TRACE2_CHILD_EXIT");
    command
}

pub(crate) fn validated_command(repo: &Path) -> anyhow::Result<Command> {
    let check = safe_command(Some(repo))
        .args(["rev-parse", "--git-dir"])
        .output()?;
    if check.status.success() {
        audit_local_config(repo)?;
    }
    Ok(safe_command(Some(repo)))
}

pub(crate) fn standalone_command() -> Command {
    safe_command(None)
}

pub(super) fn audit_local_config(repo: &Path) -> anyhow::Result<()> {
    let result = safe_command(Some(repo))
        .args([
            "config",
            "--local",
            "--name-only",
            "--null",
            "--list",
            "--no-includes",
        ])
        .output()?;
    anyhow::ensure!(
        result.status.success(),
        "could not inspect planning Git configuration"
    );
    for key in result
        .stdout
        .split(|byte| *byte == 0)
        .filter(|key| !key.is_empty())
    {
        let key = String::from_utf8_lossy(key).to_ascii_lowercase();
        anyhow::ensure!(
            !(key.starts_with("credential.") && key.ends_with(".helper"))
                && !key.starts_with("includeif.")
                && !(key.starts_with("diff.")
                    && (key.ends_with(".command") || key.ends_with(".textconv")))
                && !(key.starts_with("remote.")
                    && [".vcs", ".proxy", ".uploadpack", ".receivepack", ".mirror"]
                        .iter()
                        .any(|suffix| key.ends_with(suffix)))
                && !key.starts_with("http.")
                && !REJECTED_LOCAL_CONFIG
                    .iter()
                    .any(|prefix| key.starts_with(prefix)),
            "planning repository contains executable or redirecting Git configuration"
        );
    }
    let hooks = safe_command(Some(repo))
        .args(["rev-parse", "--git-path", "hooks"])
        .output()?;
    anyhow::ensure!(
        hooks.status.success(),
        "could not inspect planning Git hooks"
    );
    let path = Path::new(String::from_utf8_lossy(&hooks.stdout).trim()).to_path_buf();
    let hooks_path = if path.is_absolute() {
        path
    } else {
        repo.join(path)
    };
    const HOOKS: &[&str] = &[
        "applypatch-msg",
        "pre-applypatch",
        "post-applypatch",
        "pre-commit",
        "prepare-commit-msg",
        "commit-msg",
        "post-commit",
        "pre-rebase",
        "post-checkout",
        "post-merge",
        "post-rewrite",
        "reference-transaction",
        "push-to-checkout",
        "post-index-change",
    ];
    anyhow::ensure!(
        !HOOKS.iter().any(|hook| hooks_path.join(hook).is_file()),
        "planning repository contains an active Git hook"
    );
    let bare = safe_command(Some(repo))
        .args(["config", "--local", "--bool", "--get", "core.bare"])
        .output()?;
    anyhow::ensure!(
        bare.status.code() == Some(1)
            || bare.status.success() && bare.stdout.trim_ascii() == b"false",
        "planning repository may not override core.bare to true"
    );
    let signing = safe_command(Some(repo))
        .args(["config", "--local", "--bool", "--get", "commit.gpgsign"])
        .output()?;
    anyhow::ensure!(
        signing.status.code() == Some(1)
            || signing.status.success() && signing.stdout.trim_ascii() == b"false",
        "planning repository may not enable an untrusted commit signer"
    );
    for args in [
        &["remote", "get-url", "--all", "origin"][..],
        &["remote", "get-url", "--push", "--all", "origin"][..],
    ] {
        let remotes = safe_command(Some(repo)).args(args).output()?;
        if remotes.status.success() {
            let values = String::from_utf8_lossy(&remotes.stdout)
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            anyhow::ensure!(
                values.len() <= 1,
                "planning repository has multiple origin URLs or push URLs"
            );
            for remote in values {
                anyhow::ensure!(
                    !remote.starts_with('-'),
                    "planning repository remote may not begin with an option prefix"
                );
                anyhow::ensure!(
                    !is_remote_helper(&remote),
                    "planning repository uses an executable Git remote helper"
                );
                if let Some((scheme, _)) = remote.split_once("://") {
                    anyhow::ensure!(
                        matches!(
                            scheme.to_ascii_lowercase().as_str(),
                            "https" | "http" | "ssh" | "git" | "file"
                        ),
                        "planning repository uses an unsupported Git remote protocol"
                    );
                }
            }
        }
    }
    Ok(())
}

pub(super) fn is_remote_helper(remote: &str) -> bool {
    if remote.starts_with("ext::") || remote.contains("://") || remote.starts_with('[') {
        return remote.starts_with("ext::");
    }
    remote.split_once("::").is_some_and(|(transport, _)| {
        !transport.contains('/') && !transport.contains('@') && !transport.contains('[')
    })
}

pub(crate) fn validate_repository_config(repo: &Path) -> anyhow::Result<()> {
    audit_local_config(repo)
}
