use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

pub(crate) fn project_id(root: &Path) -> anyhow::Result<String> {
    let root = root.canonicalize()?;
    if let Some(project_id) = task_clone_project_id(&root)? {
        return Ok(project_id);
    }
    Ok(crate::persistence::project_slug(&git_project_root(&root)?))
}

fn git_project_root(root: &Path) -> anyhow::Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Cannot locate project Git metadata for dependency authorization"
    );
    let common = PathBuf::from(String::from_utf8(output.stdout)?.trim()).canonicalize()?;
    common
        .parent()
        .unwrap_or(&common)
        .canonicalize()
        .map_err(Into::into)
}

fn task_clone_project_id(root: &Path) -> anyhow::Result<Option<String>> {
    let state_root = crate::persistence::state_root();
    let Ok(state_root) = state_root.canonicalize() else {
        return Ok(None);
    };
    let projects = state_root.join("projects");
    let Ok(projects) = projects.canonicalize() else {
        return Ok(None);
    };
    let Ok(relative) = root.strip_prefix(&projects) else {
        return Ok(None);
    };
    let mut components = relative.components();
    let Some(Component::Normal(project_id)) = components.next() else {
        return Ok(None);
    };
    let Some(Component::Normal(task_repositories)) = components.next() else {
        return Ok(None);
    };
    let Some(Component::Normal(repository_id)) = components.next() else {
        return Ok(None);
    };
    let Some(Component::Normal(task_key)) = components.next() else {
        return Ok(None);
    };
    if components.next().is_some() || task_repositories != "task-repositories" {
        return Ok(None);
    }
    let Some(project_id) = project_id.to_str() else {
        return Ok(None);
    };
    let Some(repository_id) = repository_id.to_str() else {
        return Ok(None);
    };
    let Some(task_key) = task_key.to_str() else {
        return Ok(None);
    };
    let valid_component = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    };
    if project_id.len() > 128
        || !valid_component(project_id)
        || repository_id.len() > 40
        || !valid_component(repository_id)
        || task_key.len() > 128
        || !valid_component(task_key)
    {
        return Ok(None);
    }
    let metadata = match fs::symlink_metadata(root.join(".git")) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(None);
    }
    Ok(Some(project_id.to_owned()))
}
