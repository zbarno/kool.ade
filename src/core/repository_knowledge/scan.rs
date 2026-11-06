use sha2::Digest;
use std::path::{Path, PathBuf};

use super::{
    RepositoryKnowledgeProfileV1, RepositoryRevisionKeyV1,
    source::{MAX_BLOB_BYTES, command_bytes, command_text, valid_oid, valid_path},
};
mod build;
mod cache;
mod compute;
mod git;
use crate::{artifacts::planning_store::PlanningStore, core::project_repos::ProjectManifest};
use build::scan_uncached;
use cache::{cached, save_cache};
use git::{
    checkout_is_dirty, normalize_ref, read_regular_blob as regular_blob, resolve_commit,
    resolve_root,
};

const MAX_FACTS: usize = 128;

/// Request for one explicitly selected repository ref. Callers must bind a
/// task to its source/destination ref before requesting a profile.
pub struct ProfileRequest<'a> {
    pub repository_id: &'a str,
    pub source_ref: &'a str,
    pub code_root: &'a Path,
    pub planning_store: &'a PlanningStore,
    pub repositories: &'a ProjectManifest,
    pub now_unix_ms: u64,
}

/// Owned source coordinates for a profile and subsequent exact-file reads.
#[derive(Clone)]
pub struct ProfileSource {
    pub repository_id: String,
    pub source_ref: String,
    pub code_root: PathBuf,
    pub planning_store: PlanningStore,
    pub repositories: ProjectManifest,
}

impl<'a> From<ProfileRequest<'a>> for ProfileSource {
    fn from(request: ProfileRequest<'a>) -> Self {
        Self {
            repository_id: request.repository_id.to_owned(),
            source_ref: request.source_ref.to_owned(),
            code_root: request.code_root.to_path_buf(),
            planning_store: request.planning_store.clone(),
            repositories: request.repositories.clone(),
        }
    }
}

/// Build or load the bounded local profile for one repository/ref/commit key.
pub fn scan_profile(request: ProfileRequest<'_>) -> Result<RepositoryKnowledgeProfileV1, String> {
    let now = request.now_unix_ms.max(1);
    let source = ProfileSource::from(request);
    let root = resolve_root(&source)?;
    let source_ref = normalize_ref(&source.source_ref)?;
    let commit_sha = resolve_commit(&root, &source_ref)?;
    let dirty_checkout = checkout_is_dirty(&root)?;
    let key = RepositoryRevisionKeyV1 {
        repository_id: source.repository_id.clone(),
        source_ref,
        commit_sha,
        dirty_checkout,
    };
    if let Some(profile) = cached(&source.planning_store, &key) {
        return Ok(profile);
    }
    let profile = scan_uncached(&root, key, now)?;
    save_cache(&source.planning_store, profile.clone());
    Ok(profile)
}

/// Re-read one selected file from the exact commit recorded by a profile.
/// The worktree and index are never consulted for file contents.
pub fn read_pinned_file(
    source: &ProfileSource,
    key: &RepositoryRevisionKeyV1,
    relative_path: &str,
) -> Result<(String, String), String> {
    if source.repository_id != key.repository_id
        || normalize_ref(&source.source_ref)? != key.source_ref
        || !valid_oid(&key.commit_sha)
        || !valid_path(relative_path)
    {
        return Err("profile file request does not match its pinned repository identity".into());
    }
    let root = resolve_root(source)?;
    let listing = command_bytes(
        &root,
        &[
            "ls-tree",
            "-z",
            &key.commit_sha,
            "--",
            &format!(":(literal){relative_path}"),
        ],
    )?;
    let object_id = regular_blob(&listing, relative_path)?;
    let size = command_text(&root, &["cat-file", "-s", &object_id])?
        .trim()
        .parse::<usize>()
        .map_err(|_| "Git returned an invalid blob size".to_owned())?;
    if size > MAX_BLOB_BYTES {
        return Err("selected repository document exceeds the size limit".into());
    }
    let bytes = command_bytes(&root, &["cat-file", "blob", &object_id])?;
    if bytes.len() != size {
        return Err("selected repository blob changed while being read".into());
    }
    let digest = format!("{:x}", sha2::Sha256::digest(&bytes));
    let content = String::from_utf8(bytes)
        .map_err(|_| "selected repository document is not valid UTF-8".to_owned())?;
    Ok((content, digest))
}
