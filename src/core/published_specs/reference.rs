use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::git::{git, validate_reference};

#[derive(Serialize, Deserialize)]
struct Designation {
    schema_version: u32,
    reference: String,
}

/// Resolve the published ref once per local project and keep it outside Git.
pub(super) fn designated_ref(repository: &Path) -> anyhow::Result<String> {
    let slug = crate::persistence::project_slug(repository);
    let project_id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, slug.as_bytes());
    let directory = crate::persistence::project_dir(&project_id.hyphenated().to_string());
    anyhow::ensure!(
        !crate::persistence::is_inside_git_worktree(&directory),
        "published-spec metadata must be stored outside Git worktrees"
    );
    fs::create_dir_all(&directory)?;
    anyhow::ensure!(
        fs::symlink_metadata(&directory)?.file_type().is_dir(),
        "published-spec metadata directory must be a local directory"
    );
    let metadata = directory.join("published-spec-ref.json");
    let lock_path = directory.join("published-spec-ref.lock");
    let lock = open_private(&lock_path, false)?;
    fs2::FileExt::lock_exclusive(&lock)?;

    if metadata.exists() {
        return read_designation(&metadata);
    }

    let reference = discover_ref(repository)?;
    let serialized = serde_json::to_vec(&Designation {
        schema_version: 1,
        reference: reference.clone(),
    })?;
    let temporary = directory.join(format!(".published-spec-ref-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = open_private(&temporary, true)?;
    file.write_all(&serialized)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&temporary, &metadata)?;
    if let Ok(parent) = OpenOptions::new().read(true).open(&directory) {
        let _ = parent.sync_all();
    }
    Ok(reference)
}

fn discover_ref(repository: &Path) -> anyhow::Result<String> {
    let remote_default = git(
        repository,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .map(|value| value.trim().to_owned())
    .filter(|value| value.starts_with("refs/remotes/origin/"));
    let reference = match remote_default {
        Some(reference) => reference,
        None => {
            let remotes = git(repository, &["remote"])?
                .lines()
                .map(str::trim)
                .filter(|remote| !remote.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if remotes.iter().any(|remote| remote == "origin") {
                anyhow::bail!(
                    "cannot determine the published-spec default: configure refs/remotes/origin/HEAD with `git remote set-head origin --auto`; the checked-out branch is not used"
                );
            }
            if !remotes.is_empty() {
                anyhow::bail!(
                    "cannot determine the published-spec default from configured remotes; configure origin and its default branch"
                );
            }

            let local_branches = git(
                repository,
                &["for-each-ref", "--format=%(refname)", "refs/heads"],
            )?
            .lines()
            .map(str::trim)
            .filter(|reference| !reference.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
            let conventional = ["refs/heads/main", "refs/heads/master", "refs/heads/trunk"]
                .into_iter()
                .find(|reference| local_branches.iter().any(|branch| branch == reference))
                .map(str::to_owned);
            conventional
                .or_else(|| (local_branches.len() == 1).then(|| local_branches[0].clone()))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "cannot determine the published-spec default in this local-only repository; keep one integration branch named main, master, or trunk"
                    )
                })?
        }
    };
    validate_reference(&reference)?;
    Ok(reference)
}

fn read_designation(path: &Path) -> anyhow::Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.file_type().is_file(),
        "published-spec ref metadata must be a regular local file"
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(4097).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= 4096,
        "published-spec ref metadata exceeds its size limit"
    );
    let designation: Designation = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        designation.schema_version == 1,
        "unsupported published-spec ref metadata version"
    );
    validate_reference(&designation.reference)?;
    Ok(designation.reference)
}

fn open_private(path: &PathBuf, create_new: bool) -> anyhow::Result<fs::File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(!create_new)
        .create_new(create_new)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    Ok(options.open(path)?)
}

#[cfg(test)]
#[path = "reference/tests.rs"]
mod tests;
