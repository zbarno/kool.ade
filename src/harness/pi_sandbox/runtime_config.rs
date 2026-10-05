//! Explicit operator grants for ignored project configuration, never host env.
use super::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, process::Command};

pub(crate) const GRANT_FILE: &str = "koolade/runtime-config.json";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Grant {
    pub schema_version: u8,
    pub source_root: PathBuf,
    pub files: Vec<String>,
}

pub(crate) fn paths(root: &Path) -> anyhow::Result<BTreeSet<String>> {
    Ok(bindings(root)?
        .into_iter()
        .map(|(relative, _)| relative)
        .collect())
}

pub(super) fn mount(args: &mut Vec<String>, root: &Path) -> anyhow::Result<()> {
    for (relative, source) in bindings(root)? {
        let destination = root.join(relative);
        args.extend([
            "--ro-bind".into(),
            source.to_string_lossy().into_owned(),
            destination.to_string_lossy().into_owned(),
        ]);
    }
    Ok(())
}

fn bindings(root: &Path) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let root = root.canonicalize()?;
    let common = super::config::git_path(&root, "--git-common-dir")?.canonicalize()?;
    let path = common.join(GRANT_FILE);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    regular_inside(&common, Path::new(GRANT_FILE), true)?;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let owner = fs::metadata(&common)?.uid();
    for private in [common.join("koolade"), path.clone()] {
        let metadata = fs::metadata(private)?;
        anyhow::ensure!(
            metadata.uid() == owner && metadata.permissions().mode() & 0o077 == 0,
            "Runtime configuration grants must be private to the repository owner"
        );
    }
    anyhow::ensure!(
        fs::metadata(&path)?.len() <= 65536,
        "Runtime configuration grant is too large"
    );
    let grant: Grant = serde_json::from_slice(&fs::read(path)?)?;
    let source = grant.source_root.canonicalize()?;
    anyhow::ensure!(
        grant.schema_version == 1
            && source == grant.source_root
            && common.parent() == Some(source.as_path())
            && grant.files.len() <= 32
            && super::config::git_path(&source, "--git-common-dir")?.canonicalize()? == common,
        "Runtime configuration grant does not match this repository"
    );
    if root == source {
        return Ok(Vec::new());
    }
    let admin = super::config::git_path(&root, "--git-dir")?;
    super::config::validate_koolade_worktree(&root, &admin, &common)?;
    let mut result = Vec::new();
    let mut unique = BTreeSet::new();
    for relative in grant.files {
        let relative_path = Path::new(&relative);
        anyhow::ensure!(
            relative.len() <= 4096
                && !relative.chars().any(char::is_control)
                && relative_path
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
                    == relative,
            "Runtime configuration paths must use exact relative names"
        );
        anyhow::ensure!(
            relative_path.file_name().is_some_and(|name| name == ".env")
                && unique.insert(relative.clone()),
            "Only unique project .env files may be granted"
        );
        let source_path = regular_inside(&source, relative_path, true)?;
        regular_inside(&root, relative_path, false)?;
        anyhow::ensure!(
            fs::metadata(&source_path)?.len() <= 1024 * 1024,
            "Granted project configuration is too large"
        );
        for repository in [&source, &root] {
            anyhow::ensure!(
                ignored_untracked(repository, &relative)?,
                "Granted project configuration must be ignored and untracked in both checkouts"
            );
        }
        result.push((relative, source_path));
    }
    Ok(result)
}

fn ignored_untracked(root: &Path, relative: &str) -> anyhow::Result<bool> {
    let run = |args: &[&str]| -> anyhow::Result<std::process::Output> {
        Ok(Command::new(super::config::locate_git(root)?)
            .args(args)
            .current_dir(root)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()?)
    };
    let tracked = run(&[
        "--literal-pathspecs",
        "ls-files",
        "--error-unmatch",
        "--",
        relative,
    ])?;
    anyhow::ensure!(
        matches!(tracked.status.code(), Some(0 | 1)),
        "Could not validate runtime configuration tracking"
    );
    if tracked.status.success() {
        return Ok(false);
    }
    let ignored = run(&["check-ignore", "--quiet", "--", relative])?;
    anyhow::ensure!(
        matches!(ignored.status.code(), Some(0 | 1)),
        "Could not validate runtime configuration ignore rules"
    );
    Ok(ignored.status.success())
}

fn regular_inside(root: &Path, relative: &Path, required: bool) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_))),
        "Runtime configuration path must remain inside its repository"
    );
    let mut current = root.to_owned();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => anyhow::ensure!(
                !metadata.file_type().is_symlink(),
                "Runtime configuration paths must not contain symlinks"
            ),
            Err(error) if !required && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    if let Ok(metadata) = fs::symlink_metadata(&current) {
        anyhow::ensure!(
            metadata.is_file(),
            "Runtime configuration must be a regular file"
        );
    } else {
        anyhow::ensure!(!required, "Granted runtime configuration is missing");
    }
    Ok(current)
}
