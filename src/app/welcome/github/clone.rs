use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{core::gitops, error::AppError};

use super::clone_destination;

/// Clone a canonical GitHub source into Kool.ad/e's managed code folder.
pub fn perform_clone(source: &str, repo: &str) -> Result<PathBuf, AppError> {
    let home = std::env::var("HOME")
        .ok()
        .filter(|home| !home.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| AppError::Io {
            op: "locate home directory".into(),
            detail: "Set $HOME to a writable directory before cloning.".into(),
        })?;
    perform_clone_at(source, repo, &home)
}

/// Clone into an injected home directory. The final directory is published
/// atomically without replacing anything another process created meanwhile.
pub fn perform_clone_at(source: &str, repo: &str, home: &Path) -> Result<PathBuf, AppError> {
    perform_clone_at_with_pre_publish(source, repo, home, |_| Ok(()))
}

/// `before_publish` records recovery proof in the clone's private Git metadata
/// and beside the destination, after Git has produced the staging checkout
/// but before the no-replace publication.
pub(crate) fn perform_clone_at_with_pre_publish(
    source: &str,
    repo: &str,
    home: &Path,
    before_publish: impl FnOnce(&Path) -> Result<(), AppError>,
) -> Result<PathBuf, AppError> {
    let destination = clone_destination(repo, home);
    reject_occupied_destination(&destination)?;
    if !home.is_dir() {
        return Err(AppError::Io {
            op: "prepare clone parent".into(),
            detail: format!(
                "$HOME points at {}, which is not a directory",
                home.to_string_lossy()
            ),
        });
    }
    let parent = destination
        .parent()
        .expect("managed clone destination has a parent");
    fs::create_dir_all(parent).map_err(|error| AppError::Io {
        op: "prepare clone parent".into(),
        detail: format!("could not create {}: {error}", parent.to_string_lossy()),
    })?;

    let scratch = parent.join(format!(
        "{repo}.koolade-cloning.{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&scratch).map_err(|error| AppError::Io {
        op: "reserve clone staging folder".into(),
        detail: format!("could not reserve {}: {error}", scratch.to_string_lossy()),
    })?;
    if let Err(error) = gitops::clone_repo(source, &scratch) {
        remove_owned_scratch(&scratch);
        return Err(error);
    }
    if !is_completed_worktree(&scratch, source) {
        remove_owned_scratch(&scratch);
        return Err(AppError::Git {
            cmd: crate::error::redact_secrets(&format!("clone {source}")),
            detail: "Git did not produce the requested repository in its staging folder".into(),
        });
    }
    if let Err(error) = before_publish(&scratch) {
        remove_owned_scratch(&scratch);
        return Err(error);
    }

    if let Err(error) = crate::core::managed_workspace::publish_directory(&scratch, &destination) {
        remove_owned_scratch(&scratch);
        return Err(AppError::Io {
            op: "place cloned repository".into(),
            detail: format!("could not move the clone into place: {error:#}"),
        });
    }
    Ok(destination)
}

pub(crate) fn reject_occupied_destination(destination: &Path) -> Result<(), AppError> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            let detail = if metadata.file_type().is_symlink() {
                "managed clone destination is a symbolic link; Kool.ad/e will not adopt it"
            } else if metadata.is_dir() {
                "managed clone destination already exists; Kool.ad/e will not adopt an existing checkout"
            } else {
                "a file already occupies that name"
            };
            Err(AppError::InvalidRepo {
                path: destination.to_string_lossy().into_owned(),
                detail: detail.into(),
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Io {
            op: "inspect clone destination".into(),
            detail: format!(
                "could not inspect {}: {error}",
                destination.to_string_lossy()
            ),
        }),
    }
}

fn is_completed_worktree(path: &Path, source: &str) -> bool {
    let git_dir = path.join(".git");
    fs::symlink_metadata(&git_dir).is_ok_and(|metadata| {
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && gitops::is_work_tree(path)
            && gitops::origin_url(path).as_deref() == Some(source)
    })
}

fn remove_owned_scratch(path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}
