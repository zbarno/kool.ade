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
    // State belongs to the repository's Packet workspace, not to .git. This
    // keeps resumable implementation evidence visible in the local workspace.
    // Git ignores this directory; moving a live workspace requires a separate
    // backup of these files.
    Ok(crate::artifacts::layout::ArtifactLayout::new(repo)
        .implementation_root()
        .join(key(ticket)))
}

fn state_dir_by_task_uid(repo: &Path, uid: &str) -> anyhow::Result<Option<PathBuf>> {
    let root = crate::artifacts::layout::ArtifactLayout::new(repo).implementation_root();
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries.collect::<Result<Vec<_>, _>>()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut matches = Vec::new();
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
    anyhow::ensure!(
        matches.len() <= 1,
        "Multiple implementation records share task identity {uid}"
    );
    Ok(matches.pop())
}

pub(super) fn state_dir_for_task(
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
