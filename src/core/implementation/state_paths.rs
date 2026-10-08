use super::*;

pub(super) fn key(ticket: &str) -> String {
    format!(
        "{}-{:016x}",
        crate::artifacts::task_docs::slug(
            Path::new(ticket)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("ticket")
        ),
        crate::persistence::fnv1a64(ticket.as_bytes())
    )
}

pub(crate) fn key_for_ticket(ticket: &str) -> String {
    key(ticket)
}
pub(super) fn common(repo: &Path) -> anyhow::Result<PathBuf> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(repo)
        .output()?;
    anyhow::ensure!(output.status.success(), "Cannot locate Git metadata");
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}
pub(crate) fn state_dir(repo: &Path, ticket: &str) -> anyhow::Result<PathBuf> {
    let name = key(ticket);
    let private = private_implementation_root(repo)?.join(&name);
    let legacy = legacy_implementation_root(repo).join(&name);
    let private_state = private.join("state.json").exists();
    let legacy_state = legacy.join("state.json").exists();
    anyhow::ensure!(
        !(private_state && legacy_state),
        "Task has both private and legacy implementation records; review them before resuming"
    );
    if legacy_state {
        Ok(legacy)
    } else {
        Ok(private)
    }
}

fn state_dir_by_task_uid(repo: &Path, uid: &str) -> anyhow::Result<Option<PathBuf>> {
    let mut matches = Vec::new();
    for root in implementation_roots(repo)? {
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries.collect::<Result<Vec<_>, _>>()?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            if !entry
                .file_type()
                .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
            {
                continue;
            }
            let state_path = entry.path().join("state.json");
            if read_state_file(&state_path)
                .ok()
                .is_some_and(|state| state.task_uid.as_deref() == Some(uid))
            {
                matches.push(entry.path());
            }
        }
    }
    anyhow::ensure!(
        matches.len() <= 1,
        "Multiple implementation records share task identity {uid}"
    );
    Ok(matches.pop())
}

pub(super) fn implementation_roots(repo: &Path) -> anyhow::Result<Vec<PathBuf>> {
    Ok(vec![
        private_implementation_root(repo)?,
        legacy_implementation_root(repo),
    ])
}

fn private_implementation_root(repo: &Path) -> anyhow::Result<PathBuf> {
    let project_id = crate::persistence::project_slug(&repo.canonicalize()?);
    Ok(crate::persistence::project_dir(&project_id).join("implementations"))
}

fn legacy_implementation_root(repo: &Path) -> PathBuf {
    crate::artifacts::layout::ArtifactLayout::new(repo).implementation_root()
}

pub(crate) fn state_dir_for_task(
    repo: &Path,
    ticket: &str,
    task_uid: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let direct = state_dir(repo, ticket)?;
    if direct.join("state.json").exists() {
        return Ok(direct);
    }
    if let Some(uid) = task_uid
        && let Some(existing) = state_dir_by_task_uid(repo, uid)?
    {
        return Ok(existing);
    }
    Ok(direct)
}
