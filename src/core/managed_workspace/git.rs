use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

pub(super) fn run(root: Option<&Path>, args: &[&str]) -> anyhow::Result<Output> {
    let output = output(root, args)?;
    anyhow::ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.first().copied().unwrap_or("command"),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(output)
}

pub(super) fn output(root: Option<&Path>, args: &[&str]) -> anyhow::Result<Output> {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(root) = root {
        command.current_dir(root);
    }
    Ok(command
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()?)
}

pub(super) fn init_planning_repo(path: &Path) -> anyhow::Result<()> {
    let path_text = path.to_string_lossy();
    run(
        None,
        &[
            "-c",
            "init.templateDir=",
            "init",
            "--quiet",
            "--initial-branch=main",
            "--",
            path_text.as_ref(),
        ],
    )?;
    Ok(())
}

pub(super) fn repository_root(path: &Path) -> anyhow::Result<PathBuf> {
    let output = run(
        Some(path),
        &["rev-parse", "--path-format=absolute", "--show-toplevel"],
    )?;
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

pub(super) fn status_paths(path: &Path) -> anyhow::Result<Vec<String>> {
    let output = run(
        Some(path),
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    String::from_utf8(output.stdout)?
        .lines()
        .map(|line| {
            anyhow::ensure!(line.len() >= 4, "malformed Git status entry");
            Ok(line[3..].to_owned())
        })
        .collect()
}

pub(super) fn has_commit(path: &Path) -> anyhow::Result<bool> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(path)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()?;
    Ok(output.status.success())
}

pub(super) fn origin(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--get", "remote.origin.url"])
        .current_dir(path)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(super) fn is_work_tree(path: &Path) -> bool {
    run(Some(path), &["rev-parse", "--is-inside-work-tree"])
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "true")
}
